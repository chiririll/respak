//! Bounds-checked little-endian reads from a byte slice.
//!
//! Parsers read untrusted files; every read here fails with [`ErrorKind::Corrupt`] (or returns
//! `None` from the `try_*` variants, for detection) instead of panicking or overflowing.
//!
//! [`Reader`] reads at absolute offsets, [`Cursor`] reads forward from a position. Both carry a
//! short `what` (usually the format id) that prefixes error messages.
//!
//! [`ErrorKind::Corrupt`]: crate::ErrorKind::Corrupt

use crate::error::{Error, Result};

macro_rules! numbers {
    ($($name:ident, $try_name:ident: $ty:ty;)*) => {
        impl<'a> Reader<'a> {
            $(
                #[doc = concat!("The little-endian `", stringify!($ty), "` at `at`.")]
                pub fn $name(&self, at: usize) -> Result<$ty> {
                    self.array(at).map(<$ty>::from_le_bytes)
                }

                #[doc = concat!("The little-endian `", stringify!($ty), "` at `at`, if it is in bounds.")]
                pub fn $try_name(&self, at: usize) -> Option<$ty> {
                    self.try_array(at).map(<$ty>::from_le_bytes)
                }
            )*
        }

        impl<'a> Cursor<'a> {
            $(
                #[doc = concat!("Reads a little-endian `", stringify!($ty), "` and moves past it.")]
                pub fn $name(&mut self) -> Result<$ty> {
                    self.array().map(<$ty>::from_le_bytes)
                }
            )*
        }
    };
}

numbers! {
    u8, try_u8: u8;
    u16, try_u16: u16;
    i16, try_i16: i16;
    u32, try_u32: u32;
    i32, try_i32: i32;
    u64, try_u64: u64;
    i64, try_i64: i64;
    f32, try_f32: f32;
}

/// Reads at absolute offsets into a block.
#[derive(Debug, Clone, Copy)]
pub struct Reader<'a> {
    buf: &'a [u8],
    what: &'static str,
}

impl<'a> Reader<'a> {
    /// A reader over `buf`; `what` prefixes error messages.
    pub fn new(what: &'static str, buf: &'a [u8]) -> Self {
        Self { buf, what }
    }

    /// The whole block.
    pub fn bytes(&self) -> &'a [u8] {
        self.buf
    }

    /// The block's length.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the block is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// `len` bytes at `at`, if they are in bounds.
    pub fn try_get(&self, at: usize, len: usize) -> Option<&'a [u8]> {
        self.buf.get(at..at.checked_add(len)?)
    }

    /// `len` bytes at `at`.
    pub fn get(&self, at: usize, len: usize) -> Result<&'a [u8]> {
        self.try_get(at, len).ok_or_else(|| {
            Error::corrupt(format!(
                "{}: {len} bytes at {at:#x} are outside a block of {} bytes",
                self.what,
                self.buf.len()
            ))
        })
    }

    fn array<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.get(at, N)?);
        Ok(out)
    }

    fn try_array<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.try_get(at, N)?);
        Some(out)
    }

    /// `N` consecutive little-endian `f32`s from `at`.
    pub fn floats<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32(at.saturating_add(4 * i))?;
        }
        Ok(out)
    }

    /// A NUL-terminated string at `at` that ends within `max` bytes.
    pub fn cstr(&self, at: usize, max: usize) -> Result<String> {
        let end = at.saturating_add(max).min(self.buf.len());
        let bytes = self.buf.get(at..end).ok_or_else(|| {
            Error::corrupt(format!(
                "{}: string at {at:#x} is outside its block",
                self.what
            ))
        })?;
        let nul = bytes.iter().position(|&b| b == 0).ok_or_else(|| {
            Error::corrupt(format!("{}: string at {at:#x} has no end", self.what))
        })?;
        Ok(String::from_utf8_lossy(&bytes[..nul]).into_owned())
    }

    /// A cursor over the same block, starting at `pos`.
    pub fn cursor(&self, pos: usize) -> Cursor<'a> {
        Cursor { reader: *self, pos }
    }
}

/// Reads forward through a block.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    reader: Reader<'a>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `buf`; `what` prefixes error messages.
    pub fn new(what: &'static str, buf: &'a [u8]) -> Self {
        Reader::new(what, buf).cursor(0)
    }

    /// The offset of the next read.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Moves to `pos`. A position past the end is reported by the next read.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    /// Bytes left after the current position.
    pub fn remaining(&self) -> usize {
        self.reader.len().saturating_sub(self.pos)
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.reader.get(self.pos, n)?;
        self.pos += n;
        Ok(s)
    }

    /// Moves past `n` bytes.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_at_offsets() {
        let r = Reader::new("t", &[1, 0, 2, 0, 0, 0, 0xff, 0xff]);
        assert_eq!(r.u16(0).unwrap(), 1);
        assert_eq!(r.u32(2).unwrap(), 2);
        assert_eq!(r.i16(6).unwrap(), -1);
        assert_eq!(r.try_u16(7), None);
        assert_eq!(r.try_u16(6), Some(0xffff));
    }

    #[test]
    fn out_of_bounds_and_overflow_are_corrupt_not_panics() {
        let r = Reader::new("t", &[0; 4]);
        assert!(r.u32(1).is_err());
        assert!(r.u8(4).is_err());
        assert!(r.u32(usize::MAX).is_err());
        assert!(r.try_u32(usize::MAX - 1).is_none());
        assert!(r.floats::<2>(usize::MAX - 2).is_err());
        let e = r.u64(0).unwrap_err().to_string();
        assert!(e.starts_with("corrupt data: t: 8 bytes at 0x0"), "{e}");
    }

    #[test]
    fn cstr_needs_a_nul_within_max() {
        let r = Reader::new("t", b"ab\0cd");
        assert_eq!(r.cstr(0, 8).unwrap(), "ab");
        assert!(r.cstr(3, 8).is_err());
        assert!(r.cstr(0, 2).is_err());
        assert!(r.cstr(9, 1).is_err());
    }

    #[test]
    fn cursor_reads_forward_and_stops_at_the_end() {
        let mut c = Cursor::new("t", &[1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 9]);
        assert_eq!(c.u32().unwrap(), 1);
        assert_eq!(c.u64().unwrap(), 2);
        assert_eq!(c.remaining(), 1);
        assert!(c.u16().is_err());
        assert_eq!(c.pos(), 12, "a failed read does not move the cursor");
        c.skip(1).unwrap();
        assert!(c.skip(1).is_err());
        c.seek(usize::MAX);
        assert!(c.u8().is_err());
    }
}
