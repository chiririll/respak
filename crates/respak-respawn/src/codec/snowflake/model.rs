//! Model set-up: `InitSnowflakeDecompState` and the tables it builds.

use super::simd::highest_bit;
use super::state::*;
use super::{SnowflakeError, MAX_OUTPUT, R};

pub(super) const LUT_SNOWFLAKE_0: [u8; 16] =
    [0, 3, 7, 15, 35, 63, 82, 71, 66, 69, 0, 0, 0, 0, 0, 0];

/// The start state of a probability model: the cumulative frequencies of eight equal steps.
const UNIFORM: [u32; 7] = [
    402657280, 671096832, 939536384, 1207975936, 1476415488, 1744855040, 2013294592,
];

/// `(offset, value, width in bytes)` writes of `c880` relative to its base.
const HEAD: [(isize, u32, u8); 55] = [
    (0, 0x7880000, 4),
    (4, 0x16970f0f, 4),
    (8, 0x25a61e1e, 4),
    (12, 0x34b52d2d, 4),
    (16, 0x43c43c3c, 4),
    (20, 0x52d34b4b, 4),
    (24, 0x61e25a5a, 4),
    (28, 0x70f16969, 4),
    (32, 0x80007878, 4),
    (36, 0x71c0000, 4),
    (40, 0x15550e39, 4),
    (44, 0x238e1c72, 4),
    (48, 0x31c72aab, 4),
    (52, 0x400038e4, 4),
    (56, 0x10000000, 4),
    (60, 0x30002000, 4),
    (64, 0x50004000, 4),
    (68, 0x70006000, 4),
    (72, 0x8000, 2),
    (74, 0x7880000, 4),
    (78, 0x16970f0f, 4),
    (82, 0x25a61e1e, 4),
    (86, 0x34b52d2d, 4),
    (90, 0x43c43c3c, 4),
    (94, 0x52d34b4b, 4),
    (98, 0x61e25a5a, 4),
    (102, 0x70f16969, 4),
    (106, 0x80007878, 4),
    (110, 0x3c40000, 4),
    (114, 0xb4b0788, 4),
    (118, 0x12d30f0f, 4),
    (122, 0x1a5a1697, 4),
    (126, 0x21e21e1e, 4),
    (130, 0x296925a6, 4),
    (134, 0x30f12d2d, 4),
    (138, 0x387834b5, 4),
    (142, 0x40003c3c, 4),
    (146, 0x7880000, 4),
    (150, 0x16970f0f, 4),
    (154, 0x25a61e1e, 4),
    (158, 0x34b52d2d, 4),
    (162, 0x43c43c3c, 4),
    (166, 0x52d34b4b, 4),
    (170, 0x61e25a5a, 4),
    (174, 0x70f16969, 4),
    (178, 0x80007878, 4),
    (182, 0x4000000, 4),
    (186, 0xc000800, 4),
    (190, 0x14001000, 4),
    (194, 0x1c001800, 4),
    (198, 0x24002000, 4),
    (202, 0x2c002800, 4),
    (206, 0x34003000, 4),
    (210, 0x3c003800, 4),
    (214, 0x4000, 2),
];

/// The same, for each of the four blocks of 218 bytes that start at base + 216.
const REPEATED: [(isize, u32, u8); 56] = [
    (2, 0x4000200, 4),
    (0, 0x0, 2),
    (6, 0x8000600, 4),
    (10, 0xc000a00, 4),
    (14, 0x10000e00, 4),
    (18, 0x10000000, 4),
    (22, 0x30002000, 4),
    (26, 0x4000, 4),
    (30, 0x20001000, 4),
    (34, 0x40003000, 4),
    (38, 0x10000000, 4),
    (42, 0x30002000, 4),
    (46, 0x4000, 4),
    (50, 0x20001000, 4),
    (54, 0x40003000, 4),
    (58, 0x10000000, 4),
    (62, 0x30002000, 4),
    (66, 0x4000, 4),
    (70, 0x20001000, 4),
    (74, 0x40003000, 4),
    (78, 0x60005000, 4),
    (82, 0x80007000, 4),
    (86, 0x10000000, 4),
    (90, 0x30002000, 4),
    (94, 0x50004000, 4),
    (98, 0x70006000, 4),
    (102, 0x8000, 2),
    (122, 0x4920000, 4),
    (126, 0xdb70925, 4),
    (130, 0x16db1249, 4),
    (134, 0x20001b6e, 4),
    (138, 0x4920000, 4),
    (142, 0xdb70925, 4),
    (146, 0x16db1249, 4),
    (150, 0x20001b6e, 4),
    (154, 0x4920000, 4),
    (158, 0xdb70925, 4),
    (162, 0x16db1249, 4),
    (166, 0x20001b6e, 4),
    (170, 0x4920000, 4),
    (174, 0xdb70925, 4),
    (178, 0x16db1249, 4),
    (182, 0x20001b6e, 4),
    (186, 0x4920000, 4),
    (190, 0xdb70925, 4),
    (194, 0x16db1249, 4),
    (198, 0x20001b6e, 4),
    (202, 0x4920000, 4),
    (206, 0xdb70925, 4),
    (210, 0x16db1249, 4),
    (214, 0x20001b6e, 4),
    (104, 0x10000000, 4),
    (108, 0x30002000, 4),
    (112, 0x50004000, 4),
    (116, 0x70006000, 4),
    (120, 0x8000, 2),
];

