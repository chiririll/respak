//! `aseq`: the Respawn animation sequence asset, read from `rpak.asset`. Apex Legends, asset
//! versions 12.1 to 14 (seqdesc v18, animdesc v19.1, data in `asqd`).
//!
//! Ported from rsx `src/game/rtech/assets/animseq.{h,cpp}` (`AnimSeqAssetHeader_v8_t`,
//! `GetAnimSeqVersionFromAsset`, `LoadAnimSeqAsset`, `AnimSeq_ParseExtraData`),
//! `src/core/mdl/animdata.{h,cpp}` (seqdesc/animdesc parsing, `Stall_2` section lookup,
//! `ParseAnimation`, `ParseAnimDesc_Origin`) and `studio_r5_v16.h` (`mstudioseqdesc_v18_t`,
//! `mstudioanimdesc_v19_1_t`, `mstudioframemovement_t`, `FIX_OFFSET`). Offsets are in
//! `todo/formats/aseq.md`; they were computed by hand from the rsx structs.
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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Animation, AnimationInfo, Bone, BoneSample, BoneTrack, Dependency, Metadata, Track};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey, Version,
};
use serde::{Deserialize, Serialize};

use crate::anim_codec::{self, BoneFrame, FlagWidth, RleHeader, SectionIndex, BONE_POS, BONE_ROT, BONE_SCALE};
use crate::arig::{explicit_rig, owner_of};
use crate::asqd::AsqdAsset;
use crate::matl_json::{hex, locate, pak_of};
use crate::rmdl::{fix_offset, read_guids, B};
use crate::rpak::{RpakAsset, MEM};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

fn unsupported(version: impl Into<String>) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: "aseq".into(), version: version.into() })
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

/// Which skeleton an `aseq` is bound to (the `rig` option): `"auto"` or a guid / path.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum RigSelect {
    /// The owner found by the reverse index: the first `arig`, else the first `mdl_`, that lists
    /// the sequence. Written `"auto"`.
    #[default]
    Auto,
    /// An `arig` / `mdl_` named by guid (`0x...`) or by path (hashed with `string_to_guid`).
    Explicit(String),
}

impl From<String> for RigSelect {
    fn from(s: String) -> Self {
        if s == "auto" {
            Self::Auto
        } else {
            Self::Explicit(s)
        }
    }
}

impl From<RigSelect> for String {
    fn from(s: RigSelect) -> Self {
        match s {
            RigSelect::Auto => "auto".into(),
            RigSelect::Explicit(name) => name,
        }
    }
}

/// The variant of a sequence. 12.0 (before the pak creation time cut-off), 7 to 11, 9 and 15+ are
/// `UnsupportedVersion`.
pub fn variant(asset_version: u32, created_time: u64) -> Result<Version> {
    match asset_version {
        12 if created_time >= CREATED_12_1 => Ok(Version::with_minor(12, 1)),
        12 => Err(unsupported("12.0")),
        13 | 14 => Ok(Version::new(asset_version)),
        other => Err(unsupported(other.to_string())),
    }
}

/// The bone flag width of a variant (4 bits for 12.1, 6 for 13 and 14).
pub(crate) fn flag_width(version: Version) -> FlagWidth {
    if version.major >= 13 {
        FlagWidth::Six
    } else {
        FlagWidth::Four
    }
}

/// The decoded 64-byte asset header (`AnimSeqAssetHeader_v8_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AseqHeader {
    /// The variant.
    pub version: Version,
    /// Where the seqdesc, animdescs and section tables are in `.mem`.
    pub data_offset: u64,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
    /// Model guids (`.mem` offset, count).
    pub models: (Option<u64>, u32),
    /// Effect guids (`.mem` offset). The header stores no count for them (open, see `aseq.md`), so
    /// they are not listed as dependencies until a real file shows the layout.
    pub effects: Option<u64>,
    /// Settings guids (`.mem` offset, count).
    pub settings: (Option<u64>, u32),
    /// The external sections (`dataExtra`): `.mem` offset (`None` if null) and size in bytes.
    pub extra: (Option<u64>, u32),
}

