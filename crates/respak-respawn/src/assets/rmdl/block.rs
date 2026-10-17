//! The bounds-checked block reader shared by the rmdl family and the `FIX_OFFSET` fields.

use respak_core::reader::Reader;
use respak_core::{Error, Result};

/// A bounds-checked reader over an rmdl-family block (also used by the other animation
/// formats, which share its layout).
pub(crate) fn reader(buf: &[u8]) -> Reader<'_> {
    Reader::new("rmdl", buf)
}

/// Reads of `FIX_OFFSET` fields.
pub(crate) trait FixedOffset {
    /// `base + FIX_OFFSET(u16 at field)`, bounds are checked by the reader that uses it.
    fn fixed(&self, base: usize, field: usize) -> Result<usize>;
}

impl FixedOffset for Reader<'_> {
    fn fixed(&self, base: usize, field: usize) -> Result<usize> {
        usize::try_from(fix_offset(self.u16(field)?))
            .ok()
            .and_then(|off| base.checked_add(off))
            .ok_or_else(|| Error::corrupt(format!("rmdl: offset at {field:#x} overflows")))
    }
}

/// `FIX_OFFSET`: bit 0 set means `(v & 0xFFFE) * 16`, otherwise `v`.
pub(crate) fn fix_offset(v: u16) -> u32 {
    u32::from(v & 0xFFFE) << (4 * (v & 1))
}
