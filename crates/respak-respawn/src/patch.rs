//! The edit stream of a patched pak: how the pages that the patch does not store are rebuilt from
//! the lower files of the chain.
//!
//! Ported from rsx `src/game/rtech/cpakfile.cpp` (`ParsePatchEditStream`, `DecodePatchCommands`)
//! and `patchapi.cpp` (`PakPatch_DecodeData`, `PatchCmd_0` .. `PatchCmd_6`). rsx runs the script
//! over memory it allocates per segment; here it only produces a **map**: for every byte range of
//! the output, where it comes from. A read of the pak's flat page memory follows the map into the
//! lower files, so nothing is decoded that is not asked for.
//!
//! # Inputs
//! - `F`: the files of the chain, bodies back to back: the top file's body, then the body of each
//!   lower file in the order of the patch block's index list. The script starts at the top file's
//!   own page data (an implicit copy of its stored pages) and continues into the lower files.
//! - `S`: the replacement data, at the end of the edit stream.
//! - The output `D` is the pages in load order: pages `patch_pages..` (the stored ones) first, then
//!   `0..patch_pages`. [`PageMap::locate`] takes `.mem` offsets (page index order) and does the
//!   rotation.
//!
//! # The stream
//! ```text
//! table A   Huffman table, depth 6:  command + code length
//! table B   Huffman table, depth 8:  size exponent + code length
//! u64 word  low 24 bits = offset of S from the end of table B; the bit stream starts here, after
//!           those 24 bits
//! bits      LSB-first
//! S         replacement data
//! ```
//! Each command: a 6-bit lookup in A. Commands 0 to 3 then look up 8 bits in B for an exponent `e`
//! and read `e` more bits: the count is `(1 << e) + bits`. Commands 4 to 6 have a fixed count.
//!
//! | cmd | effect |
//! |---|---|
//! | 0 | copy `n` bytes of F |
//! | 1 | skip `n` bytes of F |
//! | 2 | insert `n` bytes of S (F stays) |
//! | 3 | replace: `n` bytes of S, F moves on by `n` |
//! | 4, 5 | one byte of S (F moves by 1), then copy 3 or 7 bytes of F |
//! | 6 | two bytes of S (F moves by 2), then copy 6 bytes of F |
//!
//! # Differences from rsx
//! rsx skips bytes at the places where it re-packs asset headers in memory (`numBytesToSkip`);
//! this map has no such gaps. rsx clamps a command that runs past the end of F or of the output
//! and goes on; here a command past the end of F is checked when it is read, and the last command
//! may run past the end of the output (it is cut). Every other malformed input is `Corrupt`.

use respak_core::{Error, ErrorKind, Result};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// Where a run of output bytes comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `F`, from this offset.
    File(u64),
    /// `S`, from this offset.
    Replacement(u64),
}

/// `len` bytes of output at `at` (in `D`), taken from `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// Offset in `D`.
    pub at: u64,
    /// Length.
    pub len: u64,
    /// Where the bytes come from.
    pub source: Source,
}

/// A piece of a read: `len` bytes from `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Where the bytes come from.
    pub source: Source,
    /// Length.
    pub len: u64,
}

/// A Huffman table of the stream: for each `depth`-bit lookup, the symbol and the code length.
struct Table {
    symbol: Vec<u8>,
    bits: Vec<u8>,
}

/// Bit-reverses the low `n` bits of `v`.
fn reverse(v: u32, n: u32) -> u32 {
    (v.reverse_bits() >> 24) >> (8 - n)
}

