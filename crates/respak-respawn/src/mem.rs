//! Clamped reads of a volume file.

use bytes::Bytes;
use opendal::Operator;
use respak_core::Result;

/// Up to `max` bytes of `path` at `offset`, never past `len`: fewer when the data ends first, none
/// when `offset` is at or past the end.
pub(crate) async fn read_clamped(
    volume: &Operator,
    path: &str,
    offset: u64,
    max: u64,
    len: u64,
) -> Result<Bytes> {
    let end = offset.saturating_add(max).min(len);
    if offset >= end {
        return Ok(Bytes::new());
    }
    Ok(volume.read_with(path).range(offset..end).await?.to_bytes())
}
