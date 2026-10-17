//! The command loop of `DecompressSnowflake`: literal length, match length, offset, copy.

use super::simd::*;
use super::state::*;
use super::{SnowflakeError, R};

pub(super) const LUT_SNOWFLAKE_1: [u64; 4] = [
    4611756117654110208,
    4611756116592754688,
    4611686559597395968,
    53199311768322048,
];

/// `_mm_shuffle_epi8` masks for moving a recent offset to the front.
const SHUFFLES: [[u8; 16]; 4] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [4, 5, 6, 7, 0, 1, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15],
    [8, 9, 10, 11, 0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15],
    [12, 13, 14, 15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
];

/// Lane constants shared by the command models.
pub(super) struct Consts {
    c2: V,
    c3: V,
    c4: V,
    c5: V,
    unk_m: V,
}

impl Consts {
    fn new() -> Self {
        Self {
            c2: set32(0x3f003b1, 0x3720333, 0x2f402b5, 0x2760237),
            c3: set32(0x7c107c10, 0x7c107c10, 0x7c107c10, 0x7c107c10),
            c4: set32(0x1f801b9, 0x17a013b, 0xfc00bd, 0x7e003f),
            c5: set32(0xd900ba, 0x9b007c, 0x5d003e, 0x1f0000),
            unk_m: set32(0x1b9017a, 0x13b00fc, 0xbd007e, 0x3f0000),
        }
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
    pub(super) fn step(&mut self, v: u64) {
        let v = self.renorm(v);
        let x1 = self.r64(X1);
        self.w64(X0, x1);
        self.w64(X1, v);
    }

    /// `DecompressSnowflake`. `data_size` is the size of the input, `cap` the output we may fill.
    /// `true` when the whole stream was decoded.
    pub(super) fn run(&mut self, data_size: u64, cap: u64) -> R<bool> {
        let k = Consts::new();
        let total = self.r64(TOTAL);
        if total < 0x40 {
            let at = self.r64(CURSOR);
            for i in 0..total {
                let b = *self
                    .input
                    .get((at + i) as usize)
                    .ok_or(SnowflakeError::Corrupt("truncated"))?;
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
                            let b = *self
                                .input
                                .get(at as usize)
                                .ok_or(SnowflakeError::Corrupt("truncated"))?;
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
                    let left =
                        (self.r64(BLOCK_END) as u32).wrapping_sub(self.r64(OUTPOS) as u32) as i32;
                    if left != 8 {
                        self.literals(left.wrapping_sub(8) as u32)?;
                    }
                    let op = self.r64(OUTPOS);
                    let (x0, x1) = (self.r64(X0) as u32, self.r64(X1) as u32);
                    for (i, b) in x1
                        .to_le_bytes()
                        .into_iter()
                        .chain(x0.to_le_bytes())
                        .enumerate()
                    {
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
                        let v29 = rans(
                            x0,
                            15,
                            self.t(48 + 2 * v27 as usize),
                            self.t(48 + 2 * (17 - v26) as usize),
                        );
                        self.step(v29);
                        self.stv(50, adapt(v21, v24, k.c3, k.c4, 6));
                        self.stv(66, adapt(v22, v25, k.c3, k.c2, 6));
                        if v27 != 0 {
                            let mut count = v27;
                            if v27 == 16 {
                                count = self.long_literals(&k)?;
                                v27 = count;
                            }
                            self.literals(count)?;
                            if v27 == 511 {
                                continue;
                            }
                        }

                        // Match length.
                        let len = self.match_length(&k);

                        // Offset.
                        let offset = self.offset(len, &k)?;

                        // Copy the match.
                        let at = self.r64(OUTPOS);
                        self.w64(OUTPOS, at + len);
                        if offset == 0 || u64::from(offset) > at || at + len > self.out.len() as u64
                        {
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

    /// The match length symbol, and for the longest classes its extension.
    pub(super) fn match_length(&mut self, k: &Consts) -> u64 {
        let x0 = self.r64(X0);
        let v60 = self.ldv(124);
        let v61 = self.ldv(140);
        let v62 = splat(x0 & 0x7FFF);
        let v63 = cmpgt(v60, v62);
        let v64 = cmpgt(v61, v62);
        let v65 = lanes(v63) + lanes(v64);
        let v66 = 16 - v65;
        let v68 = rans(
            x0,
            15,
            self.t(122 + 2 * (16 - v65) as usize),
            self.t(122 + 2 * (17 - v65) as usize),
        );
        self.step(v68);
        self.stv(124, adapt(v60, v63, k.c3, k.c4, 6));
        self.stv(140, adapt(v61, v64, k.c3, k.c2, 6));
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
                let v82 = rans(
                    x0,
                    14,
                    self.t(158 + 2 * (16 - v80) as usize),
                    self.t(158 + 2 * (17 - v80) as usize),
                );
                len = u64::from(16 - v80 + 18);
                self.step(v82);
                let s = set32(0x3e103e10, 0x3e103e10, 0x3e103e10, 0x3e103e10);
                self.stv(
                    160,
                    adapt(
                        v75,
                        v78,
                        s,
                        set32(0xf800d9, 0xba009b, 0x7c005d, 0x3e001f),
                        5,
                    ),
                );
                self.stv(
                    176,
                    adapt(
                        v76,
                        v79,
                        s,
                        set32(0x1f001d1, 0x1b20193, 0x1740155, 0x1360117),
                        5,
                    ),
                );
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
                let v104 = rans(
                    x0,
                    15,
                    self.t(194 + 2 * (16 - v98) as usize),
                    self.t(194 + 2 * (17 - v98) as usize),
                );
                let v105 = rans(
                    x1,
                    14,
                    self.t(230 + 2 * v102 as usize),
                    self.t(230 + 2 * (16 - v99) as usize),
                );
                let v104 = self.renorm(v104);
                let v105 = self.renorm(v105);
                let s = set32(0x3c4f3c4f, 0x3c4f3c4f, 0x3c4f3c4f, 0x3c4f3c4f);
                self.w64(X0, v104);
                self.w64(X1, v105);
                let v112 = (v104 as u32) & ((1u32 << (17 - v98)) - 1);
                self.stv(196, adapt(v86, v92, k.c3, k.c4, 6));
                self.stv(212, adapt(v87, v93, k.c3, k.c2, 6));
                let v113 = v104 >> (17 - v98);
                self.stv(230, adapt(v88, v96, s, k.unk_m, 6));
                self.stv(
                    246,
                    adapt(
                        v89,
                        v97,
                        s,
                        set32(0x3b10372, 0x33302f4, 0x2b50276, 0x23701f8),
                        6,
                    ),
                );
                let v113 = self.renorm(v113);
                let nx = self.r64(X1);
                self.w64(X0, nx);
                self.w64(X1, v113);
                len = u64::from(v102 | (16 * v112)) | u64::from(32u32 << v100);
            }
        }
        len
    }

    /// The match offset: explicit (class 0-5), the short form (6) or one of the four recent ones.
    fn offset(&mut self, len: u64, k: &Consts) -> R<u32> {
        let ctx = if len >= 3 && len - 3 < 3 {
            (len - 3) as usize
        } else {
            3
        };
        let v117 = 218 * ctx;
        let x0 = self.r64(X0);
        let v119 = self.ldv(v117 + 264);
        let v120 = cmpgt(v119, splat(x0 & 0xFFF));
        let v121 = lanes(v120) as usize;
        if v121 > 7 {
            return Err(SnowflakeError::Corrupt("offset class"));
        }
        let v123 = rans(
            x0,
            12,
            self.t(v117 + 2 * (7 - v121) + 264),
            self.t(v117 + 2 * (8 - v121) + 264),
        );
        self.step(v123);
        let km = set32(0xf270f27, 0xf270f27, 0xf270f27, 0xf270000);
        self.stv(v117 + 264, adapt(v119, v120, km, k.c5, 5));
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
            let v168 = rans(
                x0,
                13,
                self.t(v156 + 2 * v164 + 386),
                self.t(v156 + 2 * (8 - v163) + 386),
            );
            let v169 = rans(
                x1,
                15,
                self.t(v117 + 2 * (7 - v166) + 368),
                self.t(v117 + 2 * (8 - v166) + 368),
            );
            let v168 = self.renorm(v168);
            let v169 = self.renorm(v169);
            let v173 = set32(0x20001e86, 0x1e861e86, 0x1e861e86, 0x1e861e86);
            let v174 = set32(0x7e477e47, 0x7e477e47, 0x7e477e47, 0x7e477e47);
            self.w64(X0, v168);
            self.w64(X1, v169);
            self.stv(
                v156 + 386,
                adapt(
                    v157,
                    v161,
                    v173,
                    set32(0x17a, 0x13b00fc, 0xbd007e, 0x3f0000),
                    6,
                ),
            );
            self.stv(v117 + 368, adapt(v155, v162, v174, k.unk_m, 6));
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
            let v138 = rans(
                x0,
                15,
                self.t(v117 + 2 * (7 - v133) + 332),
                self.t(v117 + 2 * (8 - v133) + 332),
            );
            let v139 = rans(
                x1,
                15,
                self.t(v117 + 2 * (7 - v136) + 350),
                self.t(v117 + 2 * (8 - v136) + 350),
            );
            let v138 = self.renorm(v138);
            let v139 = self.renorm(v139);
            self.w64(X0, v138);
            self.w64(X1, v139);
            offset = (((7 - v136) | (8 * (7 - v133))) + 1) as u32;
            let k1 = set32(0x7c877c87, 0x7c877c87, 0x7c877c87, 0x7c877c87);
            let k2 = set32(0x7f277f27, 0x7f277f27, 0x7f277f27, 0x7f277f27);
            self.stv(
                v117 + 332,
                adapt(
                    v127,
                    v131,
                    k1,
                    set32(0x37902fa, 0x27b01fc, 0x17d00fe, 0x7f0000),
                    7,
                ),
            );
            self.stv(v117 + 350, adapt(v128, v132, k2, k.c5, 5));
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
            let v151 = rans(
                x0,
                14,
                self.t(v146 + 2 * v149 + 282),
                self.t(v146 + 2 * (4 - v148) + 282),
            );
            self.step(v151);
            let lut = LUT_SNOWFLAKE_1[v149];
            let target: V = std::array::from_fn(|i| {
                if i < 4 {
                    (lut >> (16 * i)) as u16 as i16
                } else {
                    0
                }
            });
            let updated = add(srai(sub(target, v147), 6), v147);
            for (i, x) in updated.iter().take(4).enumerate() {
                self.w16(v146 + 282 + 2 * i, *x as u16);
            }
            offset = self.r32(HIST + 4 * v149);
            self.w8(HIST_IDX, 0);
            let hist: [u8; 16] = self.m[HIST..HIST + 16].try_into().expect("16 bytes");
            let sh = SHUFFLES[v149];
            let moved: [u8; 16] = std::array::from_fn(|i| hist[usize::from(sh[i])]);
            self.m[HIST..HIST + 16].copy_from_slice(&moved);
        }
        Ok(offset)
    }

    /// The literal count of an extended literal-length symbol.
    pub(super) fn long_literals(&mut self, k: &Consts) -> R<u32> {
        let x0 = self.r64(X0);
        let v32 = self.ldv(104);
        let v33 = cmpgt(v32, splat(x0 & 0x7FFF));
        let v34 = lanes(v33) as usize;
        let v35 = 7 - v34;
        let v37 = rans(
            x0,
            15,
            self.t(104 + 2 * (7 - v34)),
            self.t(104 + 2 * (8 - v34)),
        );
        self.step(v37);
        let km = set32(0x7f277f27, 0x7f277f27, 0x7f277f27, 0x7f270000);
        self.stv(104, adapt(v32, v33, km, k.c5, 5));

        let x0 = self.r64(X0);
        let v41 = self.ldv(86);
        let v42 = cmpgt(v41, splat(x0 & 0x3FFF));
        let v43 = lanes(v42) as usize;
        let v44 = 8 - v43;
        let v46 = rans(x0, 14, self.t(84 + 2 * v44), self.t(84 + 2 * (9 - v43)));
        self.step(v46);
        let km = set32(0x3e083e08, 0x3e083e08, 0x3e083e08, 0x3e083e08);
        self.stv(86, adapt(v41, v42, km, k.c4, 6));
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