/// Reads one table from the start of `buf` (`PakPatch_DecodeData`) and returns it with the number
/// of bytes it used. A byte holds `symbol << 3 | (code length - 1)`; entries come in order of
/// ascending code length and are assigned canonical codes, stored bit-reversed because the stream
/// is read LSB-first.
fn decode_table(buf: &[u8], depth: u32) -> Result<(Table, usize)> {
    let size = 1usize << depth;
    let (mut symbol, mut bits) = (vec![0u8; size], vec![0u8; size]);
    let mut remaining = size;
    let (mut code, mut offset, mut previous) = (0u32, 0usize, 0u32);
    while remaining > 0 {
        let byte = *buf.get(offset).ok_or_else(|| corrupt("rpak patch: a Huffman table runs past the edit stream"))?;
        let len = u32::from(byte & 7) + 1;
        if offset > 0 {
            // The next code: one past the last, with the extra bits of a longer code appended.
            if len < previous {
                return Err(corrupt("rpak patch: Huffman code lengths are not in ascending order"));
            }
            code = (code + 1) << (len - previous);
        }
        previous = len;
        if code >= 1 << len {
            return Err(corrupt("rpak patch: Huffman codes overflow"));
        }
        let mut j = reverse(code, len) as usize;
        while j < size {
            if bits[j] != 0 {
                return Err(corrupt("rpak patch: Huffman codes overlap"));
            }
            symbol[j] = byte >> 3;
            bits[j] = len as u8;
            remaining -= 1;
            j += 1 << len;
        }
        offset += 1;
    }
    Ok((Table { symbol, bits }, offset))
}

/// A little-endian bit reader over the stream, LSB first.
struct Bits<'a> {
    data: &'a [u8],
    pos: u64,
}

impl Bits<'_> {
    /// The next `n` (at most 32) bits without consuming them; bits past the end read as zero.
    fn peek(&self, n: u32) -> u64 {
        let first = (self.pos / 8) as usize;
        let mut word = [0u8; 8];
        if first < self.data.len() {
            let have = (self.data.len() - first).min(8);
            word[..have].copy_from_slice(&self.data[first..first + have]);
        }
        (u64::from_le_bytes(word) >> (self.pos % 8)) & ((1u64 << n) - 1)
    }

    fn skip(&mut self, n: u32) -> Result<()> {
        self.pos += u64::from(n);
        if self.pos > self.data.len() as u64 * 8 {
            return Err(corrupt("rpak patch: the edit stream's bits run out"));
        }
        Ok(())
    }
}

/// The map from `D` to its sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMap {
    runs: Vec<Run>,
    /// Bytes of the pages `patch_pages..` (they come first in `D`).
    stored_len: u64,
    /// Bytes of the whole output.
    len: u64,
    /// Offset of S in the edit stream.
    pub replacement_at: usize,
}

