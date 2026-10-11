//! The [`Blob`] capability.

use bytes::Bytes;

/// In-memory bytes.
///
/// A capability is a lazy view over the format's native type, not a copy of it. Writing a
/// `Blob` value to the output is done by core itself.
pub trait Blob: Send + Sync {
    /// The bytes. Cheap to clone.
    fn bytes(&self) -> &Bytes;
}
