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

/// First `0x4E2` bytes of rsx's `s_PakFileCompressionLUT` (the remainder of the table is
/// unrelated data).
#[rustfmt::skip]
static LUT: [u8; 0x4E2] = [
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF9, 0x04, 0xFD, 0xFC, 0x07, 0x04, 0x05, 0xFF, 0xF4,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF6, 0x04, 0xFD, 0xFC, 0xFB, 0x04, 0x06, 0xFF, 0x0B,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF8, 0x04, 0xFD, 0xFC, 0x0C, 0x04, 0x05, 0xFF, 0xF7,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF5, 0x04, 0xFD, 0xFC, 0xFA, 0x04, 0x06, 0xFF, 0xF3,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF9, 0x04, 0xFD, 0xFC, 0x07, 0x04, 0x05, 0xFF, 0xF4,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF6, 0x04, 0xFD, 0xFC, 0xFB, 0x04, 0x06, 0xFF, 0x0E,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF8, 0x04, 0xFD, 0xFC, 0x0C, 0x04, 0x05, 0xFF, 0x09,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF5, 0x04, 0xFD, 0xFC, 0xFA, 0x04, 0x06, 0xFF, 0xF1,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF9, 0x04, 0xFD, 0xFC, 0x07, 0x04, 0x05, 0xFF, 0xF4,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF6, 0x04, 0xFD, 0xFC, 0xFB, 0x04, 0x06, 0xFF, 0x0D,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF8, 0x04, 0xFD, 0xFC, 0x0C, 0x04, 0x05, 0xFF, 0xF7,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF5, 0x04, 0xFD, 0xFC, 0xFA, 0x04, 0x06, 0xFF, 0xF2,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF9, 0x04, 0xFD, 0xFC, 0x07, 0x04, 0x05, 0xFF, 0xF4,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF6, 0x04, 0xFD, 0xFC, 0xFB, 0x04, 0x06, 0xFF, 0x0F,
    0x04, 0xFE, 0xFC, 0x08, 0x04, 0xEF, 0x11, 0xF8, 0x04, 0xFD, 0xFC, 0x0C, 0x04, 0x05, 0xFF, 0x0A,
    0x04, 0xFE, 0xFC, 0x10, 0x04, 0xEF, 0x11, 0xF5, 0x04, 0xFD, 0xFC, 0xFA, 0x04, 0x06, 0xFF, 0xF0,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x11,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0C,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x09,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0E,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x11,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0B,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0A,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x10,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x11,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0C,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x09,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0F,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x11,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0D,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x07, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x0A,
    0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0x08, 0x04, 0x05, 0x04, 0x06, 0x04, 0x05, 0x04, 0xFF,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x06,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x07,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x06,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x06,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x07,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x06,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x02, 0x04, 0x03, 0x05, 0x02, 0x04, 0x04, 0x06, 0x02, 0x04, 0x03, 0x06, 0x02, 0x05, 0x04, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x07,
    0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x03, 0x01, 0x02, 0x01, 0x08,
    0x00, 0x08, 0x00, 0x04, 0x00, 0x08, 0x00, 0x06, 0x00, 0x08, 0x00, 0x01, 0x00, 0x08, 0x00, 0x0B,
    0x00, 0x08, 0x00, 0x0C, 0x00, 0x08, 0x00, 0x09, 0x00, 0x08, 0x00, 0x03, 0x00, 0x08, 0x00, 0x0E,
    0x00, 0x08, 0x00, 0x04, 0x00, 0x08, 0x00, 0x07, 0x00, 0x08, 0x00, 0x02, 0x00, 0x08, 0x00, 0x0D,
    0x00, 0x08, 0x00, 0x0C, 0x00, 0x08, 0x00, 0x0A, 0x00, 0x08, 0x00, 0x05, 0x00, 0x08, 0x00, 0x0F,
    0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06,
    0x01, 0x02, 0x01, 0x05, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06, 0x01, 0x02, 0x01, 0x06,
    0x4A, 0x00, 0x00, 0x00, 0x6A, 0x00, 0x00, 0x00, 0x8A, 0x00, 0x00, 0x00, 0xAA, 0x00, 0x00, 0x00,
    0xCA, 0x00, 0x00, 0x00, 0xEA, 0x00, 0x00, 0x00, 0x0A, 0x01, 0x00, 0x00, 0x2A, 0x01, 0x00, 0x00,
    0x4A, 0x01, 0x00, 0x00, 0x6A, 0x01, 0x00, 0x00, 0x8A, 0x01, 0x00, 0x00, 0xAA, 0x01, 0x00, 0x00,
    0xAA, 0x03, 0x00, 0x00, 0xAA, 0x05, 0x00, 0x00, 0xAA, 0x25, 0x00, 0x00, 0xAA, 0x25, 0x02, 0x00,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x09, 0x09, 0x0D, 0x11, 0x15,
    0x00, 0x00, 0x02, 0x04, 0x06, 0x08, 0x0A, 0x2A, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x05, 0x05,
    0x11, 0x01,
];

