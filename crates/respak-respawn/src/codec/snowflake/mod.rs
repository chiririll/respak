//! Snowflake: the entropy + LZ codec of Respawn's streamed data (codec 2).
//!
//! Ported from rsx `src/game/rtech/utils/utils.cpp` (`InitSnowflakeDecompState`,
//! `DecompressSnowflake` and the helpers `sub_7FF7FC23B960` (bit reader), `C680`, `C880`, `BA70`
//! (model set-up) and `CD20` (literals)), which is itself decompiler output. The codec is an
//! adaptive two-stream rANS coder with 15-bit probabilities and an LZ layer: a literal-length
//! symbol, then the literals (an order-1 nibble model), then a match length and an offset (a
//! recent-offsets list of four plus explicit offsets).
//!
//! The decoder state is a flat 0x25000-byte block addressed by raw offsets, exactly as in the
//! original; the SIMD (SSE2/SSSE3) table compares and updates are done on `[i16; 8]` lanes with
//! the same wrapping and signedness. Offsets inside the block:
//!
//! | offset | what |
//! |---|---|
//! | 0 / 8 | the two rANS states |
//! | 24 | position in the input of the next 32-bit refill |
//! | 40 / 44 | bit reader: buffered bits / how many |
//! | 48.. | probability tables (see the `c880` set-up) |
//! | 1138 / 1139 | previous literal / index into the recent-offsets list |
//! | 1140 | the four recent offsets (u32 each) |
//! | 149128 | commands left in the block |
//! | 149132 | bits of the command count |
//! | 149136 / 149140 | block size, and its second copy |
//! | 149144 | total decoded size |
//! | 149152 | end of the current block in the output |
//! | 149160 | the size of the compressed stream |
//! | 149184 | cursor of the bit reader in the input |
//! | 149208 | position in the output |
//!
//! The stream is `bit-packed header | blocks`. A block is a command count, two rANS start
//! states, the commands, the literals of the tail and the final states stored raw.
//!
//! Files: `state` (the state block and its offsets), `simd` (lane operations, rANS step), `bits`
//! (input reads, refill, bit reader), `model` (set-up of the tables), `literals`, `run` (the
//! command loop). `tests` holds the unit tests and the hashes of real blocks for the ignored one.

use std::fmt;

mod bits;
mod literals;
mod model;
mod run;
mod simd;
mod state;

use state::{Snow, BLOCK_END, BLOCK_LEN, OUTPOS, STATE_LEN, TOTAL};

/// A Snowflake stream that cannot be decoded.
#[derive(Debug)]
pub enum SnowflakeError {
    /// The stream is truncated or inconsistent.
    Corrupt(&'static str),
    /// More output than the stream says it holds, or an unreasonable size.
    TooBig,
}

impl fmt::Display for SnowflakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(what) => write!(f, "corrupt Snowflake stream: {what}"),
            Self::TooBig => f.write_str("Snowflake stream is too big"),
        }
    }
}

impl std::error::Error for SnowflakeError {}

type R<T> = Result<T, SnowflakeError>;

/// Largest output we are willing to produce (the largest streamed texture is far below this).
const MAX_OUTPUT: u64 = 1 << 30;

/// Decodes a Snowflake stream (`input` is the whole stored block, padding included) to the size
/// the stream says it has.
pub fn decompress(input: &[u8]) -> Result<Vec<u8>, SnowflakeError> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut s = Snow {
            m: vec![0u8; STATE_LEN],
            input,
            out: Vec::new(),
        };
        s.init()?;
        let total = s.r64(TOTAL);
        s.out = vec![0u8; total as usize];
        let block = u64::from(s.r32(BLOCK_LEN));
        s.w64(BLOCK_END, total.min(block));
        s.w64(OUTPOS, 0);
        if s.run(input.len() as u64, total)? {
            Ok(s.out)
        } else {
            Err(SnowflakeError::Corrupt("truncated"))
        }
    }));
    attempt.unwrap_or(Err(SnowflakeError::Corrupt("the decoder panicked")))
}

#[cfg(test)]
mod tests;