impl AseqHeader {
    /// Parses a relocated 64-byte header of a supported variant.
    pub fn parse(version: Version, header: &[u8]) -> Result<Self> {
        if !(12..=14).contains(&version.major) {
            return Err(unsupported(version.to_string()));
        }
        if header.len() != HEADER_LEN {
            return Err(corrupt(format!("aseq header is {} bytes, expected {HEADER_LEN}", header.len())));
        }
        let h = B(header);
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = h.u64(at)?;
            Ok((v != u64::MAX && v != 0).then_some(v))
        };
        Ok(Self {
            version,
            data_offset: ptr(0x00)?.ok_or_else(|| corrupt("aseq has no data pointer"))?,
            name_offset: ptr(0x08)?,
            models: (ptr(0x20)?, u32::from(h.u16(0x18)?)),
            effects: ptr(0x28)?,
            settings: (ptr(0x30)?, u32::from(h.u16(0x1A)?)),
            extra: (ptr(0x38)?, h.u32(0x1C)?),
        })
    }
}

/// Where a bone weight list comes from (`weightlistindex` of the seqdesc).
#[derive(Debug, Clone, PartialEq)]
pub enum Weights {
    /// The special value 1: every weight 0.
    Zero,
    /// The special value 3: every weight 1.
    One,
    /// One weight per bone.
    List(Vec<f32>),
}

/// The fields of `mstudioseqdesc_v18_t` the conversion uses.
#[derive(Debug, Clone, PartialEq)]
pub struct SeqDesc {
    /// The label (`szlabelindex`).
    pub label: String,
    /// `flags`; `0x20000` means the sequence has scale.
    pub flags: i32,
    /// The blend grid `groupsize[2]`; the blend count is their product.
    pub group_size: [u8; 2],
    /// Offsets of the animdescs, relative to the seqdesc (`FIX_OFFSET`ed).
    pub anim_offsets: Vec<u32>,
    /// The per-bone weights.
    pub weights: Weights,
    /// Guids of the sequences layered on this one (`mstudioautolayer_v8_t`, 0x20 bytes each).
    pub autolayers: Vec<u64>,
}

impl SeqDesc {
    /// Parses the seqdesc at `at` in `block` for a rig of `bone_count` bones.
    pub fn parse(block: &[u8], at: usize, bone_count: usize) -> Result<Self> {
        let b = B(block);
        let label = b.cstr(b.fixed(at, at)?, 256)?;
        let flags = b.i32(at + 0x04)?;
        let group_size = [b.u8(at + 0x54)?, b.u8(at + 0x55)?];
        let blends = usize::from(group_size[0]) * usize::from(group_size[1]);
        if blends == 0 {
            return Err(corrupt(format!("aseq `{label}` has no animations (group size {group_size:?})")));
        }
        let table = b.fixed(at, at + 0x2A)?;
        let anim_offsets = (0..blends)
            .map(|i| Ok(fix_offset(b.u16(table + 2 * i)?)))
            .collect::<Result<Vec<u32>>>()?;
        let weights = match b.u16(at + 0x52)? {
            0 | 3 => Weights::One,
            1 => Weights::Zero,
            _ => {
                let list = b.fixed(at, at + 0x52)?;
                Weights::List((0..bone_count).map(|i| b.f32(list + 4 * i)).collect::<Result<_>>()?)
            }
        };
        // Autolayers are only listed as dependencies, so a table that does not fit is not an error.
        let count = usize::from(b.u16(at + 0x4E)?).min(MAX_AUTOLAYERS);
        let layers = b.fixed(at, at + 0x50)?;
        let autolayers = (0..count).map_while(|i| b.u64(layers + 0x20 * i).ok()).collect();
        Ok(Self { label, flags, group_size, anim_offsets, weights, autolayers })
    }
}

/// How a section of an animdesc is stored (`Stall_2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionData {
    /// At this byte offset in the animdesc's `asqd` blob.
    Asqd(u32),
    /// At this byte offset in the sequence's `dataExtra`.
    Extra(u32),
}

/// The root motion of an animation (`mstudioframemovement_t`): offsets of tracks for x, y, z and
/// yaw per section of `section_frames` frames.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMovement {
    /// Scale of each track (x, y, z, yaw in degrees).
    pub scale: [f32; 4],
    /// Frames per section, 0 if the animation is one section.
    pub section_frames: i32,
    /// Per section the block offset of each track, `None` if the axis has none.
    pub sections: Vec<[Option<usize>; 4]>,
}

