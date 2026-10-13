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

use std::fmt;

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

const STATE_LEN: usize = 0x25000;
const X0: usize = 0;
const X1: usize = 8;
const INPOS: usize = 24;
const BITBUF: usize = 40;
const BITCNT: usize = 44;
const PREV: usize = 1138;
const HIST_IDX: usize = 1139;
const HIST: usize = 1140;
const COUNT: usize = 149128;
const COUNT_BITS: usize = 149132;
const BLOCK_LEN: usize = 149136;
const TOTAL: usize = 149144;
const BLOCK_END: usize = 149152;
const MIN_LEN: usize = 149160;
const CURSOR: usize = 149184;
const OUTPOS: usize = 149208;

const LUT_SNOWFLAKE_0: [u8; 16] = [0, 3, 7, 15, 35, 63, 82, 71, 66, 69, 0, 0, 0, 0, 0, 0];
const LUT_SNOWFLAKE_1: [u64; 4] = [4611756117654110208, 4611756116592754688, 4611686559597395968, 53199311768322048];

/// Eight 16-bit lanes (an `__m128i`).
type V = [i16; 8];

/// `_mm_set_epi32(e3, e2, e1, e0)`.
fn set32(e3: u32, e2: u32, e1: u32, e0: u32) -> V {
    let w = |x: u32, i: u32| ((x >> (16 * i)) & 0xFFFF) as u16 as i16;
    [w(e0, 0), w(e0, 1), w(e1, 0), w(e1, 1), w(e2, 0), w(e2, 1), w(e3, 0), w(e3, 1)]
}

/// The low 16 bits of `x` in every lane (shuffle / unpack broadcast).
fn splat(x: u64) -> V {
    [x as u16 as i16; 8]
}

fn cmpgt(a: V, b: V) -> V {
    std::array::from_fn(|i| if a[i] > b[i] { -1 } else { 0 })
}

fn and(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] & b[i])
}

fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].wrapping_sub(b[i]))
}

fn srai(a: V, n: u32) -> V {
    std::array::from_fn(|i| a[i] >> n)
}

/// How many lanes of a compare result are set (`popcnt(movemask)` of the packed compare, or of
/// one compare divided by two).
fn lanes(a: V) -> u32 {
    a.iter().filter(|&&x| x != 0).count() as u32
}

/// `old + (((target) - old) >> shift)` per lane, where `target = (mask & cmp) + k`.
fn adapt(old: V, cmp: V, mask: V, k: V, shift: u32) -> V {
    add(srai(sub(add(and(cmp, mask), k), old), shift), old)
}

/// rANS step: `(x & mask) + (x >> bits) * (c1 - c0) - c0`.
fn rans(x: u64, bits: u32, c0: u16, c1: u16) -> u64 {
    let freq = u64::from(u32::from(c1).wrapping_sub(u32::from(c0)));
    (x & ((1u64 << bits) - 1)).wrapping_add((x >> bits).wrapping_mul(freq)).wrapping_sub(u64::from(c0))
}

fn highest_bit(x: u64) -> Option<u32> {
    (x != 0).then(|| 63 - x.leading_zeros())
}

struct Snow<'a> {
    m: Vec<u8>,
    input: &'a [u8],
    out: Vec<u8>,
}