const LEN_OFF: usize = 512;
const DIST_SYM_OFF: usize = 1024;
const DIST_LEN_OFF: usize = 1088;
const EXT_BASE16_OFF: usize = 1152; // 16 x u32 LE
const EXT_BITS16_OFF: usize = 1216;
const EXT_BASE8_OFF: usize = 1232;
const EXT_BITS8_OFF: usize = 1240;
const ESCAPE_OFF: usize = 1248;

/// Largest output we are willing to allocate when the caller gave no expected length.
const MAX_OUTPUT: u64 = 1 << 32;

#[inline]
fn shr(x: u64, n: u64) -> u64 {
    if n >= 64 { 0 } else { x >> n }
}

#[inline]
fn shl(x: u64, n: u64) -> u64 {
    if n >= 64 { 0 } else { x << n }
}

#[inline]
fn low_mask(bits: u64) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// Reads the extended-length / base table entry for a 3-bit selector (non-zero).
#[inline]
fn ext16_base(i: usize) -> u64 {
    let o = EXT_BASE16_OFF + i * 4;
    u64::from(u32::from_le_bytes([LUT[o], LUT[o + 1], LUT[o + 2], LUT[o + 3]]))
}

/// Copies `n` bytes within `out` from `dst - dist` to `dst` with LZ77 overlap semantics.
/// The caller guarantees `dist <= dst` and `dst + n <= out.len()`.
#[inline]
fn lz_copy(out: &mut [u8], dst: usize, dist: usize, n: usize) {
    if dist == 0 || n == 0 {
        return;
    }
    let src = dst - dist;
    if dist >= n {
        out.copy_within(src..src + n, dst);
    } else if dist == 1 {
        let b = out[src];
        out[dst..dst + n].fill(b);
    } else if dist < 16 {
        for i in 0..n {
            out[dst + i] = out[src + i];
        }
    } else {
        let mut done = 0;
        while done < n {
            let k = dist.min(n - done);
            out.copy_within(src + done..src + done + k, dst + done);
            done += k;
        }
    }
}

struct Decoder<'a> {
    data: &'a [u8],
    out: Vec<u8>,
    /// Total decompressed size including the header region.
    decomp_size: u64,
    inv_in: u64,
    inv_out: u64,
    hdr_off: u64,
    q70: u64,
    buffer_needed: u64,
    comp_stream: u64,
    decomp_stream: u64,
    rd: u64,
    bits: u64,
    nb: u64,
    pos: u64,
    /// The input is a prefix of the file: running out of it asks for more instead of padding with zeros.
    partial: bool,
    /// Stop at the first output chunk boundary at or after this absolute position.
    stop_at: u64,
}