impl FrameMovement {
    fn parse(b: &B, at: usize, frame_count: u32) -> Result<Self> {
        let scale = b.floats::<4>(at)?;
        let section_frames = b.i32(at + 0x10)?;
        let count = if section_frames > 0 { (frame_count.max(1) as usize - 1) / section_frames as usize + 1 } else { 1 };
        if count > MAX_SECTIONS {
            return Err(corrupt(format!("frame movement with {count} sections")));
        }
        let sections = (0..count)
            .map(|i| {
                let section = b.fixed(at, at + 0x14 + 2 * i)?;
                let mut tracks = [None; 4];
                for (k, track) in tracks.iter_mut().enumerate() {
                    let off = usize::from(b.u16(section + 2 * k)?);
                    *track = (off != 0).then_some(section + off);
                }
                Ok(tracks)
            })
            .collect::<Result<_>>()?;
        Ok(Self { scale, section_frames, sections })
    }

    /// The accumulated movement on `frame`: position and yaw in degrees.
    fn at(&self, block: &[u8], frame: u32) -> Result<([f32; 3], f32)> {
        let (section, local) = if self.section_frames > 0 {
            let sf = self.section_frames as u32;
            ((frame / sf) as usize, frame % sf)
        } else {
            (0, frame)
        };
        let tracks = self.sections.get(section).ok_or_else(|| corrupt(format!("frame movement has no section for frame {frame}")))?;
        let mut v = [0.0; 4];
        for (k, track) in tracks.iter().enumerate() {
            if let Some(at) = track {
                v[k] = anim_codec::decode_track(block, *at, local, self.scale[k])?;
            }
        }
        Ok(([v[0], v[1], v[2]], v[3]))
    }
}

/// The fields of `mstudioanimdesc_v19_1_t` the conversion uses.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimDesc {
    /// The name (`sznameindex`), empty if none.
    pub name: String,
    /// Frames per second; outside `0..=2048` is `Corrupt`.
    pub fps: f32,
    /// `flags` (`0x1` looping, `0x4` delta, `0x20` all zeros, `0x20000` has data, `0x40000` frame
    /// movement, `0x200000` datapoint).
    pub flags: i32,
    /// Number of frames; outside `0..=0x20000` is `Corrupt`.
    pub frame_count: u32,
    /// Guid of the `asqd` with the data, 0 if none.
    pub asqd: u64,
    /// Frames of the first, "stall" section.
    pub stall_frames: u16,
    /// Frames of every later section, 0 if the animation is one section.
    pub section_frames: u16,
    /// The section table in order.
    pub sections: Vec<SectionData>,
    /// The root motion, if the animation has any.
    pub movement: Option<FrameMovement>,
}

impl AnimDesc {
    /// Parses the animdesc at `at` in `block` (offsets inside it are relative to `at`).
    pub fn parse(block: &[u8], at: usize) -> Result<Self> {
        let b = B(block);
        let fps = b.f32(at)?;
        let flags = b.i32(at + 0x04)?;
        let frames = b.i32(at + 0x08)?;
        // rsx's `ANIMDESC_SANITY_CHECK`
        if !(0.0..=2048.0).contains(&fps) {
            return Err(corrupt(format!("animation with {fps} frames per second")));
        }
        let frame_count = u32::try_from(frames).ok().filter(|&n| n <= 0x20000).ok_or_else(|| corrupt(format!("animation with {frames} frames")))?;
        let name = if b.u16(at + 0x0C)? == 0 { String::new() } else { b.cstr(b.fixed(at, at + 0x0C)?, 256)? };
        let asqd = b.u64(at + 0x18)?;
        let stall_frames = b.u16(at + 0x2C)?;
        let section_frames = b.u16(at + 0x2E)?;

        let mut sections = Vec::new();
        if section_frames != 0 && b.u16(at + 0x2A)? != 0 {
            let count = (frame_count as usize).saturating_sub(usize::from(stall_frames) + 1) / usize::from(section_frames) + 2;
            let table = b.fixed(at, at + 0x2A)?;
            // A table that ends early stays short; `locate` reports the missing entry when it is needed.
            for entry in (0..count.min(MAX_SECTIONS)).map_while(|i| b.i32(table + 4 * i).ok()) {
                sections.push(if entry < 0 { SectionData::Extra(!entry as u32) } else { SectionData::Asqd(entry as u32) });
            }
        }
        let movement = if flags & ANIM_FRAMEMOVEMENT != 0 && b.u16(at + 0x0E)? != 0 {
            Some(FrameMovement::parse(&b, b.fixed(at, at + 0x0E)?, frame_count)?)
        } else {
            None
        };
        Ok(Self { name, fps, flags, frame_count, asqd, stall_frames, section_frames, sections, movement })
    }