impl PageMap {
    /// Runs the script of `stream` (the whole edit stream).
    ///
    /// - `len`: the size of all pages; `stored_len`: the size of those the file stores (the
    ///   pages from `patch_pages` on).
    /// - `first`: the offset in F of the file's own page data. The first `stored_len` bytes of
    ///   the output are copied from there.
    pub fn build(stream: &[u8], len: u64, stored_len: u64, first: u64) -> Result<Self> {
        if stored_len > len {
            return Err(corrupt("rpak patch: the stored pages are larger than the pak"));
        }
        let (a, used_a) = decode_table(stream, 6)?;
        let (b, used_b) = decode_table(&stream[used_a..], 8)?;
        let base = used_a + used_b;
        let word = stream.get(base..base + 8).ok_or_else(|| corrupt("rpak patch: the edit stream is too short"))?;
        let replacement_at = base + (u32::from_le_bytes(word[..4].try_into().expect("4 bytes")) & 0xFF_FFFF) as usize;
        if replacement_at < base + 8 || replacement_at > stream.len() {
            return Err(corrupt("rpak patch: the replacement data is outside the edit stream"));
        }
        let mut bits = Bits { data: &stream[base..replacement_at], pos: 24 };

        let mut map = Self { runs: Vec::new(), stored_len, len, replacement_at };
        let mut out = 0u64;
        let (mut f, mut s) = (first, 0u64);
        let emit = |out: &mut u64, source: Source, n: u64, runs: &mut Vec<Run>| -> u64 {
            let n = n.min(len - *out);
            if n == 0 {
                return 0;
            }
            match runs.last_mut() {
                Some(r) if r.at + r.len == *out && contiguous(r.source, r.len, source) => r.len += n,
                _ => runs.push(Run { at: *out, len: n, source }),
            }
            *out += n;
            n
        };
        // The pages the file stores.
        emit(&mut out, Source::File(f), stored_len, &mut map.runs);
        f += stored_len;

        while out < len {
            let command = a.symbol[bits.peek(6) as usize];
            bits.skip(u32::from(a.bits[bits.peek(6) as usize]))?;
            let n = if command <= 3 {
                let index = bits.peek(8) as usize;
                let exponent = u32::from(b.symbol[index]);
                bits.skip(u32::from(b.bits[index]))?;
                let n = (1u64 << exponent) + bits.peek(exponent);
                bits.skip(exponent)?;
                n
            } else {
                *[3u64, 7, 6]
                    .get(usize::from(command) - 4)
                    .ok_or_else(|| corrupt(format!("rpak patch: unknown command {command}")))?
            };
            match command {
                0 => {
                    let took = emit(&mut out, Source::File(f), n, &mut map.runs);
                    f += took;
                }
                1 => f = f.saturating_add(n),
                2 => {
                    let took = emit(&mut out, Source::Replacement(s), n, &mut map.runs);
                    s += took;
                }
                3 => {
                    let took = emit(&mut out, Source::Replacement(s), n, &mut map.runs);
                    s += took;
                    f += took;
                }
                4..=6 => {
                    let replaced = if command == 6 { 2 } else { 1 };
                    let took = emit(&mut out, Source::Replacement(s), replaced, &mut map.runs);
                    s += took;
                    f += took;
                    let took = emit(&mut out, Source::File(f), n, &mut map.runs);
                    f += took;
                }
                _ => return Err(corrupt(format!("rpak patch: unknown command {command}"))),
            }
        }
        if map.replacement_at as u64 + s > stream.len() as u64 {
            return Err(corrupt("rpak patch: the script reads past the replacement data"));
        }
        Ok(map)
    }

    /// The pieces that make up `.mem` bytes `range` (offsets in page order), in order.
    pub fn locate(&self, range: std::ops::Range<u64>) -> Result<Vec<Piece>> {
        if range.start > range.end || range.end > self.len {
            return Err(corrupt("read past the end of the patched pages"));
        }
        // `.mem` is the pages in index order; `D` has the stored pages first.
        let head = self.len - self.stored_len; // bytes of the pages that are not stored
        let mut pieces = Vec::new();
        let parts = [
            (range.start, range.end.min(head), self.stored_len),
            (range.start.max(head), range.end, 0u64.wrapping_sub(head)),
        ];
        for (start, end, shift) in parts {
            if start < end {
                self.pieces(start.wrapping_add(shift)..end.wrapping_add(shift), &mut pieces)?;
            }
        }
        Ok(pieces)
    }

    fn pieces(&self, range: std::ops::Range<u64>, out: &mut Vec<Piece>) -> Result<()> {
        let mut i = self.runs.partition_point(|r| r.at + r.len <= range.start);
        let mut at = range.start;
        while at < range.end {
            let run = self.runs.get(i).filter(|r| r.at <= at).ok_or_else(|| corrupt("rpak patch: the script leaves a gap"))?;
            let skip = at - run.at;
            let n = (run.len - skip).min(range.end - at);
            let source = match run.source {
                Source::File(f) => Source::File(f + skip),
                Source::Replacement(s) => Source::Replacement(s + skip),
            };
            out.push(Piece { source, len: n });
            at += n;
            i += 1;
        }
        Ok(())
    }
}

/// Whether a run of `len` bytes from `a` is followed directly by `b` in its source.
fn contiguous(a: Source, len: u64, b: Source) -> bool {
    match (a, b) {
        (Source::File(x), Source::File(y)) | (Source::Replacement(x), Source::Replacement(y)) => x + len == y,
        _ => false,
    }
}