impl Decoder<'_> {
    /// Loads 8 bytes at `pos`, zero padded past the end (the original over-reads there).
    fn load64(&self, pos: u64) -> Result<u64, RtechError> {
        let len = self.data.len() as u64;
        if self.partial && pos.saturating_add(8) > len {
            return Err(RtechError::NeedInput { at_least: pos.saturating_add(8) });
        }
        if pos > len.saturating_add(16) {
            return Err(RtechError::Truncated("read past end of input"));
        }
        let p = pos as usize;
        if let Some(s) = self.data.get(p..p + 8) {
            return Ok(u64::from_le_bytes(s.try_into().unwrap_or([0; 8])));
        }
        let mut buf = [0u8; 8];
        if let Some(s) = self.data.get(p..) {
            buf[..s.len()].copy_from_slice(s);
        }
        Ok(u64::from_le_bytes(buf))
    }

    /// Whether the input cannot hold the current compressed chunk (plus the over-read slack after it,
    /// unless the input is the whole file).
    fn input_short(&self) -> bool {
        let slack = if self.partial { 16 } else { 0 };
        (self.data.len() as u64) < self.buffer_needed.saturating_add(slack)
    }

    fn short_error(&self) -> RtechError {
        if self.partial {
            RtechError::NeedInput { at_least: self.buffer_needed.saturating_add(16) }
        } else {
            RtechError::Truncated("compressed chunk")
        }
    }

    /// Reads the extended length common to long literal runs and long matches (selector,
    /// base and raw bits) and consumes the used bits from the bit buffer.
    fn ext_length(&mut self, long_match: bool) -> Result<u64, RtechError> {
        let sel = (self.bits & 7) as usize;
        let mut rest = self.bits >> 3;
        let (base, nbits);
        if sel != 0 {
            base = u64::from(LUT[EXT_BASE8_OFF + sel]);
            nbits = u64::from(LUT[EXT_BITS8_OFF + sel]);
        } else {
            let t = (rest & 0xF) as usize;
            rest >>= 4;
            self.nb += 4;
            base = ext16_base(t);
            nbits = u64::from(LUT[EXT_BITS16_OFF + t]);
            if long_match && self.nb + nbits >= 61 {
                // The original tops the bit buffer up with one byte here.
                let byte = match self.data.get(self.rd as usize) {
                    Some(b) => u64::from(*b),
                    None if self.partial => return Err(RtechError::NeedInput { at_least: self.rd + 1 }),
                    None if self.rd <= self.data.len() as u64 + 16 => 0,
                    None => return Err(RtechError::Truncated("read past end of input")),
                };
                self.rd += 1;
                if self.nb > 61 {
                    return Err(RtechError::Corrupt("bit buffer overrun"));
                }
                rest |= byte << (61 - self.nb);
                self.nb -= 8;
            }
        }
        self.nb += nbits + 3;
        self.bits = shr(rest, nbits);
        Ok(base + (rest & low_mask(nbits)))
    }

    fn run(&mut self) -> Result<(), RtechError> {
        let size = self.decomp_size;
        let mut state = 0usize;
        let mut limit = self.q70.min(self.comp_stream);
        let mut refill = self.nb != 0;
        if self.data.len() as u64 > u64::MAX / 2 {
            return Err(RtechError::Corrupt("input too large"));
        }
        loop {
            if refill {
                if self.nb > 64 {
                    return Err(RtechError::Corrupt("bit buffer overrun"));
                }
                let w = self.load64(self.rd)?;
                let v = shl(w, 64 - self.nb) | self.bits;
                self.rd += self.nb >> 3;
                self.nb &= 7;
                self.bits = shr(u64::MAX, self.nb) & v;
            }
            refill = true;

            let idx = (self.bits & 0xFF) as usize + state * 256;
            let l = u64::from(LUT[LEN_OFF + idx]);
            self.nb += l;
            self.bits >>= l;
            let op = LUT[idx] as i8;

            if op < 0 {
                let mut cnt = u64::from(op.unsigned_abs());
                let prev = state;
                state = 1;
                if cnt == u64::from(LUT[ESCAPE_OFF + prev]) {
                    if (!self.rd & self.inv_in) < 0xF
                        || (self.inv_out & !self.pos) < 15
                        || size.saturating_sub(self.pos) < 0x10
                    {
                        cnt = 1;
                    }
                    cnt += self.ext_length(false)?;
                }
                let s = self.rd as usize;
                let d = self.pos as usize;
                let n = cnt as usize;
                let src = self
                    .data
                    .get(s..s.checked_add(n).ok_or(RtechError::Corrupt("literal run"))?)
                    .ok_or(RtechError::Truncated("literal run past end of input"))?;
                let dst = self
                    .out
                    .get_mut(d..d.checked_add(n).ok_or(RtechError::Corrupt("literal run"))?)
                    .ok_or(RtechError::Corrupt("literal run past end of output"))?;
                dst.copy_from_slice(src);
                self.rd += cnt;
                self.pos += cnt;
            } else {
                state = 0;
                let v21 = self.bits & 0xF;
                let (t, u) = if v21 == 15 { (6, (self.bits >> 4) & 3) } else { (4, 0) };
                let e = v21 + u;
                let v22 = ((self.bits >> t) & 0x3F) as usize;
                let dlen = u64::from(LUT[DIST_LEN_OFF + v22]);
                let sym = u64::from(LUT[DIST_SYM_OFF + v22]);
                let mant = shr(self.bits, t + dlen) & low_mask(e);
                let dist = 16 * ((1u64 << e) + mant) + sym - 16;
                self.nb += t + dlen + e;
                self.bits = shr(self.bits, t + dlen + e);

                let len = if op == 17 {
                    let ext = self.ext_length(true)?;
                    if dist < 8 { ext + 4 } else { ext + 17 }
                } else {
                    u64::from(op as u8)
                };
                if dist > self.pos {
                    return Err(RtechError::Corrupt("match distance before start of output"));
                }
                if self.pos + len > size {
                    return Err(RtechError::Corrupt("match past end of output"));
                }
                lz_copy(&mut self.out, self.pos as usize, dist as usize, len as usize);
                self.pos += len;
            }

            if self.pos == size {
                return Ok(());
            }
            if self.pos >= self.stop_at && self.pos == self.decomp_stream {
                return Ok(());
            }
            if self.pos > self.decomp_stream {
                return Err(RtechError::Corrupt("op crosses output chunk boundary"));
            }

            if self.rd >= limit {
                if self.pos == self.decomp_stream {
                    // Start of a new chunk: one pad bit, then the chunk size field.
                    self.bits >>= 1;
                    self.nb += 1;
                    let to_boundary = self.inv_in & 0u64.wrapping_sub(self.rd);
                    if self.hdr_off > to_boundary {
                        self.rd += to_boundary;
                        if self.rd > self.q70 {
                            self.q70 = self.q70.wrapping_add(self.inv_in).wrapping_add(1);
                        }
                    }
                    let at = self.rd;
                    self.rd += self.hdr_off;
                    let sz = self.load64(at)? & low_mask(8 * self.hdr_off);
                    self.buffer_needed = self.buffer_needed.wrapping_add(sz);
                    self.comp_stream = self.comp_stream.wrapping_add(sz);
                    let mut next = self.pos.wrapping_add(self.inv_out).wrapping_add(1);
                    if next >= size {
                        next = size;
                        self.comp_stream = self.comp_stream.wrapping_add(self.hdr_off);
                    }
                    self.decomp_stream = next;
                    if self.input_short() {
                        return Err(self.short_error());
                    }
                }
                limit = self.q70;
                if self.rd >= limit {
                    self.rd = !self.inv_in & self.rd.wrapping_add(7);
                    limit = limit.wrapping_add(self.inv_in).wrapping_add(1);
                    self.q70 = limit;
                }
                limit = limit.min(self.comp_stream);
            }
        }
    }
}

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
pub fn decompress(file: &[u8], header_len: usize, expected_len: Option<u64>) -> Result<Vec<u8>, RtechError> {
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
        RtechError::Truncated(_) if !input_complete => RtechError::NeedInput { at_least: file.len() as u64 + 1 },
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
    // --- InitPakDecoder ---
    let hdr_end = header_len.checked_add(16).ok_or(RtechError::Truncated("stream header"))?;
    let hdr = file.get(header_len..hdr_end.min(file.len())).ok_or(RtechError::Truncated("stream header"))?;
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
    let in_bits = if w == 0 { 64 } else { w };
    let out_bits = if o == 0 { 64 } else { o };
    let inv_in = low_mask(in_bits);
    let inv_out = low_mask(out_bits);

    let hl = header_len as u64;
    if size < hl {
        return Err(RtechError::Corrupt("decompressed size smaller than header"));
    }
    let body_len = size - hl;
    if let Some(exp) = expected_len {
        if exp != body_len {
            return Err(RtechError::SizeMismatch { expected: exp, actual: body_len });
        }
    } else if size > MAX_OUTPUT {
        return Err(RtechError::TooLarge(size));
    }

    let byte_pos = header_len + (sh / 8) as usize; // byte holding the first payload bit
    let rem = u64::from(sh % 8);
    let mut cur = [0u8; 8];
    if let Some(s) = file.get(byte_pos..) {
        let k = s.len().min(8);
        cur[..k].copy_from_slice(&s[..k]);
    } else {
        return Err(RtechError::Truncated("stream header"));
    }
    let bits = u64::from_le_bytes(cur) >> rem;
    let mut rd = byte_pos as u64 + 8;

    let (hdr_off, first) = if inv_in == u64::MAX {
        (0u64, file.len() as u64)
    } else {
        let ho = (in_bits >> 3) + 1;
        let mut f = [0u8; 8];
        let s = file.get(rd as usize..).unwrap_or(&[]);
        let k = s.len().min(8);
        f[..k].copy_from_slice(&s[..k]);
        rd += ho;
        (ho, u64::from_le_bytes(f) & low_mask(8 * ho))
    };

    let mut dec = Decoder {
        data: file,
        out: Vec::new(),
        decomp_size: size,
        inv_in,
        inv_out,
        hdr_off,
        q70: inv_in.wrapping_sub(6),
        buffer_needed: first,
        comp_stream: first,
        decomp_stream: size,
        rd,
        bits,
        nb: rem,
        pos: hl,
        partial,
        stop_at: hl.saturating_add(want),
    };
    if size - 1 > inv_out {
        dec.comp_stream = first.wrapping_sub(hdr_off);
        dec.decomp_stream = inv_out + 1;
    }
    if dec.input_short() {
        return Err(dec.short_error());
    }
    // Only as much output as the wanted prefix needs, up to the end of the chunk it ends in.
    let chunk = inv_out.wrapping_add(1);
    let cuttable = inv_in != u64::MAX && chunk != 0 && size - 1 > inv_out;
    let alloc = if want >= body_len || !cuttable { size } else { dec.stop_at.div_ceil(chunk).saturating_mul(chunk).min(size) };
    let total = usize::try_from(alloc).map_err(|_| RtechError::TooLarge(size))?;
    dec.out.try_reserve_exact(total).map_err(|_| RtechError::TooLarge(size))?;
    dec.out.resize(total, 0);

    dec.run()?;

    let complete = dec.pos == size;
    let mut out = dec.out;
    out.truncate(dec.pos as usize);
    out.drain(..header_len);
    Ok(Prefix { body: out, complete })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UI_RPAK: &str =
        r"C:\Users\chiririll\Projects\rsx\rsx_steam_cache\rpak\1172471_6197768190074397522\paks\win64\ui.rpak";

    /// Hand-made stream: size 4, one literal run of 4 bytes.
    fn tiny() -> Vec<u8> {
        let mut v = vec![0x02, 0x00, 0x40, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(b"ABCD");
        v
    }

    #[test]
    fn tiny_stream_roundtrip() {
        let out = decompress(&tiny(), 0, Some(4)).unwrap();
        assert_eq!(out, b"ABCD");
        assert!(matches!(decompress(&tiny(), 0, Some(5)), Err(RtechError::SizeMismatch { .. })));
    }

    #[test]
    fn truncated_prefixes_do_not_panic() {
        let t = tiny();
        for len in 0..=t.len() {
            let _ = decompress(&t[..len], 0, None);
            let _ = decompress(&t[..len], 8, None);
        }
        assert!(decompress(&t[..12], 0, Some(4)).is_err());
    }

    #[test]
    fn garbage_does_not_panic() {
        let mut s: u64 = 0x1234_5678_9ABC_DEF0;
        for round in 0..3000 {
            let len = (round % 300) + 1;
            let mut buf = Vec::with_capacity(len);
            for _ in 0..len {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                buf.push((s >> 24) as u8);
            }
            for hl in [0usize, 0x58, 0x80] {
                // Keep the announced size small so the test stays fast.
                if let Some(b) = buf.get_mut(hl) {
                    *b = (*b & 0xC0) | (round as u8 % 14);
                }
                let _ = decompress(&buf, hl, None);
            }
        }
    }

    #[test]
    #[ignore = "needs local game files"]
    fn ui_rpak() {
        let Ok(file) = std::fs::read(UI_RPAK) else {
            eprintln!("ui.rpak not found, skipping");
            return;
        };
        let le32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let le64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let dcmp = le64(&file, 0x30);
        let t = std::time::Instant::now();
        let out = decompress(&file, 0x80, Some(dcmp - 0x80)).unwrap();
        eprintln!("decompressed {} bytes in {:?}", out.len(), t.elapsed());
        assert_eq!(out.len() as u64, dcmp - 0x80);

        // v8 table region (header fields: numSegments / numPages at 0x3C).
        let num_segments = u16::from_le_bytes([file[0x4C], file[0x4D]]) as usize;
        let num_pages = u16::from_le_bytes([file[0x4E], file[0x4F]]) as usize;
        assert_eq!((num_segments, num_pages), (6, 63));

        // Segment headers: { flags: u32, align: u32, size: u64 }.
        let mut seg_sizes = Vec::new();
        for s in 0..num_segments {
            let o = s * 16;
            let (align, size) = (le32(&out, o + 4), le64(&out, o + 8));
            assert!(align.is_power_of_two() && align <= 0x1000, "segment {s} align {align}");
            assert!(size > 0 && size < dcmp, "segment {s} size {size}");
            seg_sizes.push(size);
        }
        // Page headers: { segment: u32, align: u32, size: u32 }.
        let mut per_seg = vec![0u64; num_segments];
        let mut pages_total = 0u64;
        for p in 0..num_pages {
            let o = num_segments * 16 + p * 12;
            let (seg, align, size) = (le32(&out, o) as usize, le32(&out, o + 4), le32(&out, o + 8));
            assert!(seg < num_segments && align.is_power_of_two(), "page {p}");
            per_seg[seg] += u64::from(size);
            pages_total += u64::from(size);
        }
        eprintln!("segment sizes {seg_sizes:x?}, page sums {per_seg:x?}");
        for (s, (&seg, &pg)) in seg_sizes.iter().zip(&per_seg).enumerate() {
            // Segment size is the page sizes plus alignment padding.
            assert!(seg >= pg && seg - pg < pg / 8 + 4096 * 64, "segment {s}: {seg:#x} vs pages {pg:#x}");
        }
        let tables = (num_segments * 16 + num_pages * 12) as u64;
        eprintln!("tables {tables:#x}, pages {pages_total:#x}, out {:#x}", out.len());
        assert!(tables + pages_total <= out.len() as u64);
        assert!(out.len() as u64 - tables - pages_total < out.len() as u64 / 4);
    }

    #[test]
    fn a_partial_input_asks_for_more_instead_of_guessing() {
        let t = tiny();
        for len in 0..t.len() {
            match decompress_prefix(&t[..len], false, 0, Some(4), u64::MAX) {
                Err(RtechError::NeedInput { at_least }) => assert!(at_least > len as u64, "{len}: {at_least}"),
                other => panic!("{len} bytes of input: {other:?}"),
            }
        }
        let whole = decompress_prefix(&t, true, 0, Some(4), u64::MAX).unwrap();
        assert_eq!((whole.body.as_slice(), whole.complete), (&b"ABCD"[..], true));
    }

    /// Prefixes of a real v8 pak: equal to the start of the whole body, and a small prefix needs only
    /// a small part of the file.
    #[test]
    #[ignore = "needs local game files"]
    fn ui_rpak_prefixes() {
        let Ok(file) = std::fs::read(UI_RPAK) else {
            eprintln!("ui.rpak not found, skipping");
            return;
        };
        let dcmp = u64::from_le_bytes(file[0x30..0x38].try_into().unwrap());
        let len = dcmp - 0x80;
        let whole = decompress(&file, 0x80, Some(len)).unwrap();
        for want in [1u64, 4096, 1 << 20, 10 << 20, len] {
            let mut input = 1usize << 20;
            let prefix = loop {
                let part = &file[..input.min(file.len())];
                match decompress_prefix(part, part.len() == file.len(), 0x80, Some(len), want) {
                    Ok(p) => break p,
                    Err(RtechError::NeedInput { at_least }) => input = (at_least as usize).max(input * 2),
                    Err(e) => panic!("want {want}: {e}"),
                }
            };
            assert!(prefix.body.len() as u64 >= want.min(len), "want {want}, got {}", prefix.body.len());
            assert_eq!(prefix.body, whole[..prefix.body.len()], "want {want}");
            assert_eq!(prefix.complete, prefix.body.len() as u64 == len);
            eprintln!("want {want}: {} bytes decoded from {} of {} input bytes", prefix.body.len(), input.min(file.len()), file.len());
            if want <= 1 << 20 {
                assert!(input < file.len() / 4, "a small prefix should not need most of the file");
            }
        }
    }
}