impl<'a> Snow<'a> {
    /// `sub_7FF7FC23C680`: a 256-entry table of symbols coded with a 3-bit length prefix code,
    /// read from the input at byte `pos` after `shift` bits. Returns the bits used and the table.
    pub(super) fn c680(&self, pos: u64, shift: u8, a4: u32) -> R<(i64, [u8; 256])> {
        let mut v29 = [0u8; 528];
        let g = |v: &[u8; 528], i: usize| u16::from_le_bytes([v[2 * i], v[2 * i + 1]]);
        let s = |v: &mut [u8; 528], i: usize, x: u16| {
            v[2 * i..2 * i + 2].copy_from_slice(&x.to_le_bytes())
        };
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
                        i = i
                            .checked_sub(1)
                            .ok_or(SnowflakeError::Corrupt("code table"))?;
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
        Ok((
            i64::from(v22) + 8 * (v21 - pos) as i64 - i64::from(shift),
            out,
        ))
    }

    /// `sub_7FF7FC23BA70`: the literal models for a stored length table, based at `b`.
    pub(super) fn ba70(&mut self, b: usize, lens: &[u8; 256]) -> R<()> {
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
    pub(super) fn c880(&mut self, b: usize, sel: u8, lens: Option<&[u8; 256]>) -> R<()> {
        self.w8(b + 1108, LUT_SNOWFLAKE_0[sel as usize]);
        self.w16(b + 1088, 2048u16);
        self.init_table(b, &HEAD);
        for i in 0..4 {
            self.init_table(b + 216 + 218 * i, &REPEATED);
        }
        if let Some(lens) = lens {
            self.ba70(b, lens)?;
        } else {
            self.uniform_literal_models(b + 1112, 256);
            self.uniform_literal_models(b + 9816, 4096);
        }
        self.w32(b + 1092, 96u32);
        self.w16(b + 1090, 1024u16);
        self.w32(b + 1096, 128u32);
        self.w32(b + 1100, 80u32);
        self.w32(b + 1104, 112u32);
        Ok(())
    }

    /// Writes the `(offset, value, width in bytes)` entries of `table` relative to `base`.
    fn init_table(&mut self, base: usize, table: &[(isize, u32, u8)]) {
        for &(off, value, width) in table {
            let at = base.wrapping_add_signed(off);
            if width == 4 {
                self.w32(at, value);
            } else {
                self.w16(at, value as u16);
            }
        }
    }

    /// `count` literal models of 34 bytes starting at `start`, each with the uniform start state.
    fn uniform_literal_models(&mut self, start: usize, count: usize) {
        for i in 0..count {
            let m = start + 34 * i;
            self.w32(m, 0x8000000);
            self.w16(m + 32, 0x8000);
            for (j, v) in UNIFORM.into_iter().enumerate() {
                self.w32(m + 4 + 4 * j, v);
            }
        }
    }

    /// `InitSnowflakeDecompState`.
    pub(super) fn init(&mut self) -> R<()> {
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
        let (v15, lens) = self.c680(
            v14 + u64::from(v13 >> 3),
            ((v12 as u8).wrapping_neg()) & 7,
            v10,
        )?;
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
}
