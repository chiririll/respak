//! `cast`: Cast models (<https://github.com/dtzxporter/cast>), written from any `Mesh` (+ `Skeleton`).
//! Not readable.
//!
//! Ported from rsx `src/core/mdl/cast.{h,cpp}` (`CastExporter`) and `modeldata.cpp`
//! (`ExportModelCAST`, `ParseMaterialIntoCast`).
//!
//! # File
//! `u32 "cast"`, `u32 version = 1`, `u32 rootNodes`, `u32 flags = 0`; then the nodes. A node is a
//! 24-byte header (`u32 id` FourCC, `u32 nodeSize` including the header, its properties and its
//! children, `u64 hash`, `u32 propertyCount`, `u32 childCount`), its properties, its children.
//! A property is `u16 id` (type), `u16 nameSize`, `u32 arrayLength`, the name (no NUL), the data.
//! The type ids are the bytes `b h i l f d s v2 v3 v4` (a one- or two-character code, the second
//! byte 0 for the single characters).
//!
//! # Output
//! One file per (LOD, model): `root` → `modl` → `skel` (`bone`s), `matl`s (each with `file`
//! children for its textures), `mesh`es. A mesh with no geometry in the request gives a
//! skeleton-only file. Node hashes are FNV-1a of the name (rsx uses its own string hash; a hash
//! only has to be unique inside the file).

mod anim;
mod model;
mod node;
mod options;
#[cfg(test)]
mod tests;

pub use anim::CastAnimationEncode;
pub use model::CastEncode;
pub use node::CastWriter;
pub use options::{AllBlends, AllLods, BlendSelect, CastOptions, LodSelect, RootFixup};

use respak_core::{Error, ErrorKind};

fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom {
        source: None,
        message: message.into(),
    })
}

/// The last path component of `name` without its extension.
fn stem_of(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file_safe(last.rsplit_once('.').map_or(last, |(s, _)| s))
}
