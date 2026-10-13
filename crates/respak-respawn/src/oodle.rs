//! Oodle decompression (Kraken, Mermaid, Leviathan), decode only.
//!
//! Wraps the `oozextract` crate (a port of ooz). rsx calls the proprietary oo2core through
//! `RTech::DecompressStreamedBuffer` (`src/game/rtech/utils/utils.cpp`); that library is never
//! linked here. Unlike rsx, a failed or short decode is an error, never the input returned as is.
//!
//! A stream is a run of blocks of 256 KiB of output, each with a 2-byte header. A block with the
//! restart bit starts a new window, so the stream cuts into independent segments at those blocks
//! (Apex: every 16 MiB). `oozextract` ignores the bit, so [`Scanner`] finds the cuts and each
//! [`Segment`] is decoded on its own. That also lets a caller decode only the segments it needs.

use oozextract::Extractor;
use thiserror::Error;

/// Output bytes per block (Kraken, Mermaid, Leviathan).
const BLOCK: usize = 0x40000;

/// Errors produced while decompressing an Oodle stream.
#[derive(Debug, Error)]
pub enum OodleError {
    /// The stream is truncated, corrupt, or decodes to another length than expected.
    #[error("Oodle stream corrupt: {0}")]
    Corrupt(String),
    /// The stream uses something this decoder does not handle.
    #[error("Oodle stream unsupported: {0}")]
    Unsupported(String),
    /// Only for [`Scanner::advance`] on a partial input: the scanner needs at least this many bytes
    /// of the stream (counted from its start) to go on.
    #[error("Oodle stream needs at least {at_least} bytes of input")]
    NeedInput {
        /// A lower bound for the input length that lets the scanner make progress.
        at_least: usize,
    },
}

fn corrupt(what: impl Into<String>) -> OodleError {
    OodleError::Corrupt(what.into())
}

/// Where an independent run of blocks starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Offset of its first block header in the stream.
    pub input: usize,
    /// Offset of its first byte in the decoded output.
    pub output: usize,
}

/// Walks the block headers of a stream and records where the segments start.
#[derive(Debug, Default)]
pub struct Scanner {
    /// Start of the next block header.
    input: usize,
    /// Decoded bytes before that block.
    output: usize,
    /// The segments found so far; the first is at 0 once any block was seen.
    pub segments: Vec<Segment>,
}

impl Scanner {
    /// How many decoded bytes have been walked.
    pub fn scanned(&self) -> usize {
        self.output
    }

    /// Where the next block header is in the stream.
    pub fn position(&self) -> usize {
        self.input
    }

    /// Walks blocks until at least `upto` decoded bytes are covered (or the `decoded_len` end).
    /// `input` is the stream from its start; it may be a prefix. A block counts only once all of
    /// it is in `input`, else the error is [`OodleError::NeedInput`] and the scanner can go on
    /// with a longer input.
    pub fn advance(&mut self, input: &[u8], decoded_len: usize, upto: usize) -> Result<(), OodleError> {
        let upto = upto.min(decoded_len);
        while self.output < upto {
            let at = self.input;
            let need = |n: usize| OodleError::NeedInput { at_least: at + n };
            let head = input.get(at..at + 2).ok_or_else(|| need(2))?;
            if head[0] & 0xF != 0xC || (head[0] >> 4) & 3 != 0 {
                return Err(corrupt(format!("bad block header {:02x}{:02x} at {at}", head[0], head[1])));
            }
            if !matches!(head[1] & 0x7F, 6 | 0xA | 0xC) {
                return Err(OodleError::Unsupported(format!("decoder type {:#x}", head[1] & 0x7F)));
            }
            if head[1] & 0x80 != 0 {
                return Err(OodleError::Unsupported("block checksums".into()));
            }
            let out = BLOCK.min(decoded_len - self.output);
            let body = if head[0] & 0x40 != 0 {
                out
            } else {
                let q = input.get(at + 2..at + 5).ok_or_else(|| need(5))?;
                let v = usize::from(q[0]) << 16 | usize::from(q[1]) << 8 | usize::from(q[2]);
                match (v & 0x3FFFF, v >> 18) {
                    (0x3FFFF, 1) => 4,
                    (0x3FFFF, _) => return Err(OodleError::Unsupported("whole-match block".into())),
                    (size, _) => 3 + size + 1,
                }
            };
            let end = (at + 2).checked_add(body).ok_or_else(|| corrupt("block size"))?;
            if input.len() < end {
                return Err(need(end - at));
            }
            if head[0] & 0x80 != 0 || self.segments.is_empty() {
                self.segments.push(Segment { input: at, output: self.output });
            }
            self.input = end;
            self.output += out;
        }
        Ok(())
    }
}