impl<'a> Snow<'a> {
    fn r8(&self, o: usize) -> u8 {
        self.m[o]
    }
    fn r16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.m[o], self.m[o + 1]])
    }
    fn r32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.m[o..o + 4].try_into().expect("4 bytes"))
    }
    fn r64(&self, o: usize) -> u64 {
        u64::from_le_bytes(self.m[o..o + 8].try_into().expect("8 bytes"))
    }
    fn w8(&mut self, o: usize, v: u8) {
        self.m[o] = v;
    }
    fn w16(&mut self, o: usize, v: u16) {
        self.m[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn w32(&mut self, o: usize, v: u32) {
        self.m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn w64(&mut self, o: usize, v: u64) {
        self.m[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn ldv(&self, o: usize) -> V {
        std::array::from_fn(|i| self.r16(o + 2 * i) as i16)
    }
    fn stv(&mut self, o: usize, v: V) {
        for (i, x) in v.iter().enumerate() {
            self.w16(o + 2 * i, *x as u16);
        }
    }
    /// A table entry (u16) at byte offset `o`.
    fn t(&self, o: usize) -> u16 {
        self.r16(o)
    }

    /// The 32 bits of the input at `pos` (zero past the end).
    fn in32(&self, pos: u64) -> u64 {
        let p = pos as usize;
        let mut b = [0u8; 4];
        if pos < self.input.len() as u64 {
            let n = (self.input.len() - p).min(4);
            b[..n].copy_from_slice(&self.input[p..p + n]);
        }
        u64::from(u32::from_le_bytes(b))
    }

    /// The 64 bits of the input at `pos` (zero past the end).
    fn in64(&self, pos: u64) -> u64 {
        self.in32(pos) | (self.in32(pos + 4) << 32)
    }

    /// Refills a rANS state that dropped below 2^32 from the input.
    fn renorm(&mut self, v: u64) -> u64 {
        if v < 0x1_0000_0000 {
            let at = self.r64(INPOS);
            let w = self.in32(at);
            self.w64(INPOS, at + 4);
            (v << 32) | w
        } else {
            v
        }
    }

    /// `sub_7FF7FC23B960`: reads `n` (up to 63) bits, least significant first.
    fn bits(&mut self, n: u32) -> u64 {
        let have = self.r32(BITCNT);
        let mask = (1u64 << n).wrapping_sub(1);
        if have >= n {
            let buf = self.r32(BITBUF);
            self.w32(BITCNT, have - n);
            self.w32(BITBUF, buf.checked_shr(n).unwrap_or(0));
            return u64::from(buf) & mask;
        }
        let cur = self.r64(CURSOR);
        let mut v = u64::from(self.r32(BITBUF)) | (self.in32(cur) << have);
        self.w64(CURSOR, cur + 4);
        let (left, rest) = if have + 32 >= n {
            (have + 32 - n, (v >> n) as u32)
        } else {
            let w = self.in32(cur + 4);
            self.w64(CURSOR, cur + 8);
            v |= w << (have + 32);
            (have + 32 - n + 32, (w >> (n - (have + 32))) as u32)
        };
        self.w32(BITCNT, left);
        self.w32(BITBUF, rest);
        v & mask
    }

    /// `sub_7FF7FC23C680`: a 256-entry table of symbols coded with a 3-bit length prefix code,
    /// read from the input at byte `pos` after `shift` bits. Returns the bits used and the table.
    fn c680(&self, pos: u64, shift: u8, a4: u32) -> R<(i64, [u8; 256])> {
        let mut v29 = [0u8; 528];
        let g = |v: &[u8; 528], i: usize| u16::from_le_bytes([v[2 * i], v[2 * i + 1]]);
        let s = |v: &mut [u8; 528], i: usize, x: u16| v[2 * i..2 * i + 2].copy_from_slice(&x.to_le_bytes());
        // v29[0] = -65536, v29[1] = -1
        for i in 1..8 {
            s(&mut v29, i, 0xFFFF);
        }
        let mut v10 = self.in64(pos) >> shift;
        let mut v12 = 0u32;
        loop {
            let v14 = (v10 & 7) as usize;
            v10 >>= 3;
            if v14 != 0 {
                let mut v15 = g(&v29, v14);
                s(&mut v29, v14, 0xFFFF);
                if v15 == 0xFFFF {
                    let mut i = v14 - 1;
                    while g(&v29, i) == 0xFFFF {
                        i = i.checked_sub(1).ok_or(SnowflakeError::Corrupt("code table"))?;
                    }
                    v15 = g(&v29, i);
                    s(&mut v29, i, 0xFFFF);
                    let mut v17 = 1u16 << i;
                    loop {
                        i += 1;
                        let v18 = v15.wrapping_add(v17);
                        v17 = v17.wrapping_mul(2);
                        s(&mut v29, i, v18);
                        if i == v14 {
                            break;
                        }
                    }
                }
                loop {
                    let v19 = usize::from(v15);
                    v15 = v15.wrapping_add(1 << v14);
                    if v19 >= 128 {
                        return Err(SnowflakeError::Corrupt("code table"));
                    }
                    s(&mut v29, 8 + 2 * v19, v12 as u16);
                    s(&mut v29, 8 + 2 * v19 + 1, v14 as u16);
                    if v15 >= 0x80 {
                        break;
                    }
                }
            }
            v12 += 1;
            if v12 > a4 {
                break;
            }
        }
        let v20 = (a4 + 1) as u8 as i32;
        let mut v21 = pos + ((u64::from(a4) + 1 + u64::from(shift) + 2 * (u64::from(a4) + 1)) >> 3);
        let mut v22 = ((i32::from(shift) + 2 * v20 + v20) & 7) as u32;
        let mut out = [0u8; 256];
        if g(&v29, 0) != 0 {
            let mut v23 = self.in64(v21) >> v22;
            for (v11, slot) in out.iter_mut().enumerate() {
                let v24 = (v23 & 0x7F) as usize;
                let v25 = u32::from(g(&v29, 8 + 2 * v24 + 1));
                v22 += v25;
                *slot = v29[16 + 4 * v24];
                v23 = v23.checked_shr(v25).unwrap_or(0);
                if v11 & 7 == 7 {
                    let v26 = v22;
                    v22 &= 7;
                    v21 += u64::from(v26 >> 3);
                    v23 = self.in64(v21) >> v22;
                }
            }
        } else {
            out = [8u8; 256];
        }
        Ok((i64::from(v22) + 8 * (v21 - pos) as i64 - i64::from(shift), out))
    }

    /// `sub_7FF7FC23BA70`: the literal models for a stored length table, based at `b`.
    fn ba70(&mut self, b: usize, lens: &[u8; 256]) -> R<()> {
        let mask = u32::from(self.r8(b + 1108));
        let v44 = mask + 1;
        let mut v49 = 0u16;
        for v4 in 0..16u32 {
            let mut v26 = [0u16; 17];
            for (j, slot) in v26.iter_mut().take(16).enumerate() {
                let sym = u32::from(lens[(16 * v4) as usize + j]);
                if sym > 15 {
                    return Err(SnowflakeError::Corrupt("length table"));
                }
                *slot = (1u32 << (15 - sym)) as u16;
            }
            let v10 = v26[..16].iter().fold(0u16, |a, &x| a.wrapping_add(x));
            if v10 == 0 {
                return Err(SnowflakeError::Corrupt("length table"));
            }
            let v11 = u32::from(v10);
            let v12 = u32::from(v10 >> 1);
            let mut v25 = 0i32;
            for (v8, &weight) in v26.iter().enumerate() {
                let v14 = v25 << 15;
                v25 = v25.wrapping_add(i32::from(weight));
                let v15 = ((v12 as i32).wrapping_add(v14) as u32 / v11) as u16;
                for v16 in 0..v44 {
                    for k in 0..16u32 {
                        let idx = 17 * (((16 * (v4 | (16 * (v16 & mask)))) | k) as usize) + v8;
                        self.w16(b + 2 * idx + 9816, v15);
                    }
                }
            }
            for v20 in 0..v44 {
                for k in 0..16u32 {
                    let idx = v4 as usize + 17 * (((16 * (v20 & mask)) | k) as usize);
                    self.w16(b + 2 * idx + 1112, v49);
                }
            }
            v49 = v49.wrapping_add(v10);
        }
        for v13 in 0..v44 {
            for k in 0..16u32 {
                let idx = (16 * (v13 & mask)) | k;
                self.w16(b + 1144 + 34 * idx as usize, 0x8000);
            }
        }
        Ok(())
    }

    /// `sub_7FF7FC23C880`: the initial models, based at `b` (state + 48).
    fn c880(&mut self, b: usize, sel: u8, lens: Option<&[u8; 256]>) -> R<()> {
        self.w8(b + 1108, LUT_SNOWFLAKE_0[sel as usize]);
        self.w16(b + 1088, 2048u16);
        self.w32(b, 126353408u32);
        self.w32(b + 4, 378998543u32);
        self.w32(b + 8, 631643678u32);
        self.w32(b + 12, 884288813u32);
        self.w32(b + 16, 1136933948u32);
        self.w32(b + 20, 1389579083u32);
        self.w32(b + 24, 1642224218u32);
        self.w32(b + 28, 1894869353u32);
        self.w32(b + 32, (-2147452808i64) as u32);
        self.w32(b + 36, 119275520u32);
        self.w32(b + 40, 357895737u32);
        self.w32(b + 44, 596515954u32);
        self.w32(b + 48, 835136171u32);
        self.w32(b + 52, 1073756388u32);
        self.w32(b + 56, 0x10000000u32);
        self.w32(b + 60, 805314560u32);
        self.w32(b + 64, 1342193664u32);
        self.w32(b + 68, 1879072768u32);
        self.w16(b + 72, 0x8000u16);
        self.w32(b + 74, 126353408u32);
        self.w32(b + 78, 378998543u32);
        self.w32(b + 82, 631643678u32);
        self.w32(b + 86, 884288813u32);
        self.w32(b + 90, 1136933948u32);
        self.w32(b + 94, 1389579083u32);
        self.w32(b + 98, 1642224218u32);
        self.w32(b + 102, 1894869353u32);
        self.w32(b + 106, (-2147452808i64) as u32);
        self.w32(b + 110, 63176704u32);
        self.w32(b + 114, 189466504u32);
        self.w32(b + 118, 315821839u32);
        self.w32(b + 122, 442111639u32);
        self.w32(b + 126, 568466974u32);
        self.w32(b + 130, 694756774u32);
        self.w32(b + 134, 821112109u32);
        self.w32(b + 138, 947401909u32);
        self.w32(b + 142, 1073757244u32);
        self.w32(b + 146, 126353408u32);
        self.w32(b + 150, 378998543u32);
        self.w32(b + 154, 631643678u32);
        self.w32(b + 158, 884288813u32);
        self.w32(b + 162, 1136933948u32);
        self.w32(b + 166, 1389579083u32);
        self.w32(b + 170, 1642224218u32);
        self.w32(b + 174, 1894869353u32);
        self.w32(b + 178, (-2147452808i64) as u32);
        self.w32(b + 182, 0x4000000u32);
        self.w32(b + 186, 201328640u32);
        self.w32(b + 190, 335548416u32);
        self.w32(b + 194, 469768192u32);
        self.w32(b + 198, 603987968u32);
        self.w32(b + 202, 738207744u32);
        self.w32(b + 206, 872427520u32);
        self.w32(b + 210, 1006647296u32);
        self.w16(b + 214, 0x4000u16);
        let mut v6 = b + 216;
        let mut v7 = v6 + 130;
        for _ in 0..4 {
            self.w32(v7 - 128, 67109376u32);
            self.w16(v6, 0);
            self.w32(v7 - 124, 134219264u32);
            self.w32(v7 - 120, 201329152u32);
            self.w32(v7 - 116, 268439040u32);
            self.w32(v7 - 112, 0x10000000u32);
            self.w32(v7 - 108, 805314560u32);
            self.w32(v7 - 104, 0x4000u32);
            self.w32(v7 - 100, 536875008u32);
            self.w32(v7 - 96, 1073754112u32);
            self.w32(v7 - 92, 0x10000000u32);
            self.w32(v7 - 88, 805314560u32);
            self.w32(v7 - 84, 0x4000u32);
            self.w32(v7 - 80, 536875008u32);
            self.w32(v7 - 76, 1073754112u32);
            self.w32(v7 - 72, 0x10000000u32);
            self.w32(v7 - 68, 805314560u32);
            self.w32(v7 - 64, 0x4000u32);
            self.w32(v7 - 60, 536875008u32);
            self.w32(v7 - 56, 1073754112u32);
            self.w32(v7 - 52, 1610633216u32);
            self.w32(v7 - 48, (-2147454976i64) as u32);
            self.w32(v7 - 44, 0x10000000u32);
            self.w32(v7 - 40, 805314560u32);
            self.w32(v7 - 36, 1342193664u32);
            self.w32(v7 - 32, 1879072768u32);
            self.w16(v7 - 28, 0x8000u16);
            self.w32(v7 - 8, 76677120u32);
            self.w32(v7 - 4, 230099237u32);
            self.w32(v7, 383455817u32);
            self.w32(v7 + 4, 536877934u32);
            self.w32(v7 + 8, 76677120u32);
            self.w32(v7 + 12, 230099237u32);
            self.w32(v7 + 16, 383455817u32);
            self.w32(v7 + 20, 536877934u32);
            self.w32(v7 + 24, 76677120u32);
            self.w32(v7 + 28, 230099237u32);
            self.w32(v7 + 32, 383455817u32);
            self.w32(v7 + 36, 536877934u32);
            self.w32(v7 + 40, 76677120u32);
            self.w32(v7 + 44, 230099237u32);
            self.w32(v7 + 48, 383455817u32);
            self.w32(v7 + 52, 536877934u32);
            self.w32(v7 + 56, 76677120u32);
            self.w32(v7 + 60, 230099237u32);
            self.w32(v7 + 64, 383455817u32);
            self.w32(v7 + 68, 536877934u32);
            self.w32(v7 + 72, 76677120u32);
            self.w32(v7 + 76, 230099237u32);
            self.w32(v7 + 80, 383455817u32);
            self.w32(v7 + 84, 536877934u32);
            self.w32(v7 - 26, 0x10000000u32);
            self.w32(v7 - 22, 805314560u32);
            self.w32(v7 - 18, 1342193664u32);
            self.w32(v7 - 14, 1879072768u32);
            self.w16(v7 - 10, 0x8000u16);
            v6 += 218;
            v7 += 218;
        }
        if let Some(lens) = lens {
            self.ba70(b, lens)?;
        } else {
            let mut v8 = b + 1116;
            for _ in 0..256 {
                self.w32(v8 - 4, 0x8000000u32);
                self.w16(v8 + 28, 0x8000u16);
                self.w32(v8, 402657280u32);
                self.w32(v8 + 4, 671096832u32);
                self.w32(v8 + 8, 939536384u32);
                self.w32(v8 + 12, 1207975936u32);
                self.w32(v8 + 16, 1476415488u32);
                self.w32(v8 + 20, 1744855040u32);
                self.w32(v8 + 24, 2013294592u32);
                v8 += 34;
            }
            let mut v11 = b + 9820;
            for _ in 0..4096 {
                self.w32(v11 - 4, 0x8000000u32);
                self.w16(v11 + 28, 0x8000u16);
                self.w32(v11, 402657280u32);
                self.w32(v11 + 4, 671096832u32);
                self.w32(v11 + 8, 939536384u32);
                self.w32(v11 + 12, 1207975936u32);
                self.w32(v11 + 16, 1476415488u32);
                self.w32(v11 + 20, 1744855040u32);
                self.w32(v11 + 24, 2013294592u32);
                v11 += 34;
            }
        }
        self.w32(b + 1092, 96u32);
        self.w16(b + 1090, 1024u16);
        self.w32(b + 1096, 128u32);
        self.w32(b + 1100, 80u32);
        self.w32(b + 1104, 112u32);
        Ok(())
    }

    /// `InitSnowflakeDecompState`.
    fn init(&mut self) -> R<()> {
        self.w64(0x246C8, u64::MAX);
        self.w64(0x40, 0);
        let v2 = self.bits(6) as u32;
        let first = self.bits(v2);
        let total = (first | (1u64 << v2)).wrapping_sub(1);
        self.w64(TOTAL, total);
        if total > MAX_OUTPUT {
            return Err(SnowflakeError::TooBig);
        }
        let hb = highest_bit((total >> 6) + 100 + total).map_or(-1i32, |b| b as i32);
        let min_len = self.bits((hb + 1) as u32);
        self.w64(MIN_LEN, min_len);
        let v7 = self.bits(4) as u32;
        let v8 = self.bits(4) as u32;
        self.w32(BLOCK_LEN, 1 << (v7 + 9));
        self.w32(BLOCK_LEN + 4, 1 << (v8 + 9));
        self.w32(COUNT_BITS, v7 + 8);
        let v9 = self.bits(2) as u8;
        let v10 = (self.bits(3) as u32) | 8;
        if v10 <= 8 {
            return self.c880(48, v9, None);
        }
        let v12 = self.r32(BITCNT);
        let v13 = v12.wrapping_neg() & 0x1F;
        let v14 = self.r64(CURSOR) - if v12 != 0 { 4 } else { 0 };
        let (v15, lens) = self.c680(v14 + u64::from(v13 >> 3), ((v12 as u8).wrapping_neg()) & 7, v10)?;
        self.c880(48, v9, Some(&lens))?;
        let v16 = v14 + 4 * (((v15 + i64::from(v13) + 31) as u64) >> 5);
        self.w64(CURSOR, v16);
        let v17 = ((v15 + i64::from(v13)) & 0x1F) as u32;
        if v17 != 0 {
            let w = self.in32(v16 - 4) as u32;
            self.w32(BITBUF, w >> v17);
            self.w32(BITCNT, 32 - v17);
        } else {
            self.w64(BITBUF, 0);
        }
        Ok(())
    }

    fn put(&mut self, at: u64, b: u8) -> R<()> {
        *self.out.get_mut(at as usize).ok_or(SnowflakeError::TooBig)? = b;
        Ok(())
    }

    /// `sub_7FF7FC23CD20`: `count` literals, written at the output position.
    fn literals(&mut self, count: u32) -> R<()> {
        let m1 = set32(0x77106f2, 0x67305f4, 0x57504f6, 0x47703f8);
        let m2 = set32(0x788f788f, 0x788f788f, 0x788f788f, 0x788f788f);
        let m3 = set32(0x37902fa, 0x27b01fc, 0x17d00fe, 0x7f0000);
        let m4 = set32(0x3b10372, 0x33302f4, 0x2b50276, 0x23701f8);
        let m5 = set32(0x7c4f7c4f, 0x7c4f7c4f, 0x7c4f7c4f, 0x7c4f7c4f);
        let m6 = set32(0x1b9017a, 0x13b00fc, 0xbd007e, 0x3f0000);
        let m7 = set32(0xf000e, 0xd000c, 0xb000a, 0x90008);
        let m8 = set32(0x70006, 0x50004, 0x30002, 0x10000);

        let mut v2 = self.r64(OUTPOS);
        let mut v3 = u32::from(self.r8(PREV));
        let mut at = v2;
        self.w64(OUTPOS, v2 + u64::from(count));
        if v2 + u64::from(count) > self.out.len() as u64 {
            return Err(SnowflakeError::TooBig);
        }
        let mask = u32::from(self.r8(1156));
        if count > 0xF {
            let v7 = u32::from(self.r16(1136));
            let x0 = self.r64(X0);
            let v8 = x0 >> 12;
            let v9 = x0 & 0xFFF;
            let v10 = 4096 - v7;
            if (x0 as u32 & 0xFFF) >= v7 {
                // Raw mode: the literals are the low bytes of the state.
                let mut v11 = v9.wrapping_add(v8 * u64::from(v10)).wrapping_sub(u64::from(v7));
                self.w16(1136, (v7 - (v7 >> 4)) as u16);
                v11 = self.renorm(v11);
                let x1 = self.r64(X1);
                self.w64(X0, x1);
                self.w64(X1, v11);
                let mut v20 = 0u32;
                for _ in 0..count {
                    let st = self.r64(X0);
                    v20 = st as u8 as u32;
                    let v21 = self.renorm(st >> 8);
                    let nx = self.r64(X1);
                    self.w64(X0, nx);
                    self.w64(X1, v21);
                    let v25 = ((v2 as u32) & mask) * 16;
                    v2 += 1;
                    self.put(at, v20 as u8)?;
                    at += 1;
                    let v26 = 34 * (v25 | (v3 >> 4)) as usize;
                    let v27 = self.ldv(1160 + v26);
                    let v28 = self.ldv(1176 + v26);
                    let v29 = (v3 & 0xF) | (16 * (v25 | (v20 >> 4)));
                    v3 = v20;
                    let v31 = splat(u64::from(v20 >> 4));
                    let n32 = adapt(v28, cmpgt(m7, v31), m2, m1, 7);
                    let n27 = adapt(v27, cmpgt(m8, v31), m2, m3, 7);
                    self.stv(1160 + v26, n27);
                    self.stv(1176 + v26, n32);
                    let v33 = 34 * v29 as usize;
                    let lo = u64::from(v20 & 0xF);
                    let v37 = splat(lo);
                    let v35 = self.ldv(9880 + v33);
                    let v39 = self.ldv(9864 + v33);
                    let n38 = adapt(v35, cmpgt(m7, v37), m5, m4, 6);
                    let n39 = adapt(v39, cmpgt(m8, v37), m5, m6, 6);
                    self.stv(9864 + v33, n39);
                    self.stv(9880 + v33, n38);
                }
                self.w8(PREV, v20 as u8);
                return Ok(());
            }
            let mut v40 = v9.wrapping_add(u64::from(v7) * v8);
            self.w16(1136, (v7 + (v10 >> 4)) as u16);
            v40 = self.renorm(v40);
            let x1 = self.r64(X1);
            self.w64(X0, x1);
            self.w64(X1, v40);
        }
        // Modelled mode: each literal is a high nibble then a low nibble.
        let mut v43 = v2 as u8;
        let mut v44 = (v3 >> 4) | (16 * (u32::from(v2 as u8) & mask));
        let v45 = 1160 + 34 * v44 as usize;
        let v46 = self.ldv(v45);
        let v47 = self.ldv(v45 + 16);
        let x0 = self.r64(X0);
        let v49 = splat(x0 & 0x7FFF);
        let v50 = cmpgt(v46, v49);
        let v51 = cmpgt(v47, v49);
        let v52 = lanes(v50) + lanes(v51);
        let mut v53 = 15 - v52;
        let v54 = self.t(v45 + 2 * (15 - v52) as usize);
        let v55 = rans(x0, 15, v54, self.t(v45 + 2 * (16 - v52) as usize));
        let v55 = self.renorm(v55);
        let x1 = self.r64(X1);
        self.w64(X0, x1);
        self.w64(X1, v55);
        self.stv(v45, adapt(v46, v50, m2, m3, 7));
        self.stv(v45 + 16, adapt(v47, v51, m2, m1, 7));
        let mut hi = 15 - v52; // the high nibble waiting for its low nibble
        if count != 1 {
            let mut v65 = 16 * mask;
            for _ in 0..count - 1 {
                v44 = v53 + (v65 & (v44 + 16));
                let v67 = 1160 + 34 * v44 as usize;
                let v68 = self.ldv(v67);
                let v69 = self.ldv(v67 + 16);
                let v102 = (v3 & 0xF) | (16 * (hi | (16 * (mask & u32::from(v43)))));
                let v101 = 9864 + 34 * v102 as usize;
                let v70 = self.ldv(v101);
                let v71 = self.ldv(v101 + 16);
                let (x0, x1) = (self.r64(X0), self.r64(X1));
                let v73 = splat(x0 & 0x7FFF);
                let v74 = cmpgt(v70, v73);
                let v75 = cmpgt(v71, v73);
                let v77 = splat(x1 & 0x7FFF);
                let v78 = cmpgt(v68, v77);
                let v79 = cmpgt(v69, v77);
                // _BitScanForward of the compare mask with bit 16 set: the first lane that is set.
                let first = |a: V, b: V| a.iter().chain(b.iter()).position(|&x| x != 0).map_or(16, |i| i as u32);
                v65 = first(v74, v75);
                let v81 = first(v78, v79);
                if v65 == 0 || v81 == 0 {
                    return Err(SnowflakeError::Corrupt("literal model"));
                }
                let v82 = self.t(v101 + 2 * (v65 as usize - 1));
                let nx0 = rans(x0, 15, v82, self.t(v101 + 2 * v65 as usize));
                let c0 = self.t(v67 + 2 * (v81 as usize - 1));
                let nx1 = rans(x1, 15, c0, self.t(v67 + 2 * v81 as usize));
                let nx0 = self.renorm(nx0);
                let nx1 = self.renorm(nx1);
                self.w64(X0, nx0);
                self.w64(X1, nx1);
                let byte = (v65 - 1) | (16 * hi);
                hi = v81 - 1;
                v3 = byte;
                self.stv(v101, adapt(v70, v74, m5, m6, 6));
                self.stv(v101 + 16, adapt(v71, v75, m5, m4, 6));
                self.stv(v67, adapt(v68, v78, m2, m3, 7));
                self.stv(v67 + 16, adapt(v69, v79, m2, m1, 7));
                v43 = v43.wrapping_add(1);
                v53 = hi;
                self.put(at, byte as u8)?;
                at += 1;
                v65 = 16 * mask;
            }
        }
        let v20 = 9864 + 34 * ((v3 & 0xF) | (16 * (v53 | (16 * (mask & u32::from(v43)))))) as usize;
        let v89 = self.ldv(v20);
        let v90 = self.ldv(v20 + 16);
        let x0 = self.r64(X0);
        let v92 = splat(x0 & 0x7FFF);
        let v93 = cmpgt(v89, v92);
        let v94 = cmpgt(v90, v92);
        let v95 = lanes(v93) + lanes(v94);
        let v96 = self.t(v20 + 2 * (15 - v95) as usize);
        let v97 = rans(x0, 15, v96, self.t(v20 + 2 * (16 - v95) as usize));
        let v97 = self.renorm(v97);
        let x1 = self.r64(X1);
        self.w64(X0, x1);
        self.w64(X1, v97);
        self.stv(v20, adapt(v89, v93, m5, m6, 6));
        self.stv(v20 + 16, adapt(v90, v94, m5, m4, 6));
        let last = ((16 * hi) | (15 - v95)) as u8;
        self.put(at, last)?;
        self.w8(PREV, last);
        Ok(())
    }
}

/// Where the command loop is (the labels of the original function).
#[derive(Clone, Copy)]
enum Pc {
    /// Start of a block: read its header.
    Top,
    /// Commands of a block.
    Commands,
    /// End of a block: the tail literals and the stored states.
    Tail,
}

impl<'a> Snow<'a> {
    /// `x0 <- x1; x1 <- v` after refilling `v`.
    fn step(&mut self, v: u64) {
        let v = self.renorm(v);
        let x1 = self.r64(X1);
        self.w64(X0, x1);
        self.w64(X1, v);
    }

    /// `DecompressSnowflake`. `data_size` is the size of the input, `cap` the output we may fill.
    /// `true` when the whole stream was decoded.
    fn run(&mut self, data_size: u64, cap: u64) -> R<bool> {
        let c2 = set32(0x3f003b1, 0x3720333, 0x2f402b5, 0x2760237);
        let c3 = set32(0x7c107c10, 0x7c107c10, 0x7c107c10, 0x7c107c10);
        let c4 = set32(0x1f801b9, 0x17a013b, 0xfc00bd, 0x7e003f);
        let c5 = set32(0xd900ba, 0x9b007c, 0x5d003e, 0x1f0000);
        let unk_m = set32(0x1b9017a, 0x13b00fc, 0xbd007e, 0x3f0000);
        // `_mm_shuffle_epi8` masks for moving a recent offset to the front.
        let shuffles: [[u8; 16]; 4] = [
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            [4, 5, 6, 7, 0, 1, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15],
            [8, 9, 10, 11, 0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15],
            [12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        ];

        let total = self.r64(TOTAL);
        if total < 0x40 {
            let at = self.r64(CURSOR);
            for i in 0..total {
                let b = *self.input.get((at + i) as usize).ok_or(SnowflakeError::Corrupt("truncated"))?;
                self.put(i, b)?;
            }
            return Ok(true);
        }
        let mut v5 = data_size;
        if data_size < self.r64(MIN_LEN) {
            if data_size < 0x21C {
                return Ok(false);
            }
            v5 = data_size - 540;
        }
        let v193 = v5;
        let mut v8 = self.r32(COUNT);
        let mut v9 = self.r64(BLOCK_END);
        if cap < v9 {
            return Ok(false);
        }
        let mut pc = if v8 == 0 { Pc::Top } else { Pc::Commands };
        loop {
            match pc {
                Pc::Top => {
                    let v15 = self.r64(OUTPOS);
                    if v9.wrapping_sub(v15) <= 8 {
                        // The last bytes of the stream are stored as they are.
                        let end = self.r64(BLOCK_END);
                        let mut i = v15;
                        while i < end {
                            let at = self.r64(CURSOR);
                            let b = *self.input.get(at as usize).ok_or(SnowflakeError::Corrupt("truncated"))?;
                            self.put(i, b)?;
                            self.w64(CURSOR, at + 1);
                            i += 1;
                            self.w64(OUTPOS, i);
                        }
                        return Ok(true);
                    }
                    let n = self.r32(COUNT_BITS);
                    v8 = self.bits(n) as u32;
                    let l0 = (self.bits(5) as u32) | 0x20;
                    let s0 = self.bits(l0) | (1u64 << l0);
                    let l1 = (self.bits(5) as u32) | 0x20;
                    let s1 = self.bits(l1) | (1u64 << l1);
                    self.w64(X0, s0);
                    self.w64(INPOS, self.r64(CURSOR));
                    self.w64(X1, s1);
                    v5 = v193;
                    pc = if v8 != 0 { Pc::Commands } else { Pc::Tail };
                }
                Pc::Tail => {
                    let left = (self.r64(BLOCK_END) as u32).wrapping_sub(self.r64(OUTPOS) as u32) as i32;
                    if left != 8 {
                        self.literals(left.wrapping_sub(8) as u32)?;
                    }
                    let op = self.r64(OUTPOS);
                    let (x0, x1) = (self.r64(X0) as u32, self.r64(X1) as u32);
                    for (i, b) in x1.to_le_bytes().into_iter().chain(x0.to_le_bytes()).enumerate() {
                        self.put(op + i as u64, b)?;
                    }
                    let end = self.r64(BLOCK_END);
                    v9 = end + u64::from(self.r32(BLOCK_LEN));
                    self.w64(OUTPOS, end);
                    self.w64(BLOCK_END, v9);
                    if v9 > total {
                        if end == total {
                            return Ok(true);
                        }
                        self.w64(BLOCK_END, total);
                        v9 = total;
                    }
                    self.w64(CURSOR, self.r64(INPOS));
                    if cap < v9 {
                        return Ok(false);
                    }
                    pc = Pc::Top;
                }
                Pc::Commands => {
                    let v18 = v8 - 1;
                    loop {
                        if self.r64(INPOS) > v5 {
                            self.w32(COUNT, v8);
                            return Ok(false);
                        }
                        // Literal length class.
                        let x0 = self.r64(X0);
                        let v21 = self.ldv(50);
                        let v22 = self.ldv(66);
                        let v23 = splat(x0 & 0x7FFF);
                        let v24 = cmpgt(v21, v23);
                        let v25 = cmpgt(v22, v23);
                        let v26 = lanes(v24) + lanes(v25);
                        let mut v27 = 16 - v26;
                        let v29 = rans(x0, 15, self.t(48 + 2 * v27 as usize), self.t(48 + 2 * (17 - v26) as usize));
                        self.step(v29);
                        self.stv(50, adapt(v21, v24, c3, c4, 6));
                        self.stv(66, adapt(v22, v25, c3, c2, 6));
                        if v27 != 0 {
                            let mut count = v27;
                            if v27 == 16 {
                                count = self.long_literals(c4, c5)?;
                                v27 = count;
                            }
                            self.literals(count)?;
                            if v27 == 511 {
                                continue;
                            }
                        }

                        // Match length.
                        let x0 = self.r64(X0);
                        let v60 = self.ldv(124);
                        let v61 = self.ldv(140);
                        let v62 = splat(x0 & 0x7FFF);
                        let v63 = cmpgt(v60, v62);
                        let v64 = cmpgt(v61, v62);
                        let v65 = lanes(v63) + lanes(v64);
                        let v66 = 16 - v65;
                        let v68 = rans(x0, 15, self.t(122 + 2 * (16 - v65) as usize), self.t(122 + 2 * (17 - v65) as usize));
                        self.step(v68);
                        self.stv(124, adapt(v60, v63, c3, c4, 6));
                        self.stv(140, adapt(v61, v64, c3, c2, 6));
                        let mut len = u64::from(v66 + 3);
                        if v66 + 3 >= 0x12 {
                            let x0 = self.r64(X0);
                            if v66 == 15 {
                                let v75 = self.ldv(160);
                                let v76 = self.ldv(176);
                                let v77 = splat(x0 & 0x3FFF);
                                let v78 = cmpgt(v75, v77);
                                let v79 = cmpgt(v76, v77);
                                let v80 = lanes(v78) + lanes(v79);
                                let v82 = rans(x0, 14, self.t(158 + 2 * (16 - v80) as usize), self.t(158 + 2 * (17 - v80) as usize));
                                len = u64::from(16 - v80 + 18);
                                self.step(v82);
                                let s = set32(0x3e103e10, 0x3e103e10, 0x3e103e10, 0x3e103e10);
                                self.stv(160, adapt(v75, v78, s, set32(0xf800d9, 0xba009b, 0x7c005d, 0x3e001f), 5));
                                self.stv(176, adapt(v76, v79, s, set32(0x1f001d1, 0x1b20193, 0x1740155, 0x1360117), 5));
                            } else {
                                let x1 = self.r64(X1);
                                let v86 = self.ldv(196);
                                let v87 = self.ldv(212);
                                let v88 = self.ldv(230);
                                let v89 = self.ldv(246);
                                let v91 = splat(x0 & 0x7FFF);
                                let v92 = cmpgt(v86, v91);
                                let v93 = cmpgt(v87, v91);
                                let v95 = splat(x1 & 0x3FFF);
                                let v96 = cmpgt(v88, v95);
                                let v97 = cmpgt(v89, v95);
                                let v98 = lanes(v92) + lanes(v93);
                                let v99 = lanes(v96) + lanes(v97);
                                let v100 = 16 - v98;
                                let v102 = 15 - v99;
                                let v104 = rans(x0, 15, self.t(194 + 2 * (16 - v98) as usize), self.t(194 + 2 * (17 - v98) as usize));
                                let v105 = rans(x1, 14, self.t(230 + 2 * v102 as usize), self.t(230 + 2 * (16 - v99) as usize));
                                let v104 = self.renorm(v104);
                                let v105 = self.renorm(v105);
                                let s = set32(0x3c4f3c4f, 0x3c4f3c4f, 0x3c4f3c4f, 0x3c4f3c4f);
                                self.w64(X0, v104);
                                self.w64(X1, v105);
                                let v112 = (v104 as u32) & ((1u32 << (17 - v98)) - 1);
                                self.stv(196, adapt(v86, v92, c3, c4, 6));
                                self.stv(212, adapt(v87, v93, c3, c2, 6));
                                let v113 = v104 >> (17 - v98);
                                self.stv(230, adapt(v88, v96, s, unk_m, 6));
                                self.stv(246, adapt(v89, v97, s, set32(0x3b10372, 0x33302f4, 0x2b50276, 0x23701f8), 6));
                                let v113 = self.renorm(v113);
                                let nx = self.r64(X1);
                                self.w64(X0, nx);
                                self.w64(X1, v113);
                                len = u64::from(v102 | (16 * v112)) | u64::from(32u32 << v100);
                            }
                        }

                        // Offset.
                        let ctx = if len >= 3 && len - 3 < 3 { (len - 3) as usize } else { 3 };
                        let v117 = 218 * ctx;
                        let x0 = self.r64(X0);
                        let v119 = self.ldv(v117 + 264);
                        let v120 = cmpgt(v119, splat(x0 & 0xFFF));
                        let v121 = lanes(v120) as usize;
                        if v121 > 7 {
                            return Err(SnowflakeError::Corrupt("offset class"));
                        }
                        let v123 = rans(x0, 12, self.t(v117 + 2 * (7 - v121) + 264), self.t(v117 + 2 * (8 - v121) + 264));
                        self.step(v123);
                        let k = set32(0xf270f27, 0xf270f27, 0xf270f27, 0xf270000);
                        self.stv(v117 + 264, adapt(v119, v120, k, c5, 5));
                        let offset: u32;
                        if 7 - v121 > 1 {
                            let (x0, x1) = (self.r64(X0), self.r64(X1));
                            let v155 = self.ldv(v117 + 368);
                            let v156 = v117 + 16 * (5 - v121);
                            let v157 = self.ldv(v156 + 386);
                            let v158 = (3 * (7 - v121)) as u32;
                            let v161 = cmpgt(v157, splat(x0 & 0x1FFF));
                            let v162 = cmpgt(v155, splat(x1 & 0x7FFF));
                            let v163 = lanes(v161) as usize;
                            let v164 = 7 - v163;
                            let v166 = lanes(v162) as usize;
                            let v168 = rans(x0, 13, self.t(v156 + 2 * v164 + 386), self.t(v156 + 2 * (8 - v163) + 386));
                            let v169 = rans(x1, 15, self.t(v117 + 2 * (7 - v166) + 368), self.t(v117 + 2 * (8 - v166) + 368));
                            let v168 = self.renorm(v168);
                            let v169 = self.renorm(v169);
                            let v173 = set32(0x20001e86, 0x1e861e86, 0x1e861e86, 0x1e861e86);
                            let v174 = set32(0x7e477e47, 0x7e477e47, 0x7e477e47, 0x7e477e47);
                            self.w64(X0, v168);
                            self.w64(X1, v169);
                            self.stv(v156 + 386, adapt(v157, v161, v173, set32(0x17a, 0x13b00fc, 0xbd007e, 0x3f0000), 6));
                            self.stv(v117 + 368, adapt(v155, v162, v174, unk_m, 6));
                            let v175 = v168 >> (v158 - 3);
                            let v176 = (v168 as u32) & ((1u32 << (v158 - 3)) - 1);
                            let v175 = self.renorm(v175);
                            self.w64(X0, v169);
                            self.w64(X1, v175);
                            offset = ((7 - v166 as u32) | (8 * v176) | ((v164 as u32 + 1) << v158)).wrapping_add(1);
                            let idx = self.r8(HIST_IDX);
                            let hist: [u8; 16] = self.m[HIST..HIST + 16].try_into().expect("16 bytes");
                            let mut shifted = [0u8; 16];
                            shifted[4..].copy_from_slice(&hist[..12]);
                            shifted[..4].copy_from_slice(&offset.to_le_bytes());
                            self.w8(HIST_IDX, (i32::from(idx) - i32::from(idx >> 2) + 1) as u8);
                            self.m[HIST..HIST + 16].copy_from_slice(&shifted);
                        } else if v121 == 6 {
                            let (x0, x1) = (self.r64(X0), self.r64(X1));
                            let v127 = self.ldv(v117 + 332);
                            let v128 = self.ldv(v117 + 350);
                            let v131 = cmpgt(v127, splat(x0 & 0x7FFF));
                            let v132 = cmpgt(v128, splat(x1 & 0x7FFF));
                            let v133 = lanes(v131) as usize;
                            let v136 = lanes(v132) as usize;
                            let v138 = rans(x0, 15, self.t(v117 + 2 * (7 - v133) + 332), self.t(v117 + 2 * (8 - v133) + 332));
                            let v139 = rans(x1, 15, self.t(v117 + 2 * (7 - v136) + 350), self.t(v117 + 2 * (8 - v136) + 350));
                            let v138 = self.renorm(v138);
                            let v139 = self.renorm(v139);
                            self.w64(X0, v138);
                            self.w64(X1, v139);
                            offset = (((7 - v136) | (8 * (7 - v133))) + 1) as u32;
                            let k1 = set32(0x7c877c87, 0x7c877c87, 0x7c877c87, 0x7c877c87);
                            let k2 = set32(0x7f277f27, 0x7f277f27, 0x7f277f27, 0x7f277f27);
                            self.stv(v117 + 332, adapt(v127, v131, k1, set32(0x37902fa, 0x27b01fc, 0x17d00fe, 0x7f0000), 7));
                            self.stv(v117 + 350, adapt(v128, v132, k2, c5, 5));
                        } else {
                            // A recent offset.
                            let idx = usize::from(self.r8(HIST_IDX));
                            let v146 = v117 + 10 * idx;
                            let mut v147: V = [0; 8];
                            for (i, slot) in v147.iter_mut().take(4).enumerate() {
                                *slot = self.r16(v146 + 282 + 2 * i) as i16;
                            }
                            let x0 = self.r64(X0);
                            let probe = (x0 & 0x3FFF) as u16 as i16;
                            let v148 = lanes(cmpgt(v147, [probe, probe, probe, probe, 0, 0, 0, 0])) as usize;
                            if v148 > 3 {
                                return Err(SnowflakeError::Corrupt("recent offset"));
                            }
                            let v149 = 3 - v148;
                            let v151 = rans(x0, 14, self.t(v146 + 2 * v149 + 282), self.t(v146 + 2 * (4 - v148) + 282));
                            self.step(v151);
                            let lut = LUT_SNOWFLAKE_1[v149];
                            let target: V = std::array::from_fn(|i| if i < 4 { (lut >> (16 * i)) as u16 as i16 } else { 0 });
                            let updated = add(srai(sub(target, v147), 6), v147);
                            for (i, x) in updated.iter().take(4).enumerate() {
                                self.w16(v146 + 282 + 2 * i, *x as u16);
                            }
                            offset = self.r32(HIST + 4 * v149);
                            self.w8(HIST_IDX, 0);
                            let hist: [u8; 16] = self.m[HIST..HIST + 16].try_into().expect("16 bytes");
                            let sh = shuffles[v149];
                            let moved: [u8; 16] = std::array::from_fn(|i| hist[usize::from(sh[i])]);
                            self.m[HIST..HIST + 16].copy_from_slice(&moved);
                        }

                        // Copy the match.
                        let at = self.r64(OUTPOS);
                        self.w64(OUTPOS, at + len);
                        if offset == 0 || u64::from(offset) > at || at + len > self.out.len() as u64 {
                            return Err(SnowflakeError::Corrupt("match leaves the output"));
                        }
                        for i in 0..len {
                            let b = self.out[(at + i - u64::from(offset)) as usize];
                            self.out[(at + i) as usize] = b;
                        }
                        let last = self.out[(at + len - 1) as usize];
                        self.w8(PREV, last);
                        v8 = v18;
                        pc = if v18 == 0 { Pc::Tail } else { Pc::Commands };
                        break;
                    }
                }
            }
        }
    }

    /// The literal count of an extended literal-length symbol.
    fn long_literals(&mut self, c4: V, c5: V) -> R<u32> {
        let x0 = self.r64(X0);
        let v32 = self.ldv(104);
        let v33 = cmpgt(v32, splat(x0 & 0x7FFF));
        let v34 = lanes(v33) as usize;
        let v35 = 7 - v34;
        let v37 = rans(x0, 15, self.t(104 + 2 * (7 - v34)), self.t(104 + 2 * (8 - v34)));
        self.step(v37);
        let k = set32(0x7f277f27, 0x7f277f27, 0x7f277f27, 0x7f270000);
        self.stv(104, adapt(v32, v33, k, c5, 5));

        let x0 = self.r64(X0);
        let v41 = self.ldv(86);
        let v42 = cmpgt(v41, splat(x0 & 0x3FFF));
        let v43 = lanes(v42) as usize;
        let v44 = 8 - v43;
        let v46 = rans(x0, 14, self.t(84 + 2 * v44), self.t(84 + 2 * (9 - v43)));
        self.step(v46);
        let k = set32(0x3e083e08, 0x3e083e08, 0x3e083e08, 0x3e083e08);
        self.stv(86, adapt(v41, v42, k, c4, 6));
        if v44 <= 5 {
            return Ok((v35 + 8 * (v44 + 2)) as u32);
        }
        let x0 = self.r64(X0);
        let (v51, v55) = if v44 == 16 {
            let v52 = self.renorm(x0 >> 3);
            let x1 = self.r64(X1);
            self.w64(X0, x1);
            self.w64(X1, v52);
            (x1, (x0 & 7) as u32 + 16)
        } else {
            (x0, v44 as u32)
        };
        let v56 = self.renorm(v51 >> (v55 - 3));
        let x1 = self.r64(X1);
        self.w64(X0, x1);
        self.w64(X1, v56);
        let low = (v51 as u32) & ((1u32 << (v55 - 3)) - 1);
        Ok(v35 as u32 + ((8 * low) | (1u32 << v55)))
    }
}

/// Decodes a Snowflake stream (`input` is the whole stored block, padding included) to the size
/// the stream says it has.
pub fn decompress(input: &[u8]) -> Result<Vec<u8>, SnowflakeError> {
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut s = Snow { m: vec![0u8; STATE_LEN], input, out: Vec::new() };
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
mod tests {
    use super::*;

    #[test]
    fn small_streams_are_stored() {
        // Header: 6 bits n = 2, 2 bits = 2 (decoded size 5), then zero fields (7 + 4 + 4 + 2 + 3 bits).
        let mut stream = vec![130u8, 0, 0, 0];
        stream.extend_from_slice(b"hello");
        assert_eq!(decompress(&stream).unwrap(), b"hello");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for seed in 0..64u8 {
            let junk: Vec<u8> = (0..4096u32).map(|i| (i as u8).wrapping_mul(seed | 1).wrapping_add(seed)).collect();
            let _ = decompress(&junk);
        }
    }

    #[test]
    fn set32_orders_lanes_like_sse() {
        assert_eq!(set32(0x00070006, 0x00050004, 0x00030002, 0x00010000), [0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
