//! Capability traits: the views through which formats of different crates talk to each other.
//!
//! A capability is a lazy view over a format's native type; it never replaces it.

mod animation;
mod audio;
mod bitmap;
mod blob;
mod container;
mod mesh;
mod metadata;
mod skeleton;
mod table;

pub use animation::Animation;
pub use audio::Audio;
pub use bitmap::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
pub use blob::Blob;
pub use container::Container;
pub use mesh::Mesh;
pub use metadata::{Dependency, Metadata};
pub use skeleton::Skeleton;
pub use table::Table;

/// Names a capability trait object type (`dyn Bitmap`, ...) so registration can say
/// `provides::<dyn Bitmap>()` and `from_cap::<dyn Bitmap>(..)`. Handlers get the capability
/// from the value itself (`value.as_bitmap()`).
pub trait Cap {
    /// Unique name; identifies the capability in the registry, e.g. `Bitmap`.
    const NAME: &'static str;
}

// Compile-time check that every capability is object-safe.
#[allow(dead_code, clippy::too_many_arguments)]
fn _object_safe(
    _: &dyn Blob,
    _: &dyn Container,
    _: &dyn Metadata,
    _: &dyn Bitmap,
    _: &dyn Mesh,
    _: &dyn Skeleton,
    _: &dyn Animation,
    _: &dyn Table,
    _: &dyn Audio,
) {
}

impl Cap for dyn Bitmap {
    const NAME: &'static str = "Bitmap";
}

impl Cap for dyn Blob {
    const NAME: &'static str = "Blob";
}
