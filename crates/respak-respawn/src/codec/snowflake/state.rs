//! The decoder state: a flat 0x25000-byte block addressed by raw offsets, as in the original.

use super::simd::V;

pub(super) const STATE_LEN: usize = 0x25000;
pub(super) const X0: usize = 0;
pub(super) const X1: usize = 8;
pub(super) const INPOS: usize = 24;
pub(super) const BITBUF: usize = 40;
pub(super) const BITCNT: usize = 44;
pub(super) const PREV: usize = 1138;
pub(super) const HIST_IDX: usize = 1139;
pub(super) const HIST: usize = 1140;
pub(super) const COUNT: usize = 149128;
pub(super) const COUNT_BITS: usize = 149132;
pub(super) const BLOCK_LEN: usize = 149136;
pub(super) const TOTAL: usize = 149144;
pub(super) const BLOCK_END: usize = 149152;
pub(super) const MIN_LEN: usize = 149160;
pub(super) const CURSOR: usize = 149184;
pub(super) const OUTPOS: usize = 149208;

/// The decoder: the state block, the input and the output being filled.
pub(super) struct Snow<'a> {
    pub(super) m: Vec<u8>,
    pub(super) input: &'a [u8],
    pub(super) out: Vec<u8>,
}

impl<'a> Snow<'a> {
    pub(super) fn r8(&self, o: usize) -> u8 {
        self.m[o]
    }
    pub(super) fn r16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.m[o], self.m[o + 1]])
    }
    pub(super) fn r32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.m[o..o + 4].try_into().expect("4 bytes"))
    }
    pub(super) fn r64(&self, o: usize) -> u64 {
        u64::from_le_bytes(self.m[o..o + 8].try_into().expect("8 bytes"))
    }
    pub(super) fn w8(&mut self, o: usize, v: u8) {
        self.m[o] = v;
    }
    pub(super) fn w16(&mut self, o: usize, v: u16) {
        self.m[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub(super) fn w32(&mut self, o: usize, v: u32) {
        self.m[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub(super) fn w64(&mut self, o: usize, v: u64) {
        self.m[o..o + 8].copy_from_slice(&v.to_le_bytes());
    }
    pub(super) fn ldv(&self, o: usize) -> V {
        std::array::from_fn(|i| self.r16(o + 2 * i) as i16)
    }
    pub(super) fn stv(&mut self, o: usize, v: V) {
        for (i, x) in v.iter().enumerate() {
            self.w16(o + 2 * i, *x as u16);
        }
    }
    /// A table entry (u16) at byte offset `o`.
    pub(super) fn t(&self, o: usize) -> u16 {
        self.r16(o)
    }
}
