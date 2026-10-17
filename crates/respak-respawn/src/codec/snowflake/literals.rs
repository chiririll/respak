//! Literals: an order-1 nibble model, or raw bytes taken from the rANS state.

use super::simd::*;
use super::state::*;
use super::{SnowflakeError, R};

impl<'a> Snow<'a> {
    pub(super) fn put(&mut self, at: u64, b: u8) -> R<()> {
        *self
            .out
            .get_mut(at as usize)
            .ok_or(SnowflakeError::TooBig)? = b;
        Ok(())
    }

    /// `sub_7FF7FC23CD20`: `count` literals, written at the output position.
    pub(super) fn literals(&mut self, count: u32) -> R<()> {
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
                let mut v11 = v9
                    .wrapping_add(v8 * u64::from(v10))
                    .wrapping_sub(u64::from(v7));
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
                let first = |a: V, b: V| {
                    a.iter()
                        .chain(b.iter())
                        .position(|&x| x != 0)
                        .map_or(16, |i| i as u32)
                };
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
