//! `rmdl`: the Respawn model asset (`mdl_`), read from `rpak.asset`. Apex Legends, asset
//! versions 16 to 20.
//!
//! Ported from rsx `src/game/rtech/assets/model.{h,cpp}` (`ModelAssetHeader_v16_t`,
//! `ModelAssetCPU_v16_t`, `GetModelVersionFromAsset`), `src/game/rtech/utils/studio/studio_r5_v16.h`
//! and `studio.h` (studiohdr, bones, bodyparts, models, meshes, `vg::rev4`, `FIX_OFFSET`),
//! `src/core/math/compressedvector.h` (`Vector64`, `Normal32`) and `src/core/mdl/modeldata.cpp`
//! (`ParseModelVertexData_v16`, `ParseVertexFromVG`). Field offsets are in
//! `docs/formats/rmdl.md`; they were computed by hand from the rsx structs.
//!
//! # Variants
//! `Version { major: asset version, minor }`; the studiohdr layout follows from both:
//! 16 → `studiohdr_v16`; 17, 18, 19.0, 19.1 → `studiohdr_v17`; 19.2, 19.3, 20 → `studiohdr_v19_2`.
//! 19.x is told apart by the pak's creation time and `sourceFilenameOffset`, which are
//! date-based guesses of rsx. Other majors are `UnsupportedVersion`.
//!
//! # What is read
//! The asset header and CPU data, then the studiohdr as one bounded block: bones (split form up
//! to 18, linear-bone form from 19), body parts → models → meshes, the material table and skins.
//! The vertex data ("VG") is read per LOD group from the mandatory starpak or the asset's own
//! pages when a model is asked for, and decompressed per group (none / RTech / Snowflake / Oodle).
//!
//! Behaviour changes against rsx: every read is bounded (`Corrupt`), a bone map shorter than a
//! bone index is `Corrupt` (rsx reads out of bounds), a vertex layout that does not add up to the
//! stride is `UnsupportedVersion` (rsx only asserts), `Vector48` positions are unsupported (rsx
//! returns zeros), a failed Oodle decode is an error (rsx treats it as uncompressed).

mod json;

pub use json::{RmdlJsonEncode, RmdlJsonOptions};

mod asset;
mod block;
mod handler;
mod header;
mod studio;
#[cfg(test)]
mod tests;
mod vg;

pub use asset::RmdlAsset;
pub(crate) use block::{fix_offset, reader, FixedOffset};
pub(crate) use handler::read_guids;
pub use handler::RmdlRead;
pub use header::{variant, RmdlHeader, StudioLayout};
pub(crate) use studio::parse_bones;
pub use studio::Studio;

use respak_core::Error;

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("rmdl", version)
}
