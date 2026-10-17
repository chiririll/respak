//! Builders for synthetic Respawn files (paks, starpaks, patches, Oodle streams and the asset
//! blocks inside them), for tests. No game files.

mod aseq_asset;
mod builder;
mod bytes;
mod dxbc;
mod edit;
mod oodle;
mod pak;
mod raw;
mod rmdl_asset;
mod starpak;
mod texture;

pub use builder::{DtblCell, PakBuilder};
pub use dxbc::dxbc_blob;
pub use edit::EditScript;
pub use oodle::{oodle_pak, oodle_stored};
pub use pak::{build_pak, build_pak_with, set_created_time, Options, TestAsset};
pub use raw::{build_patch_file, build_raw, build_raw_with, PatchSpec, RawAsset};
pub use starpak::build_starpak;
pub use texture::{build_tex_pak, build_uimg_pak, SpriteSpec, TexSpec, UimgSpec};

/// Builders for synthetic animation assets (`aseq`, `asqd`, `arig`).
pub mod aseq {
    pub use crate::aseq_asset::*;
}

/// Builders for synthetic `mdl_` assets (studio block and vertex data).
pub mod rmdl {
    pub use crate::rmdl_asset::*;
}