    /// Where the data of `frame` is and the section-local frame (`Stall_2`). Frames before `stall_frames` are held by section 0, which is the
    /// start of the `asqd` blob (`Asqd(0)`).
    pub fn locate(&self, frame: u32) -> Result<(SectionData, u32)> {
        if frame >= self.frame_count {
            return Err(Error::new(ErrorKind::NotFound { path: format!("frame {frame}") }));
        }
        let (stall, sf) = (u32::from(self.stall_frames), u32::from(self.section_frames));
        if sf == 0 || self.frame_count <= 1 || frame < stall {
            return Ok((SectionData::Asqd(0), frame));
        }
        // rsx's `Stall_2` has no case for the last frame: it is found like any other. The trailing
        // section that RLE animations carry (counted in the section count) is never read.
        let base = (frame - stall) / sf;
        let (index, local) = (base, frame - stall - base * sf);
        self.sections
            .get(index as usize)
            .map(|&s| (s, local))
            .ok_or_else(|| corrupt(format!("animation `{}`: no section {index} for frame {frame} (the table has {})", self.name, self.sections.len())))
    }

    /// Whether the animation has RLE data to decode.
    fn has_data(&self) -> bool {
        self.flags & ANIM_VALID != 0 && self.flags & ANIM_ALLZEROS == 0 && self.asqd != 0
    }
}

/// The bytes of `dataExtra`, read in ranges.
#[derive(Debug)]
struct Extra {
    volume: Operator,
    /// `.mem` or the sequence's starpak entry.
    path: String,
    base: u64,
    len: u64,
}

impl Extra {
    async fn read_upto(&self, offset: u64, max: u64) -> Result<Bytes> {
        let end = offset.saturating_add(max).min(self.len);
        if offset >= end {
            return Ok(Bytes::new());
        }
        Ok(self.volume.read_with(&self.path).range(self.base + offset..self.base + end).await?.to_bytes())
    }
}

/// One decoded section: its bytes and where each bone is in them.
#[derive(Debug)]
struct Section {
    block: Bytes,
    index: SectionIndex,
}

/// The channels of a bone on a frame; the flag says the channel is a track (not a constant).
struct Channels {
    translation: ([f32; 3], bool),
    rotation: ([f32; 4], bool),
    scale: ([f32; 3], bool),
}

/// A sequence: `Data` + `Animation` + `Metadata`. One animation per blend.
#[derive(Debug)]
pub struct AseqAsset {
    format: FormatId,
    key: ValueKey,
    header: AseqHeader,
    seq: SeqDesc,
    anims: Vec<AnimDesc>,
    name: Option<String>,
    guid: u64,
    /// The bones of the bound skeleton, copied from the rig.
    bones: Vec<Bone>,
    /// The bind rotation of each bone as Euler angles (for the axes a rotation track lacks).
    default_euler: Vec<[f32; 3]>,
    /// Guid of the rig the bones came from.
    rig: u64,
    /// Guids to report as dependencies: asqd per animdesc, autolayer aseq, models, settings.
    dependencies: Vec<u64>,
    origin: String,
    /// The description block (frame movement tracks are in it).
    block: Bytes,
    width: FlagWidth,
    extra: Option<Extra>,
    asqds: Mutex<HashMap<u64, Value>>,
    sections: Mutex<HashMap<(u32, SectionData), Arc<Section>>>,
}

impl AseqAsset {
    /// The parsed header.
    pub fn header(&self) -> &AseqHeader {
        &self.header
    }

    /// The sequence description.
    pub fn seq(&self) -> &SeqDesc {
        &self.seq
    }

    /// The animdescs, one per blend.
    pub fn anims(&self) -> &[AnimDesc] {
        &self.anims
    }

    /// Guid of the rig the animation is bound to.
    pub fn rig(&self) -> u64 {
        self.rig
    }

    fn anim(&self, blend: u32) -> Result<&AnimDesc> {
        self.anims.get(blend as usize).ok_or_else(|| Error::new(ErrorKind::NotFound { path: format!("blend {blend}") }))
    }

