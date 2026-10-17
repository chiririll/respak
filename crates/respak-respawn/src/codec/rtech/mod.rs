//! RTech "pakfile" decompressor (the LZ variant used by `.rpak` v7/v8 files with the
//! `RTECH` compression flag).
//!
//! Ported from the C++ tool rsx (`src/game/rtech/utils/utils.cpp`): `RTech::InitPakDecoder`
//! and `RTech::DecompressPakFile`, plus the lookup table `s_PakFileCompressionLUT` from
//! `src/game/rtech/utils/utils.h`. The algorithm is ported, not the pointer arithmetic: all
//! reads and writes are bounds-checked and there is no `unsafe`.
//!
//! # Stream format (as understood from the decoder)
//!
//! The compressed stream starts at byte `header_len` of the file (the pak header is stored
//! uncompressed in front of it) and begins with a little-endian *bit* header:
//!
//! | bits      | meaning                                                                         |
//! |-----------|---------------------------------------------------------------------------------|
//! | 6         | `n`                                                                             |
//! | `n`       | mantissa; total decompressed size = `(1 << n) \| mantissa` (includes the header) |
//! | 6         | input window bits `w` (0 means 64): input is laid out in `2^w`-byte windows     |
//! | 6         | output window bits `o` (0 means 64): output is produced in `2^o`-byte chunks    |
//! | 1         | unused                                                                          |
//!
//! The decompressed size counts the uncompressed pak header too; output positions start at
//! `header_len` (rsx writes the decoder output at offset `header_len` of a `dcmpSize` buffer).
//! [`decompress`] returns only the part after the header.
//!
//! If the input window is 64 bits there is a single chunk and the whole file is its data.
//! Otherwise the data is split into chunks of `2^o` output bytes (positions counted from the
//! start of the file image, so the first chunk is `2^o - header_len` bytes). Every chunk is
//! preceded in the stream by a size field of `(w >> 3) + 1` bytes (little endian) holding the
//! chunk's compressed size; the chunk's end is tracked as an absolute file position. Between
//! chunks the decoder discards one bit. Input positions are absolute file offsets and are
//! aligned to the `2^w` window: when the read-ahead pointer comes within 7 bytes of the
//! window end it skips to the next window boundary.
//!
//! The payload is a mix of a bit stream and raw literal bytes. The decoder keeps a 64-bit bit
//! buffer that is always refilled from a byte read pointer `rd`; the buffer holds the bits
//! *before* `rd`. Literal runs are raw bytes copied from `rd` (and advance it), so they are
//! skipped by the bit stream. Each op starts with a table lookup on the next 8 bits, with
//! two states (previous op was a match = state 0, was a literal run = state 1):
//!
//! * `LUT[state*256 + b]` is the op and `LUT[512 + state*256 + b]` its bit length.
//! * negative op `-k`: literal run of `k` bytes. If `k` equals the escape value (17 after a
//!   match, 1 after a literal run) an extended length follows (see below) which is added.
//! * op `1..=16`: match of that length. Op 17: long match, with extended length added to 17.
//! * match distance: 4 bits `e` (if 15, two more bits extend it by 0..3), then a 6-bit
//!   look-up prefix code (`LUT[1024 + i]` = low nibble, `LUT[1088 + i]` = code length), then
//!   `e` mantissa bits: `distance = 16 * ((1 << e) + mantissa) + nibble - 16`.
//! * extended length: 3-bit selector; non-zero selects `(base, bits)` from small tables
//!   (`LUT[1232..]`, `LUT[1240..]`); zero reads 4 more bits selecting from 16-entry tables
//!   (u32 bases at `LUT[1152..]`, bit counts at `LUT[1216..]`); then `bits` raw bits are
//!   added to the base.
//! * for a long match with distance < 8, the length is `extended + 4` rather than
//!   `extended + 17` (the encoder never uses the 1..=16 ops for distances below 8).
//!
//! The stream ends when the output reaches the decompressed size.

use thiserror::Error;

mod decoder;
mod lut;

use decoder::{low_mask, Decoder, Header};

