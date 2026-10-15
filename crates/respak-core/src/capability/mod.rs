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
mod video;

pub use animation::Animation;
pub use audio::{Audio, AudioChunk, AudioTrackInfo};
pub use bitmap::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
pub use blob::{read_all, Blob, CHUNK};
pub use container::Container;
pub use mesh::{MaterialTexture, Mesh, MeshInfo, MeshMaterial, MeshModel, MeshPart, ModelInfo};
pub use metadata::{Dependency, Metadata};
pub use skeleton::{Bone, Skeleton};
pub use table::Table;
pub use video::{FrameFormat, Video, VideoFrame, VideoInfo};

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
    _: &dyn Video,
) {
}

impl Cap for dyn Bitmap {
    const NAME: &'static str = "Bitmap";
}

impl Cap for dyn Blob {
    const NAME: &'static str = "Blob";
}

impl Cap for dyn Container {
    const NAME: &'static str = "Container";
}

impl Cap for dyn Metadata {
    const NAME: &'static str = "Metadata";
}

impl Cap for dyn Mesh {
    const NAME: &'static str = "Mesh";
}

impl Cap for dyn Skeleton {
    const NAME: &'static str = "Skeleton";
}

impl Cap for dyn Audio {
    const NAME: &'static str = "Audio";
}

impl Cap for dyn Video {
    const NAME: &'static str = "Video";
}
