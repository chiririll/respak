//! Values: everything is a value tagged with a format.

use std::any::{Any, TypeId};
use std::fmt::Debug;
use std::sync::Arc;

use crate::capability::{Animation, Audio, Bitmap, Blob, Container, Mesh, Metadata, Skeleton, Table, Video};

mod core_formats;
mod format;
mod key;
mod lazy;

pub use core_formats::{BlobData, BundleData, CompressedData, FileData, BLOB, BUNDLE, COMPRESSED, FILE};
pub use format::{FormatId, Version};
pub use key::ValueKey;
pub use lazy::{LazyBlob, LazyInit, LazySource};

/// A shared value. Results are shared between concurrent requests, hence `Arc`.
pub type Value = Arc<dyn Data>;

/// A value tagged with a format.
///
/// A format keeps its full native type and implements the capability traits that apply,
/// exposing them through the `as_*` accessors (default: unsupported). Capabilities are lazy
/// views over the native type (see [`crate::capability`]).
pub trait Data: Any + Send + Sync + Debug {
    /// Format and detected version, e.g. `txtr@10`.
    fn format(&self) -> &FormatId;
    /// Identity for dedup and cycle detection.
    fn key(&self) -> &ValueKey;
    /// Approximate memory use in bytes, for the later in-memory cache.
    fn approx_size(&self) -> usize {
        0
    }
    /// This value as bytes.
    fn as_blob(&self) -> Option<&dyn Blob> {
        None
    }
    /// This value as a container.
    fn as_container(&self) -> Option<&dyn Container> {
        None
    }
    /// This value's name, guid and dependencies.
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        None
    }
    /// This value as a bitmap.
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        None
    }
    /// This value as a mesh.
    fn as_mesh(&self) -> Option<&dyn Mesh> {
        None
    }
    /// This value as a skeleton.
    fn as_skeleton(&self) -> Option<&dyn Skeleton> {
        None
    }
    /// This value as an animation.
    fn as_animation(&self) -> Option<&dyn Animation> {
        None
    }
    /// This value as a table.
    fn as_table(&self) -> Option<&dyn Table> {
        None
    }
    /// This value as audio.
    fn as_audio(&self) -> Option<&dyn Audio> {
        None
    }
    /// This value as video.
    fn as_video(&self) -> Option<&dyn Video> {
        None
    }
    /// A plugin-defined capability, looked up by the `TypeId` of its trait object type.
    fn capability(&self, _id: TypeId) -> Option<&dyn Any> {
        None
    }
}

impl dyn Data {
    /// Whether the concrete type is `T`.
    pub fn is<T: Data>(&self) -> bool {
        (self as &dyn Any).is::<T>()
    }

    /// Borrows the concrete type, if it is `T`.
    pub fn downcast_ref<T: Data>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}

/// Turns a [`Value`] into its concrete type, or gives the value back if it is not a `T`.
pub fn downcast<T: Data>(value: Value) -> Result<Arc<T>, Value> {
    if value.is::<T>() {
        let any: Arc<dyn Any + Send + Sync> = value;
        Ok(any.downcast::<T>().expect("type checked above"))
    } else {
        Err(value)
    }
}