/// Errors produced while decompressing an RTech stream.
#[derive(Debug, Error)]
pub enum RtechError {
    /// The input ended before the stream was fully decoded.
    #[error("RTech stream truncated: {0}")]
    Truncated(&'static str),
    /// The stream is internally inconsistent.
    #[error("RTech stream corrupt: {0}")]
    Corrupt(&'static str),
    /// The size stored in the stream does not match the size the caller expects.
    #[error("RTech decompressed size mismatch: stream says {actual} bytes, expected {expected}")]
    SizeMismatch {
        /// Size the caller expected (bytes after the header).
        expected: u64,
        /// Size found in the stream (bytes after the header).
        actual: u64,
    },
    /// The stream announces an output too large to allocate.
    #[error("RTech decompressed size {0} is too large")]
    TooLarge(u64),
    /// Only for [`decompress_prefix`] on a partial input: the decoder needs at least this many bytes
    /// of the file (counted from its start) to go on.
    #[error("RTech stream needs at least {at_least} bytes of input")]
    NeedInput {
        /// A lower bound for the input length that lets the decoder make progress.
        at_least: u64,
    },
}
/// Largest output we are willing to allocate when the caller gave no expected length.
const MAX_OUTPUT: u64 = 1 << 32;

/// Decompresses the body of an RTech-compressed .rpak.
///
/// `file` is the whole compressed file; the stream header starts at byte offset `header_len`
/// (0x58 for v7, 0x80 for v8). Returns the decompressed bytes that follow the header
/// (i.e. the "file image" minus the first `header_len` bytes), exactly `expected_len` long
/// when `expected_len` is `Some`.
///
/// # Errors
///
/// Returns an error if the stream is truncated or corrupt, or if its decompressed size does
/// not match `expected_len`. Never panics on malformed input.
pub fn decompress(
    file: &[u8],
    header_len: usize,
    expected_len: Option<u64>,
) -> Result<Vec<u8>, RtechError> {
    decompress_prefix(file, true, header_len, expected_len, u64::MAX).map(|p| p.body)
}

/// The start of a decompressed body.
#[derive(Debug)]
pub struct Prefix {
    /// The decoded body bytes (after the pak header). At least `want` long unless the body is shorter;
    /// the stream is cut at an output chunk boundary, so usually somewhat longer.
    pub body: Vec<u8>,
    /// Whether `body` is the whole body.
    pub complete: bool,
}

/// Decodes only the first `want` bytes of the body (rounded up to an output chunk), from a prefix of
/// the file. The stream is split into output chunks, each with its compressed size in front, so
/// the decoder can stop at a chunk boundary and only needs the input up to there.
///
/// `file` is the file from its start; `input_complete` says it is the whole file. If it is not and
/// the decoder needs more, the error is [`RtechError::NeedInput`]: read more and call again (the
/// decoding restarts from the beginning, but costs no I/O). Streams with a single output chunk
/// cannot be cut and are decoded whole.
///
/// # Errors
///
/// As [`decompress`], plus `NeedInput` when `input_complete` is false.
pub fn decompress_prefix(
    file: &[u8],
    input_complete: bool,
    header_len: usize,
    expected_len: Option<u64>,
    want: u64,
) -> Result<Prefix, RtechError> {
    decode(file, !input_complete, header_len, expected_len, want).map_err(|e| match e {
        RtechError::Truncated(_) if !input_complete => RtechError::NeedInput {
            at_least: file.len() as u64 + 1,
        },
        e => e,
    })
}

fn decode(
    file: &[u8],
    partial: bool,
    header_len: usize,
    expected_len: Option<u64>,
    want: u64,
) -> Result<Prefix, RtechError> {
    let h = parse_header(file, header_len)?;
    let hl = header_len as u64;
    if h.size < hl {
        return Err(RtechError::Corrupt("decompressed size smaller than header"));
    }
    let body_len = h.size - hl;
    if let Some(exp) = expected_len {
        if exp != body_len {
            return Err(RtechError::SizeMismatch {
                expected: exp,
                actual: body_len,
            });
        }
    } else if h.size > MAX_OUTPUT {
        return Err(RtechError::TooLarge(h.size));
    }

    let mut dec = Decoder::new(file, partial, header_len, &h, want)?;
    dec.run()?;
    Ok(dec.finish(header_len))
}

/// Reads the bit header of the stream starting at byte `header_len` of `file`.
fn parse_header(file: &[u8], header_len: usize) -> Result<Header, RtechError> {
    let hdr_end = header_len
        .checked_add(16)
        .ok_or(RtechError::Truncated("stream header"))?;
    let hdr = file
        .get(header_len..hdr_end.min(file.len()))
        .ok_or(RtechError::Truncated("stream header"))?;
    if file.len() < header_len + 8 {
        return Err(RtechError::Truncated("stream header"));
    }
    let mut hb = [0u8; 16];
    hb[..hdr.len()].copy_from_slice(hdr);
    let x = u128::from_le_bytes(hb);

    let n = (x & 0x3F) as u32;
    let size = (((x >> 6) as u64) & low_mask(u64::from(n))) | (1u64 << n);
    let mut sh = 6 + n;
    let w = ((x >> sh) & 0x3F) as u64;
    sh += 6;
    let o = ((x >> sh) & 0x3F) as u64;
    sh += 7; // 6 bits + 1 unused bit
    Ok(Header {
        size,
        in_bits: if w == 0 { 64 } else { w },
        out_bits: if o == 0 { 64 } else { o },
        payload_bit: sh,
    })
}

#[cfg(test)]
mod tests;