    fn bone(&self, bone: u32) -> Result<&Bone> {
        self.bones.get(bone as usize).ok_or_else(|| Error::new(ErrorKind::NotFound { path: format!("bone {bone}") }))
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| hex(self.guid))
    }

    /// The `asqd` with the guid `guid`, found through the mediator (and kept).
    async fn asqd(&self, guid: u64, cx: &Cx) -> Result<Value> {
        if let Some(v) = self.asqds.lock().expect("lock").get(&guid) {
            return Ok(v.clone());
        }
        let asset = locate(guid, "asqd", &self.origin, cx).await?;
        let value = cx.mediator.identify_as(asset, &FormatId::new("asqd"), cx).await?;
        self.asqds.lock().expect("lock").insert(guid, value.clone());
        Ok(value)
    }

    /// Up to `want` bytes at the start of `section` of the animation `anim`.
    async fn window(&self, anim: &AnimDesc, section: SectionData, want: u64, cx: &Cx) -> Result<Bytes> {
        match section {
            SectionData::Asqd(offset) => {
                let value = self.asqd(anim.asqd, cx).await?;
                let asqd = value
                    .downcast_ref::<AsqdAsset>()
                    .ok_or_else(|| corrupt(format!("{} is not an asqd", hex(anim.asqd))))?;
                asqd.read_upto(u64::from(offset), want).await
            }
            SectionData::Extra(offset) => {
                let extra = self.extra.as_ref().ok_or_else(|| {
                    Error::new(ErrorKind::MissingDependency {
                        item: self.label(),
                        file: "(the sequence has an external section but no dataExtra)".into(),
                    })
                })?;
                extra.read_upto(u64::from(offset), want).await
            }
        }
    }

    /// The decoded section `section` of `blend`, read once.
    async fn section(&self, blend: u32, section: SectionData, cx: &Cx) -> Result<Arc<Section>> {
        if let Some(s) = self.sections.lock().expect("lock").get(&(blend, section)) {
            return Ok(s.clone());
        }
        cx.check_cancelled()?;
        let anim = self.anim(blend)?;
        let mut want = SECTION_WINDOW;
        let decoded = loop {
            let bytes = self.window(anim, section, want, cx).await?;
            match anim_codec::index_section(&bytes, self.bones.len(), self.width)? {
                Some(index) => {
                    let keep = bytes.len().min(index.extent + TRACK_REACH);
                    break Section { block: bytes.slice(..keep), index };
                }
                None if (bytes.len() as u64) < want || want >= MAX_SECTION => {
                    return Err(corrupt(format!(
                        "{}: section {section:?} of animation {blend} runs past the end of its data",
                        self.label()
                    )));
                }
                None => want *= 4,
            }
        };
        let decoded = Arc::new(decoded);
        self.sections.lock().expect("lock").insert((blend, section), decoded.clone());
        Ok(decoded)
    }

    /// The rest values of a bone: the bind pose, or the identity for an additive animation.
    fn rest(&self, bone: &Bone, additive: bool) -> Channels {
        if additive {
            Channels { translation: ([0.0; 3], false), rotation: ([0.0, 0.0, 0.0, 1.0], false), scale: ([1.0; 3], false) }
        } else {
            Channels { translation: (bone.position, false), rotation: (bone.rotation, false), scale: (bone.scale, false) }
        }
    }

    /// Whether the channels of `bone` in `anim` are all the rest pose.
    fn is_rest(anim: &AnimDesc, bone: u32) -> bool {
        !(anim.has_data() || bone == 0 && anim.movement.is_some())
    }

    /// The channels of `bone` on `frame` of `blend`.
    async fn channels(&self, blend: u32, bone: u32, frame: u32, cx: &Cx) -> Result<Channels> {
        let anim = self.anim(blend)?;
        let rest_bone = self.bone(bone)?;
        let additive = anim.flags & ANIM_DELTA != 0;
        let mut out = self.rest(rest_bone, additive);
        if anim.has_data() {
            if anim.flags & ANIM_DATAPOINT != 0 {
                return Err(unsupported("datapoint compression"));
            }
            let (place, local) = anim.locate(frame)?;
            let section = self.section(blend, place, cx).await?;
            let slot = bone as usize;
            if let Some(header_at) = section.index.headers[slot] {
                let header = RleHeader::parse(&section.block, header_at)?;
                let flags = section.index.flags[slot] & (BONE_POS | BONE_ROT | BONE_SCALE);
                let data = anim_codec::decode_bone(&section.block, header_at + 2, header, flags, local, self.default_euler[slot])?;
                out = self.merge(rest_bone, additive, data);
            }
        }
        if self.seq.flags & ANIM_VALID == 0 {
            out.scale = ([1.0; 3], false);
        }
        if bone == 0 {
            if let Some(movement) = &anim.movement {
                let (position, yaw) = movement.at(&self.block, frame)?;
                out.translation.0 = add(out.translation.0, position);
                if yaw != 0.0 {
                    let half = yaw.to_radians() * 0.5;
                    out.rotation.0 = anim_codec::quat_mul([0.0, 0.0, half.sin(), half.cos()], out.rotation.0);
                }
                out.translation.1 = true;
                out.rotation.1 |= yaw != 0.0;
            }
        }
        Ok(out)
    }

    /// Combines decoded data with the bind pose (a track adds to a position / scale, a constant replaces it).
    fn merge(&self, bone: &Bone, additive: bool, data: BoneFrame) -> Channels {
        let mut out = self.rest(bone, additive);
        if let Some((v, constant)) = data.translation {
            out.translation = if constant { (v, false) } else { (add(out.translation.0, v), true) };
        }
        if let Some((q, constant)) = data.rotation {
            out.rotation = (q, !constant);
        }
        if let Some((v, constant)) = data.scale {
            out.scale = if constant { (v, false) } else { (add(out.scale.0, v), true) };
        }
        out
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A channel over all frames: a constant if no frame was a track and all values are equal.
fn collapse<T: PartialEq + Copy>(frames: Vec<(T, bool)>) -> Track<T> {
    match frames.first() {
        Some(&(first, false)) if frames.iter().all(|&(v, tracked)| !tracked && v == first) => Track::Constant(first),
        _ => Track::Dense(frames.into_iter().map(|(v, _)| v).collect()),
    }
}

#[async_trait]
impl Animation for AseqAsset {
    fn blend_count(&self) -> u32 {
        self.anims.len() as u32
    }

    /// Name, fps, frame count, looping (`0x1`), additive (`0x4`) and scale (seqdesc `0x20000`).
    fn info(&self, blend: u32) -> Result<AnimationInfo> {
        let anim = self.anim(blend)?;
        let name = if anim.name.is_empty() { format!("{}_{blend}", self.seq.label) } else { anim.name.clone() };
        Ok(AnimationInfo {
            name,
            fps: anim.fps,
            frame_count: anim.frame_count,
            looping: anim.flags & ANIM_LOOPING != 0,
            additive: anim.flags & ANIM_DELTA != 0,
            has_scale: self.seq.flags & ANIM_VALID != 0,
        })
    }

    fn bones(&self) -> &[Bone] {
        &self.bones
    }

    fn bone_weights(&self) -> Vec<f32> {
        match &self.seq.weights {
            Weights::Zero => vec![0.0; self.bones.len()],
            Weights::One => Vec::new(),
            Weights::List(list) => list.clone(),
        }
    }

    /// Every frame of the bone: channels without data are `Constant` (bind pose, or identity for
    /// additive); bone 0 includes the frame movement.
    async fn track(&self, blend: u32, bone: u32, cx: &Cx) -> Result<BoneTrack> {
        let anim = self.anim(blend)?;
        let rest_bone = self.bone(bone)?;
        if anim.frame_count == 0 || Self::is_rest(anim, bone) {
            let rest = self.rest(rest_bone, anim.flags & ANIM_DELTA != 0);
            return Ok(BoneTrack {
                translation: Track::Constant(rest.translation.0),
                rotation: Track::Constant(rest.rotation.0),
                scale: Track::Constant(if self.seq.flags & ANIM_VALID != 0 { rest.scale.0 } else { [1.0; 3] }),
            });
        }
        let n = anim.frame_count as usize;
        let (mut t, mut r, mut s) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
        for frame in 0..anim.frame_count {
            if frame % 256 == 0 {
                cx.check_cancelled()?;
            }
            let c = self.channels(blend, bone, frame, cx).await?;
            t.push(c.translation);
            r.push(c.rotation);
            s.push(c.scale);
        }
        Ok(BoneTrack { translation: collapse(t), rotation: collapse(r), scale: collapse(s) })
    }

    async fn sample(&self, blend: u32, bone: u32, frame: u32, cx: &Cx) -> Result<BoneSample> {
        let anim = self.anim(blend)?;
        if frame >= anim.frame_count {
            return Err(Error::new(ErrorKind::NotFound { path: format!("frame {frame}") }));
        }
        let c = self.channels(blend, bone, frame, cx).await?;
        Ok(BoneSample { translation: c.translation.0, rotation: c.rotation.0, scale: c.scale.0 })
    }
}

impl Metadata for AseqAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("aseq")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.dependencies.iter().map(|&guid| Dependency { guid, name: None }).collect()
    }
}

