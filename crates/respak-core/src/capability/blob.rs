//! The [`Blob`] capability.

use std::ops::Range;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};

use crate::error::Result;
use crate::Cx;

/// How much [`read_all`] and the output writer ask for at a time.
pub const CHUNK: u64 = 4 << 20;

/// Bytes, read by range.
///
/// A capability is a lazy view over the format's native type, not a copy of it. A blob may keep
/// its bytes in memory, point at a file, or compute them when first asked (see
/// [`LazyBlob`](crate::value::LazyBlob)); a consumer cannot tell and must not care. Writing a
/// `Blob` value to the output is done by core itself, in chunks.
#[async_trait]
pub trait Blob: Send + Sync {
    /// The total length, if known. A computed blob does not know it until it has been produced.
    fn len(&self) -> Option<u64>;

    /// The bytes of `range`, cut at the end of the blob: a read at or past the end is empty, a
    /// read across it is short. A computed blob produces as much as `range` needs, so a read near
    /// the start is cheap and a read of the end runs everything.
    async fn read(&self, range: Range<u64>, cx: &Cx) -> Result<Bytes>;
}

/// The whole blob in memory. For small blobs; a large one is copied with [`Blob::read`] in chunks.
pub async fn read_all(blob: &dyn Blob, cx: &Cx) -> Result<Bytes> {
    let mut all = BytesMut::new();
    loop {
        let start = all.len() as u64;
        let chunk = blob.read(start..start + CHUNK, cx).await?;
        if chunk.is_empty() {
            return Ok(all.freeze());
        }
        all.extend_from_slice(&chunk);
    }
}
