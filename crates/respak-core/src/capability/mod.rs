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
pub use bitmap::Bitmap;
pub use blob::Blob;
pub use container::Container;
pub use mesh::Mesh;
pub use metadata::Metadata;
pub use skeleton::Skeleton;
pub use table::Table;
