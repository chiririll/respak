//! The bit-level decoder loop.

use super::lut::{
    ext16_base, DIST_LEN_OFF, DIST_SYM_OFF, ESCAPE_OFF, EXT_BASE8_OFF, EXT_BITS16_OFF,
    EXT_BITS8_OFF, LEN_OFF, LUT,
};
use super::{Prefix, RtechError};

#[inline]
pub(super) fn shr(x: u64, n: u64) -> u64 {
    if n >= 64 {
        0
    } else {
        x >> n
    }
}

#[inline]
pub(super) fn shl(x: u64, n: u64) -> u64 {
    if n >= 64 {
        0
    } else {
        x << n
    }
}

#[inline]
pub(super) fn low_mask(bits: u64) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
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

/// The fixed-size bit header at the start of the stream.
pub(super) struct Header {
    /// Total decompressed size, including the header region.
    pub(super) size: u64,
    /// Input window bits (64 for a single chunk).
    pub(super) in_bits: u64,
    /// Output chunk bits.
    pub(super) out_bits: u64,
    /// Bit offset of the payload from the start of the stream.
    pub(super) payload_bit: u32,
}

pub(super) struct Decoder<'a> {
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

impl<'a> Decoder<'a> {
    /// Sets up the decoder for the stream whose header is `h` (`InitPakDecoder`) and allocates
    /// the output: all of it, or for a cuttable stream only the chunk holding the first `want` bytes.
    pub(super) fn new(
        file: &'a [u8],
        partial: bool,
        header_len: usize,
        h: &Header,
        want: u64,
    ) -> Result<Self, RtechError> {
        let size = h.size;
        let hl = header_len as u64;
        let inv_in = low_mask(h.in_bits);
        let inv_out = low_mask(h.out_bits);

        // The byte holding the first payload bit.
        let byte_pos = header_len + (h.payload_bit / 8) as usize;
        let rem = u64::from(h.payload_bit % 8);
        let mut cur = [0u8; 8];
        let Some(s) = file.get(byte_pos..) else {
            return Err(RtechError::Truncated("stream header"));
        };
        let k = s.len().min(8);
        cur[..k].copy_from_slice(&s[..k]);
        let bits = u64::from_le_bytes(cur) >> rem;
        let mut rd = byte_pos as u64 + 8;

        let (hdr_off, first) = if inv_in == u64::MAX {
            (0u64, file.len() as u64)
        } else {
            let ho = (h.in_bits >> 3) + 1;
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
        let alloc = if want >= size - hl || !cuttable {
            size
        } else {
            dec.stop_at.div_ceil(chunk).saturating_mul(chunk).min(size)
        };
        let total = usize::try_from(alloc).map_err(|_| RtechError::TooLarge(size))?;
        dec.out
            .try_reserve_exact(total)
            .map_err(|_| RtechError::TooLarge(size))?;
        dec.out.resize(total, 0);
        Ok(dec)
    }

    /// The decoded output after [`run`](Self::run), without the header region, and whether it is complete.
    pub(super) fn finish(mut self, header_len: usize) -> Prefix {
        let complete = self.pos == self.decomp_size;
        self.out.truncate(self.pos as usize);
        self.out.drain(..header_len);
        Prefix {
            body: self.out,
            complete,
        }
    }
}

impl Decoder<'_> {
    /// Loads 8 bytes at `pos`, zero padded past the end (the original over-reads there).
    fn load64(&self, pos: u64) -> Result<u64, RtechError> {
        let len = self.data.len() as u64;
        if self.partial && pos.saturating_add(8) > len {
            return Err(RtechError::NeedInput {
                at_least: pos.saturating_add(8),
            });
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
            RtechError::NeedInput {
                at_least: self.buffer_needed.saturating_add(16),
            }
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
                    None if self.partial => {
                        return Err(RtechError::NeedInput {
                            at_least: self.rd + 1,
                        })
                    }
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

    /// Tops the bit buffer up from the byte read pointer.
    fn refill_bits(&mut self) -> Result<(), RtechError> {
        if self.nb > 64 {
            return Err(RtechError::Corrupt("bit buffer overrun"));
        }
        let w = self.load64(self.rd)?;
        let v = shl(w, 64 - self.nb) | self.bits;
        self.rd += self.nb >> 3;
        self.nb &= 7;
        self.bits = shr(u64::MAX, self.nb) & v;
        Ok(())
    }

    /// Copies a literal run (negative op `op`) from the input to the output. `state` is the
    /// state the op was looked up in (0 after a match, 1 after a literal run).
    fn literal_run(&mut self, op: i8, state: usize) -> Result<(), RtechError> {
        let mut cnt = u64::from(op.unsigned_abs());
        if cnt == u64::from(LUT[ESCAPE_OFF + state]) {
            if (!self.rd & self.inv_in) < 0xF
                || (self.inv_out & !self.pos) < 15
                || self.decomp_size.saturating_sub(self.pos) < 0x10
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
        Ok(())
    }

    /// Decodes the distance of a match and copies it (positive op `op`: length, or 17 for a long match).
    fn copy_match(&mut self, op: i8) -> Result<(), RtechError> {
        let v21 = self.bits & 0xF;
        let (t, u) = if v21 == 15 {
            (6, (self.bits >> 4) & 3)
        } else {
            (4, 0)
        };
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
            if dist < 8 {
                ext + 4
            } else {
                ext + 17
            }
        } else {
            u64::from(op as u8)
        };
        if dist > self.pos {
            return Err(RtechError::Corrupt("match distance before start of output"));
        }
        if self.pos + len > self.decomp_size {
            return Err(RtechError::Corrupt("match past end of output"));
        }
        lz_copy(
            &mut self.out,
            self.pos as usize,
            dist as usize,
            len as usize,
        );
        self.pos += len;
        Ok(())
    }

    /// Whether decoding is done (the output is full or a wanted chunk boundary was reached).
    fn finished(&self) -> Result<bool, RtechError> {
        if self.pos == self.decomp_size {
            return Ok(true);
        }
        if self.pos >= self.stop_at && self.pos == self.decomp_stream {
            return Ok(true);
        }
        if self.pos > self.decomp_stream {
            return Err(RtechError::Corrupt("op crosses output chunk boundary"));
        }
        Ok(false)
    }

    /// Start of a new output chunk: one pad bit, then the chunk size field.
    fn start_chunk(&mut self) -> Result<(), RtechError> {
        let size = self.decomp_size;
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
        Ok(())
    }

    /// The read pointer reached the end of the current limit: move to the next chunk and/or
    /// input window. Returns the new limit.
    fn advance_window(&mut self) -> Result<u64, RtechError> {
        if self.pos == self.decomp_stream {
            self.start_chunk()?;
        }
        let mut limit = self.q70;
        if self.rd >= limit {
            self.rd = !self.inv_in & self.rd.wrapping_add(7);
            limit = limit.wrapping_add(self.inv_in).wrapping_add(1);
            self.q70 = limit;
        }
        Ok(limit.min(self.comp_stream))
    }

    pub(super) fn run(&mut self) -> Result<(), RtechError> {
        let mut state = 0usize;
        let mut limit = self.q70.min(self.comp_stream);
        let mut refill = self.nb != 0;
        if self.data.len() as u64 > u64::MAX / 2 {
            return Err(RtechError::Corrupt("input too large"));
        }
        loop {
            if refill {
                self.refill_bits()?;
            }
            refill = true;

            let idx = (self.bits & 0xFF) as usize + state * 256;
            let l = u64::from(LUT[LEN_OFF + idx]);
            self.nb += l;
            self.bits >>= l;
            let op = LUT[idx] as i8;

            if op < 0 {
                self.literal_run(op, state)?;
                state = 1;
            } else {
                state = 0;
                self.copy_match(op)?;
            }

            if self.finished()? {
                return Ok(());
            }
            if self.rd >= limit {
                limit = self.advance_window()?;
            }
        }
    }
}