/// Decodes the segment starting at the start of `input` into `out`, which must be exactly its
/// decoded length. A panic inside the decoder becomes `Corrupt`.
pub fn decode_segment(input: &[u8], out: &mut [u8]) -> Result<(), OodleError> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Extractor::new().read_from_slice(input, out)));
    match result {
        Ok(Ok(n)) if n == out.len() => Ok(()),
        Ok(Ok(n)) => Err(corrupt(format!("decoded {n} bytes, expected {}", out.len()))),
        Ok(Err(e)) => Err(corrupt(e.to_string())),
        Err(_) => Err(corrupt("the decoder panicked")),
    }
}

/// Decodes `input` to exactly `decoded_len` bytes. Bytes after the end of the stream (the padding
/// of a streamed block) are ignored.
pub fn decompress(input: &[u8], decoded_len: u64) -> Result<Vec<u8>, OodleError> {
    let len = usize::try_from(decoded_len).map_err(|_| corrupt("decoded length"))?;
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| corrupt(format!("decoded length {len} is too large")))?;
    out.resize(len, 0);
    let mut scan = Scanner::default();
    scan.advance(input, len, len).map_err(|e| match e {
        OodleError::NeedInput { .. } => corrupt("the stream ends early"),
        e => e,
    })?;
    for (k, seg) in scan.segments.iter().enumerate() {
        let end = scan.segments.get(k + 1).map_or(len, |s| s.output);
        decode_segment(&input[seg.input..], &mut out[seg.output..end])?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored block (no compression): header, then the raw bytes.
    fn stored(restart: bool, data: &[u8]) -> Vec<u8> {
        let mut v = vec![0x4C | if restart { 0x80 } else { 0 }, 0x06];
        v.extend_from_slice(data);
        v
    }

    fn pattern(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect()
    }

    #[test]
    fn stored_blocks_round_trip() {
        let data = pattern(BLOCK + 1000, 1);
        let mut stream = stored(true, &data[..BLOCK]);
        stream.extend(stored(false, &data[BLOCK..]));
        assert_eq!(decompress(&stream, data.len() as u64).unwrap(), data);
    }

    #[test]
    fn restart_blocks_cut_segments() {
        let data = pattern(3 * BLOCK, 2);
        let mut stream = Vec::new();
        for (i, chunk) in data.chunks(BLOCK).enumerate() {
            stream.extend(stored(i == 0 || i == 2, chunk));
        }
        let mut scan = Scanner::default();
        scan.advance(&stream, data.len(), data.len()).unwrap();
        let outputs: Vec<_> = scan.segments.iter().map(|s| s.output).collect();
        assert_eq!(outputs, [0, 2 * BLOCK]);
        assert_eq!(scan.segments[1].input, 2 * (BLOCK + 2));
        assert_eq!(decompress(&stream, data.len() as u64).unwrap(), data);
    }

    #[test]
    fn padding_after_the_stream_is_ignored() {
        let data = pattern(5000, 3);
        let mut stream = stored(true, &data);
        stream.resize(4096 * 2, 0);
        assert_eq!(decompress(&stream, data.len() as u64).unwrap(), data);
    }

    #[test]
    fn a_partial_input_asks_for_more() {
        let data = pattern(BLOCK, 4);
        let stream = stored(true, &data);
        let mut scan = Scanner::default();
        match scan.advance(&stream[..100], data.len(), data.len()) {
            Err(OodleError::NeedInput { at_least }) => assert_eq!(at_least, stream.len()),
            other => panic!("{other:?}"),
        }
        scan.advance(&stream, data.len(), data.len()).unwrap();
        assert_eq!(scan.scanned(), BLOCK);
    }

    #[test]
    fn truncated_or_wrong_length_is_corrupt() {
        let data = pattern(5000, 5);
        let stream = stored(true, &data);
        assert!(matches!(decompress(&stream[..3000], 5000), Err(OodleError::Corrupt(_))));
        assert!(matches!(decompress(&stream, 6000), Err(OodleError::Corrupt(_))));
        assert!(matches!(decompress(&[], 10), Err(OodleError::Corrupt(_))));
    }

    #[test]
    fn garbage_is_corrupt_and_does_not_panic() {
        for seed in 0..64u8 {
            let junk = pattern(2000, seed);
            let mut stream = vec![0x8C, 0x06, 0x00, 0x03, 0xFF];
            stream.extend(junk);
            // Whatever it decodes to, it must come back as an error or the exact length.
            if let Ok(out) = decompress(&stream, 70_000) {
                assert_eq!(out.len(), 70_000);
            }
        }
        assert!(matches!(decompress(b"not oodle at all", 100), Err(OodleError::Corrupt(_))));
    }

    #[test]
    fn unsupported_blocks_are_reported() {
        let mut stream = stored(true, &[0; 10]);
        stream[1] |= 0x80; // checksums
        assert!(matches!(decompress(&stream, 10), Err(OodleError::Unsupported(_))));
        let mut stream = stored(true, &[0; 10]);
        stream[1] = 0x05; // LZNA
        assert!(matches!(decompress(&stream, 10), Err(OodleError::Unsupported(_))));
    }
}
