//! `aseq`: the Respawn animation sequence asset, read from `rpak.asset`. Apex Legends, asset
//! versions 12.1 to 14 (seqdesc v18, animdesc v19.1, data in `asqd`).
//!
//! Ported from rsx `src/game/rtech/assets/animseq.{h,cpp}` (`AnimSeqAssetHeader_v8_t`,
//! `GetAnimSeqVersionFromAsset`, `LoadAnimSeqAsset`, `AnimSeq_ParseExtraData`),
//! `src/core/mdl/animdata.{h,cpp}` (seqdesc/animdesc parsing, `Stall_2` section lookup,
//! `ParseAnimation`, `ParseAnimDesc_Origin`) and `studio_r5_v16.h` (`mstudioseqdesc_v18_t`,
//! `mstudioanimdesc_v19_1_t`, `mstudioframemovement_t`, `FIX_OFFSET`). Offsets are in
//! `docs/formats/aseq.md`; they were computed by hand from the rsx structs.
//!
//! # Variants
//! `Version { major: asset version, minor }`: 12.0 and 12.1 are told apart by the pak's creation
//! time (a date-based guess of rsx). Only 12.1, 13 and 14 are read; 7 to 12.0, 9 and 15+ are
//! `UnsupportedVersion`. The bone flag width follows the version (4 bits for 12.1, 6 for 13 and 14).
//!
//! # What is read
//! The 64-byte asset header, then the seqdesc (label, flags, blend grid, bone weights) and the
//! animdesc table as bounded blocks. Per animdesc: name, fps, frame count, flags, the `asqd` guid,
//! the section table and the frame movement. The bones are not in the asset: the skeleton comes
//! from the `arig` / `mdl_` that lists the sequence ([`RigSelect`]), resolved when the sequence is
//! read. Frames are decoded on demand from the `asqd` (and `dataExtra` for external sections);
//! a decoded section is kept for the next frame. Integer frames only.
//!
//! Behaviour changes against rsx: bounded reads (`Corrupt`), a sequence without a rig is
//! `MissingDependency` (rsx skips it), datapoint animations (flag `0x200000`) are
//! `UnsupportedVersion` (rsx decodes them but calls them not shipping), the frame index is an
//! integer (rsx goes through a float cycle), an `asqd` that cannot be found is `MissingDependency`
//! for that animdesc only, an external section without `dataExtra` is `MissingDependency` (rsx
//! falls back to the stall frame).

use respak_core::Error;

mod asset;
mod desc;
mod handler;
mod header;
mod options;

pub use asset::AseqAsset;
pub use desc::{AnimDesc, FrameMovement, SectionData, SeqDesc, Weights};
pub use handler::AseqRead;
pub(crate) use header::flag_width;
pub use header::{variant, AseqHeader};
pub use options::RigSelect;

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("aseq", version)
}

const HEADER_LEN: usize = 64;
/// How much of `.mem` after the seqdesc is read as the description block: the furthest a
/// `FIX_OFFSET` offset reaches, and a little more for what it points at.
const SEQ_WINDOW: u64 = (1 << 20) + 0x1000;
/// The first read of a section; grown by 4 until the section fits.
const SECTION_WINDOW: u64 = 64 << 10;
/// A section bigger than this is not one.
const MAX_SECTION: u64 = 64 << 20;
/// Bytes kept after the last bone of a section: a value track is at most this far from its pointer.
const TRACK_REACH: usize = 0x2000;
const MAX_SECTIONS: usize = 1 << 16;
const MAX_AUTOLAYERS: usize = 64;
/// pak `created_time` (FILETIME) from which rsx calls an aseq of asset version 12 "12.1" ("09/05/2025").
const CREATED_12_1: u64 = 0x01DC_1DF8_05C2_8000;

/// `looping`.
const ANIM_LOOPING: i32 = 0x1;
/// `delta` (additive).
const ANIM_DELTA: i32 = 0x4;
/// `allzeros`: no real data.
const ANIM_ALLZEROS: i32 = 0x20;
/// The animdesc has animation data (seqdesc: the sequence has scale).
const ANIM_VALID: i32 = 0x20000;
/// The animdesc has frame movement.
const ANIM_FRAMEMOVEMENT: i32 = 0x40000;
/// Datapoint compression instead of RLE.
const ANIM_DATAPOINT: i32 = 0x200000;
