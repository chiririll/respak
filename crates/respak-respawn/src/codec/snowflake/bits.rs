//! The input side: 32/64-bit reads, rANS refill and the bit reader.

use super::state::*;

impl<'a> Snow<'a> {
    /// The 32 bits of the input at `pos` (zero past the end).
    pub(super) fn in32(&self, pos: u64) -> u64 {
        let p = pos as usize;
        let mut b = [0u8; 4];
        if pos < self.input.len() as u64 {
            let n = (self.input.len() - p).min(4);
            b[..n].copy_from_slice(&self.input[p..p + n]);
        }
        u64::from(u32::from_le_bytes(b))
    }

    /// The 64 bits of the input at `pos` (zero past the end).
    pub(super) fn in64(&self, pos: u64) -> u64 {
        self.in32(pos) | (self.in32(pos + 4) << 32)
    }

    /// Refills a rANS state that dropped below 2^32 from the input.
    pub(super) fn renorm(&mut self, v: u64) -> u64 {
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
    pub(super) fn bits(&mut self, n: u32) -> u64 {
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
            (have + 64 - n, (w >> (n - (have + 32))) as u32)
        };
        self.w32(BITCNT, left);
        self.w32(BITBUF, rest);
        v & mask
    }
}