impl Data for AseqAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_animation(&self) -> Option<&dyn Animation> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// Read edge `rpak.asset -> aseq`.
pub struct AseqRead {
    rig: RigSelect,
}

impl AseqRead {
    /// A reader that binds sequences to the skeleton `rig` says. An `arig` it finds is read by the
    /// registered `arig` edge, with its `studio` setting.
    pub fn new(rig: RigSelect) -> Self {
        Self { rig }
    }
}

#[async_trait]
impl Detect for AseqRead {
    /// The asset kind is `aseq` = 10, with the asset version as the version; the variant is worked
    /// out when the sequence is read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"aseq").then(|| Detected { score: 10, version: Some(Version::new(asset.entry.version)) }))
    }
}

#[async_trait]
impl ReadHandler for AseqRead {
    /// Parses the header, the seqdesc and the animdescs, resolves the rig (`MissingDependency` if
    /// there is none) and checks that the bone weight list fits it.
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source
            .downcast_ref::<RpakAsset>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "aseq must be read from an rpak.asset".into() }))?;
        if asset.entry.kind != *b"aseq" {
            return Err(Error::new(ErrorKind::Custom { source: None, message: "the asset is not an aseq".into() }));
        }
        let version = variant(asset.entry.version, asset.pak_created_time)?;
        let header = AseqHeader::parse(version, &asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        if header.data_offset >= mem_len {
            return Err(corrupt("aseq: the data pointer is outside the pak's data"));
        }
        let origin = pak_of(source.key());

        let rig = match &self.rig {
            RigSelect::Auto => owner_of(asset.entry.guid, &origin, cx).await?,
            RigSelect::Explicit(name) => explicit_rig(name, &origin, cx).await?,
        };
        let skeleton = rig.as_skeleton().ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "the rig has no skeleton".into() }))?;
        let bones = skeleton.bones().to_vec();
        let rig_guid = rig.as_metadata().and_then(|m| m.guid()).unwrap_or(0);

        let block = asset.read_mem(header.data_offset, SEQ_WINDOW.min(mem_len - header.data_offset)).await?;
        let seq = SeqDesc::parse(&block, 0, bones.len())?;
        let anims = seq
            .anim_offsets
            .iter()
            .map(|&off| AnimDesc::parse(&block, off as usize))
            .collect::<Result<Vec<_>>>()?;

        let name = match header.name_offset {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let models = read_guids(asset, header.models.0, header.models.1).await?;
        let settings = read_guids(asset, header.settings.0, header.settings.1).await?;
        let mut dependencies: Vec<u64> = Vec::new();
        for &guid in anims.iter().map(|a| &a.asqd).chain(&seq.autolayers).chain(&models).chain(&settings) {
            if guid != 0 && !dependencies.contains(&guid) {
                dependencies.push(guid);
            }
        }

        let extra = match header.extra {
            (Some(at), len) if len > 0 => Some(Extra { volume: asset.volume.clone(), path: MEM.to_owned(), base: at, len: u64::from(len) }),
            (None, len) if len > 0 => {
                asset.stream.clone().map(|path| Extra { volume: asset.volume.clone(), path, base: 0, len: u64::from(len) })
            }
            _ => None,
        };
        let default_euler = bones.iter().map(|b| anim_codec::quat_to_euler(b.rotation)).collect();
        Ok(Arc::new(AseqAsset {
            format: FormatId::new("aseq").with_version(version),
            key: source.key().clone(),
            width: flag_width(version),
            header,
            seq,
            anims,
            name,
            guid: asset.entry.guid,
            bones,
            default_euler,
            rig: rig_guid,
            dependencies,
            origin,
            block,
            extra,
            asqds: Mutex::new(HashMap::new()),
            sections: Mutex::new(HashMap::new()),
        }))
    }
}
