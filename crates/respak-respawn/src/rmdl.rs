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

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{
    Bone, Dependency, MaterialTexture, Mesh, MeshInfo, MeshMaterial, MeshModel, MeshPart, Metadata,
    ModelInfo, Skeleton,
};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};
use serde_json::{json, Value as Json};

use crate::matl::MatlAsset;
use crate::matl_json::{hex, locate, pak_of, shader_slots};
use crate::rpak::{RpakAsset, MEM};
use crate::{oodle, rtech, snowflake};

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("rmdl", version)
}

const ASSET_HEADER_LEN: usize = 96;
const CPU_LEN: usize = 16;
/// A studio block bigger than this is not a model.
const MAX_STUDIO_LEN: u64 = 64 << 20;
const MAX_BONES: usize = 2048;
const MAX_LODS: usize = 8;
const MAX_GUIDS: usize = 4096;
const MAX_ITEMS: usize = 65_535;
/// `STUDIOHDR_FLAGS_USES_EXTRA_BONE_WEIGHTS`.
const FLAG_EXTRA_WEIGHTS: i32 = 0x4000;
/// pak `created_time` (FILETIME) from which rsx calls a 19.x model 19.3 ("07/29/2026").
const CREATED_19_3: u64 = 0x01DD_1EED_32D6_C000;
/// pak `created_time` from which rsx calls a 19.x model 19.1 ("09/05/2025").
const CREATED_19_1: u64 = 0x01DC_1DF8_05C2_8000;

/// Which studiohdr layout a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioLayout {
    /// `studiohdr_v16_t`, 224 bytes (asset version 16).
    V16,
    /// `studiohdr_v17_t`, 228 bytes (versions 17, 18, 19.0, 19.1).
    V17,
    /// `studiohdr_v19_2_t`, 244 bytes (versions 19.2, 19.3, 20).
    V19_2,
}

impl StudioLayout {
    /// The layout of a variant, `UnsupportedVersion` outside 16 to 20.
    pub fn for_version(v: Version) -> Result<Self> {
        match (v.major, v.minor) {
            (16, 0) => Ok(Self::V16),
            (17 | 18, 0) | (19, 0 | 1) => Ok(Self::V17),
            (19, 2 | 3) | (20, 0) => Ok(Self::V19_2),
            _ => Err(unsupported(v.to_string())),
        }
    }

    /// How far the fields after `boneStateCount` moved relative to `studiohdr_v17_t`.
    fn shift(self) -> usize {
        if self == Self::V19_2 {
            16
        } else {
            0
        }
    }
}

/// The variant of a model: the minor of 19.x comes from the pak's creation time and the studiohdr's
/// `sourceFilenameOffset` (the u16 at studiohdr + 216, `None` if the block is too short).
pub fn variant(
    asset_version: u32,
    created_time: u64,
    source_filename_offset: Option<u16>,
) -> Result<Version> {
    match asset_version {
        16..=18 | 20 => Ok(Version::new(asset_version)),
        19 => {
            let minor = if created_time >= CREATED_19_3 {
                3
            } else if source_filename_offset == Some(244) {
                2
            } else if created_time >= CREATED_19_1 {
                1
            } else {
                0
            };
            Ok(Version::with_minor(19, minor))
        }
        other => Err(unsupported(other.to_string())),
    }
}

/// Bounds-checked little-endian reads from a block.
pub(crate) struct B<'a>(pub(crate) &'a [u8]);

impl<'a> B<'a> {
    pub(crate) fn get(&self, at: usize, len: usize) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| {
                Error::corrupt(format!(
                    "rmdl: {len} bytes at {at:#x} are outside a block of {} bytes",
                    self.0.len()
                ))
            })
    }
    pub(crate) fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.get(at, 1)?[0])
    }
    pub(crate) fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.get(at, 2)?.try_into().expect("2 bytes"),
        ))
    }
    pub(crate) fn i16(&self, at: usize) -> Result<i16> {
        Ok(self.u16(at)? as i16)
    }
    pub(crate) fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.get(at, 4)?.try_into().expect("4 bytes"),
        ))
    }
    pub(crate) fn i32(&self, at: usize) -> Result<i32> {
        Ok(self.u32(at)? as i32)
    }
    pub(crate) fn u64(&self, at: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.get(at, 8)?.try_into().expect("8 bytes"),
        ))
    }
    pub(crate) fn f32(&self, at: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(at)?))
    }
    pub(crate) fn floats<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32(at + 4 * i)?;
        }
        Ok(out)
    }
    /// A NUL-terminated string that ends within `max` bytes.
    pub(crate) fn cstr(&self, at: usize, max: usize) -> Result<String> {
        let end = at.saturating_add(max).min(self.0.len());
        let bytes = self.0.get(at..end).ok_or_else(|| {
            Error::corrupt(format!("rmdl: string at {at:#x} is outside its block"))
        })?;
        let nul = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::corrupt(format!("rmdl: string at {at:#x} has no end")))?;
        Ok(String::from_utf8_lossy(&bytes[..nul]).into_owned())
    }
    /// `base + FIX_OFFSET(u16 at field)`, bounds are checked by the reader that uses it.
    pub(crate) fn fixed(&self, base: usize, field: usize) -> Result<usize> {
        Ok(base + fix_offset(self.u16(field)?) as usize)
    }
}

/// The decoded asset header and CPU data.
#[derive(Debug, Clone, PartialEq)]
pub struct RmdlHeader {
    /// The variant.
    pub version: Version,
    /// The studiohdr layout it uses.
    pub layout: StudioLayout,
    /// Where the studiohdr sits in `.mem`.
    pub studio_offset: u64,
    /// Bytes of the studio blob (`ModelAssetCPU_v16_t::dataSizeModel`).
    pub studio_len: u64,
    /// Bytes of the VG after baking (may be rounded up).
    pub streaming_size: u32,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
    /// The inline VG (`staticStreamingData`) as a `.mem` offset, `None` if null.
    pub static_data: Option<u64>,
    /// Bounding box (min, max).
    pub bounds: ([f32; 3], [f32; 3]),
    /// Where the animation rig guids are (`.mem` offset) and how many.
    pub anim_rigs: (Option<u64>, u32),
    /// Where the animation sequence guids are (`.mem` offset) and how many.
    pub anim_seqs: (Option<u64>, u32),
}

impl RmdlHeader {
    /// Parses a relocated 96-byte asset header (`asset_version` 16 to 20) and the 16-byte CPU
    /// data. `created_time` is the pak header's creation time and `source_filename_offset` the
    /// studiohdr's (both only matter for 19.x, see [`variant`]).
    pub fn parse(
        asset_version: u32,
        header: &[u8],
        cpu: &[u8],
        created_time: u64,
        source_filename_offset: Option<u16>,
    ) -> Result<Self> {
        let version = variant(asset_version, created_time, source_filename_offset)?;
        let layout = StudioLayout::for_version(version)?;
        if header.len() != ASSET_HEADER_LEN {
            return Err(Error::corrupt(format!(
                "rmdl header is {} bytes, expected {ASSET_HEADER_LEN}",
                header.len()
            )));
        }
        if cpu.len() != CPU_LEN {
            return Err(Error::corrupt(format!(
                "rmdl CPU data is {} bytes, expected {CPU_LEN}",
                cpu.len()
            )));
        }
        let (h, c) = (B(header), B(cpu));
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = h.u64(at)?;
            Ok((v != u64::MAX && v != 0).then_some(v))
        };
        let studio_offset =
            ptr(0x00)?.ok_or_else(|| Error::corrupt("rmdl has no studio data pointer"))?;
        let studio_len = u64::from(
            u32::try_from(c.i32(12)?).map_err(|_| Error::corrupt("rmdl: negative studio size"))?,
        );
        if studio_len == 0 || studio_len > MAX_STUDIO_LEN {
            return Err(Error::corrupt(format!(
                "rmdl studio block of {studio_len} bytes is not usable"
            )));
        }
        Ok(Self {
            version,
            layout,
            studio_offset,
            studio_len,
            streaming_size: h.u32(0x2C)?,
            name_offset: ptr(0x08)?,
            static_data: ptr(0x18)?,
            bounds: (h.floats(0x30)?, h.floats(0x3C)?),
            anim_rigs: (ptr(0x20)?, h.u32(0x28)?),
            anim_seqs: (ptr(0x50)?, u32::from(h.u16(0x4A)?)),
        })
    }
}

/// One hardware vertex group (`studio_hw_groupdata_v16_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    /// Offset of the group in the VG blob.
    pub data_offset: u32,
    /// Stored size.
    pub size_compressed: u32,
    /// Size after decompression.
    pub size_decompressed: u32,
    /// 0 none, 1 RTech, 2 Snowflake, 3 Oodle.
    pub compression: u8,
    /// Index of the group's first LOD header.
    pub lod_index: u8,
    /// LOD headers in the group.
    pub lod_count: u8,
    /// Bit `n` set: LOD `n` is in this group.
    pub lod_map: u8,
}

/// One mesh of a studio model (`mstudiomesh_v16_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudioMesh {
    /// Index into a skin family's row.
    pub material: u16,
    /// Index into the VG LOD's mesh array.
    pub mesh_id: u16,
}

/// A model of a body part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioModel {
    /// The base meshes (blend-shape meshes are not read).
    pub meshes: Vec<StudioMesh>,
}

/// A body part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bodypart {
    /// Its name.
    pub name: String,
    /// Its models.
    pub models: Vec<StudioModel>,
}

/// The studiohdr tables a mesh export needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Studio {
    /// The layout parsed.
    pub layout: StudioLayout,
    /// `studiohdr.flags`.
    pub flags: i32,
    /// The model's name.
    pub name: String,
    /// The bones.
    pub bones: Vec<Bone>,
    /// The body parts.
    pub bodyparts: Vec<Bodypart>,
    /// Material guids (`textureindex`).
    pub materials: Vec<u64>,
    /// `skinref[family][material]`, indices into `materials`.
    pub skins: Vec<Vec<u16>>,
    /// LOD switch distances.
    pub lod_switch: Vec<f32>,
    /// The VG groups.
    pub groups: Vec<Group>,
    /// `lod_slots[lod]`: the group holding LOD `lod` and the LOD's index inside it.
    pub lod_slots: Vec<Option<(usize, u32)>>,
    /// Maps a vertex's bone index to a skeleton bone; empty means identity.
    pub bone_map: Vec<u16>,
}

/// The bones of a studio block (`major` 16 to 20): the split form up to 18, the linear-bone form
/// from 19. Shared with `arig`, whose block is a cut-down studiohdr.
pub(crate) fn parse_bones(b: &B, major: u32, layout: StudioLayout) -> Result<Vec<Bone>> {
    let shift = layout.shift();
    let bone_count = usize::from(b.u16(0x74)?);
    if bone_count > MAX_BONES {
        return Err(Error::corrupt(format!("rmdl: {bone_count} bones")));
    }
    let (hdr_base, data_base) = (b.fixed(0, 0x76)?, b.fixed(0, 0x78)?);
    let linear = if major >= 19 {
        let lb = b.fixed(0, 0xCA + shift)?;
        if lb == 0 {
            return Err(Error::corrupt(
                "rmdl: v19 model without a linear bone block",
            ));
        }
        Some(lb)
    } else {
        None
    };
    let mut bones = Vec::with_capacity(bone_count);
    for i in 0..bone_count {
        let h = hdr_base + i * 12;
        let bone_name = b.cstr(b.fixed(h, h + 10)?, 256)?;
        let (parent, position, rotation, scale) = match linear {
            None => {
                let d = data_base + i * 128;
                (
                    b.i16(d + 116)?,
                    b.floats::<3>(d + 64)?,
                    b.floats::<4>(d + 76)?,
                    b.floats::<3>(d + 104)?,
                )
            }
            Some(lb) => {
                let d = data_base + i * 16;
                let at = |field: usize, stride: usize| -> Result<usize> {
                    Ok(b.fixed(lb, lb + field)? + i * stride)
                };
                (
                    b.i16(d)?,
                    b.floats::<3>(at(6, 12)?)?,
                    b.floats::<4>(at(8, 16)?)?,
                    b.floats::<3>(at(16, 12)?)?,
                )
            }
        };
        let parent = match parent {
            -1 => None,
            p if p >= 0 && (p as usize) < i => Some(p as u32),
            p => {
                return Err(Error::corrupt(format!(
                    "rmdl: bone {i} ({bone_name}) has parent {p}"
                )))
            }
        };
        bones.push(Bone {
            name: bone_name,
            parent,
            position,
            rotation,
            scale,
        });
    }
    Ok(bones)
}

impl Studio {
    /// Parses a studio block of `major` version (16 to 20) with the given layout.
    pub fn parse(major: u32, layout: StudioLayout, blob: &[u8]) -> Result<Self> {
        let b = B(blob);
        let shift = layout.shift();
        let flags = b.i32(0)?;
        let name = b.cstr(0x0A, 33)?;

        let bones = parse_bones(&b, major, layout)?;

        // Materials and skins.
        let tex_count = usize::from(b.u16(0x90)?);
        let tex_base = b.fixed(0, 0x92)?;
        let materials = (0..tex_count)
            .map(|i| b.u64(tex_base + i * 8))
            .collect::<Result<Vec<_>>>()?;
        let families = usize::from(b.u16(0x96)?);
        let skin_base = b.fixed(0, 0x98)?;
        let mut skins = Vec::with_capacity(families.max(1));
        for f in 0..families {
            skins.push(
                (0..tex_count)
                    .map(|t| b.u16(skin_base + (f * tex_count + t) * 2))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        if skins.is_empty() {
            skins.push((0..tex_count as u16).collect());
        }

        // Body parts, models, meshes.
        let bp_count = usize::from(b.u16(0x9A)?);
        let bp_base = b.fixed(0, 0x9C)?;
        let mut bodyparts = Vec::with_capacity(bp_count);
        for i in 0..bp_count {
            let bp = bp_base + i * 16;
            let bp_name = b.cstr(b.fixed(bp, bp)?, 256)?;
            let model_count = usize::try_from(b.i32(bp + 8)?)
                .map_err(|_| Error::corrupt("rmdl: negative model count"))?;
            if model_count > MAX_ITEMS {
                return Err(Error::corrupt(format!(
                    "rmdl: body part {bp_name} has {model_count} models"
                )));
            }
            let models_base = b.fixed(bp, bp + 2)?;
            let mut models = Vec::with_capacity(model_count);
            for j in 0..model_count {
                let m = models_base + j * 10;
                let mesh_count = usize::from(b.u16(m + 4)?);
                let mesh_base = b.fixed(m, m + 8)?;
                let meshes = (0..mesh_count)
                    .map(|k| {
                        let mp = mesh_base + k * 20;
                        Ok(StudioMesh {
                            material: b.u16(mp)?,
                            mesh_id: b.u16(mp + 2)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                models.push(StudioModel { meshes });
            }
            bodyparts.push(Bodypart {
                name: bp_name,
                models,
            });
        }

        // LODs and groups.
        let lod_count = usize::from(b.u16(0xBA + shift)?);
        if lod_count > MAX_LODS {
            return Err(Error::corrupt(format!("rmdl: {lod_count} LODs")));
        }
        let lod_base = b.fixed(0xB8 + shift, 0xB8 + shift)?;
        let lod_switch = (0..lod_count)
            .map(|i| b.f32(lod_base + i * 4))
            .collect::<Result<Vec<_>>>()?;
        let group_count = usize::from(b.u16(0xB6 + shift)?);
        let group_base = b.fixed(0xB4 + shift, 0xB4 + shift)?;
        let groups = (0..group_count)
            .map(|i| {
                let g = group_base + i * 16;
                Ok(Group {
                    data_offset: b.u32(g)?,
                    size_compressed: b.u32(g + 4)?,
                    size_decompressed: b.u32(g + 8)?,
                    compression: b.u8(g + 12)?,
                    lod_index: b.u8(g + 13)?,
                    lod_count: b.u8(g + 14)?,
                    lod_map: b.u8(g + 15)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut lod_slots = vec![None; lod_count];
        for (gi, g) in groups.iter().enumerate() {
            let mut idx = 0u32;
            for (level, slot) in lod_slots.iter_mut().enumerate() {
                if idx == u32::from(g.lod_count) {
                    break;
                }
                if g.lod_map & (1 << level) != 0 {
                    slot.get_or_insert((gi, idx));
                    idx += 1;
                }
            }
        }

        // Bone remap.
        let state_count = usize::from(b.u16(0xB2)?);
        let state_base = b.fixed(0xB0, 0xB0)?;
        let bone_map = (0..state_count)
            .map(|i| {
                if shift > 0 {
                    b.u16(state_base + i * 2)
                } else {
                    Ok(u16::from(b.u8(state_base + i)?))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            layout,
            flags,
            name,
            bones,
            bodyparts,
            materials,
            skins,
            lod_switch,
            groups,
            lod_slots,
            bone_map,
        })
    }

    /// The material guid mesh `m` uses in `skin`.
    fn material_of(&self, skin: u32, m: &StudioMesh) -> Result<u64> {
        let row = self.skins.get(skin as usize).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: format!("skin {skin}"),
            })
        })?;
        let index = *row.get(usize::from(m.material)).ok_or_else(|| {
            Error::corrupt(format!(
                "rmdl: material index {} outside the skin row",
                m.material
            ))
        })?;
        self.materials
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| {
                Error::corrupt(format!(
                    "rmdl: skin refers to material {index} of {}",
                    self.materials.len()
                ))
            })
    }
}

/// A mesh header of the VG (`vg::rev4::MeshHeader_t`) with the data ranges resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VgMesh {
    flags: u64,
    vert_count: usize,
    stride: usize,
    verts: usize,
    indices: usize,
    index_count: usize,
    extra: usize,
    extra_len: usize,
}

/// The mesh headers of LOD `lod_in_group` of a decompressed rev4 group.
fn vg_meshes(group: &[u8], lod_in_group: u32) -> Result<Vec<VgMesh>> {
    let b = B(group);
    let lod = 4 + b.u32(4)? as usize + lod_in_group as usize * 8;
    let mesh_count = usize::from(b.u8(lod)?);
    let first = lod + 4 + b.u32(lod + 4)? as usize;
    (0..mesh_count)
        .map(|i| {
            let m = first + i * 48;
            let packed = b.u32(m + 20)?;
            Ok(VgMesh {
                flags: b.u64(m)?,
                vert_count: b.u32(m + 8)? as usize,
                stride: usize::from(b.u16(m + 12)?),
                verts: m + 24 + b.u32(m + 24)? as usize,
                indices: m + 16 + b.u32(m + 16)? as usize,
                index_count: (packed & 0x0FFF_FFFF) as usize,
                extra: m + 32 + b.u32(m + 32)? as usize,
                extra_len: b.u32(m + 36)? as usize,
            })
        })
        .collect()
}

/// How a vertex is laid out, from the mesh flags.
#[derive(Debug, PartialEq, Eq)]
struct VertexLayout {
    /// 1: `Vec3` f32, 2: `Vector64`.
    position: u8,
    blend: bool,
    color: bool,
    uv_layers: usize,
}

const VERT_COLOR: u64 = 0x10;
const VERT_BLENDINDICES: u64 = 0x1000;
const VERT_BLENDWEIGHTS_UNPACKED: u64 = 0x2000;
const VERT_BLENDWEIGHTS_PACKED: u64 = 0x4000;

/// The layout of a vertex with `flags` that is `stride` bytes long. Anything that does not add
/// up to the stride is a layout we do not know.
fn vertex_layout(flags: u64, stride: usize) -> Result<VertexLayout> {
    let position = (flags & 3) as u8;
    let pos_len = match position {
        1 => 12,
        2 => 8,
        3 => return Err(unsupported("vertex positions in 48 bit form")),
        _ => return Err(unsupported("a vertex without positions")),
    };
    // `VERT_UNK` adds no bytes (rsx: "flag gets ignored"); the stride check below guards the guess.
    if flags & VERT_BLENDWEIGHTS_UNPACKED != 0 {
        return Err(unsupported(format!("vertex flags {flags:#x}")));
    }
    let blend = flags & (VERT_BLENDINDICES | VERT_BLENDWEIGHTS_PACKED) != 0;
    let color = flags & VERT_COLOR != 0;
    let mut uv_layers = 0;
    for slot in 0..8 {
        match (flags >> (24 + 4 * slot)) & 0xF {
            0 => {}
            2 => uv_layers += 1,
            other => {
                return Err(unsupported(format!(
                    "texcoord format {other} in slot {slot}"
                )))
            }
        }
    }
    let size = pos_len + if blend { 8 } else { 0 } + 4 + if color { 4 } else { 0 } + uv_layers * 8;
    if size != stride {
        return Err(unsupported(format!(
            "vertex flags {flags:#x} describe {size} bytes, the stride is {stride}"
        )));
    }
    Ok(VertexLayout {
        position,
        blend,
        color,
        uv_layers,
    })
}

/// What `decode_mesh` needs to know about the model.
struct DecodeCx<'a> {
    bone_map: &'a [u16],
    bone_count: usize,
    extra_weights: bool,
    wide_bones: bool,
}

impl DecodeCx<'_> {
    fn bone(&self, i: usize) -> Result<u16> {
        if self.bone_map.is_empty() {
            return Ok(i as u16);
        }
        self.bone_map
            .get(i)
            .copied()
            .ok_or_else(|| Error::corrupt(format!("rmdl: vertex bone {i} is outside the bone map")))
    }
}

/// Decodes one VG mesh into a part.
fn decode_mesh(group: &[u8], m: &VgMesh, material: u64, d: &DecodeCx) -> Result<MeshPart> {
    let layout = vertex_layout(m.flags, m.stride)?;
    let b = B(group);
    let verts = b.get(
        m.verts,
        m.vert_count
            .checked_mul(m.stride)
            .ok_or_else(|| Error::corrupt("rmdl: vertex buffer size overflows"))?,
    )?;
    let extra = B(b.get(m.extra, m.extra_len)?);
    let v = B(verts);

    let n = m.vert_count;
    let mut part = MeshPart {
        material,
        ..MeshPart::default()
    };
    part.positions.reserve(n);
    part.normals.reserve(n);
    part.uvs = vec![Vec::with_capacity(n); layout.uv_layers];
    // Influences of each vertex, flat, with how many each vertex has.
    let mut flat: Vec<(u16, f32)> = Vec::new();
    let mut counts: Vec<u8> = Vec::with_capacity(n);

    for i in 0..n {
        let mut at = i * m.stride;
        match layout.position {
            1 => {
                part.positions.push(v.floats::<3>(at)?);
                at += 12;
            }
            _ => {
                part.positions.push(unpack_vector64(v.u64(at)?));
                at += 8;
            }
        }
        let start = flat.len();
        if layout.blend {
            let (w0, w1) = (v.u16(at)?, v.u16(at + 2)?);
            let idx = v.get(at + 4, 4)?;
            at += 8;
            let remaining_start = 32767i32;
            if d.extra_weights {
                let (first, last, count) = if d.wide_bones {
                    let packed = u32::from_le_bytes(idx.try_into().expect("4 bytes"));
                    (
                        (packed & 0x3FF) as usize,
                        ((packed >> 10) & 0x3FF) as usize,
                        (packed >> 24) as usize,
                    )
                } else {
                    (
                        usize::from(idx[0]),
                        usize::from(idx[1]),
                        usize::from(idx[3]),
                    )
                };
                flat.push((d.bone(first)?, unpack_weight(w0)));
                let mut remaining = remaining_start - i32::from(w0);
                for k in 1..count {
                    let e = usize::from(w1) + (k - 1);
                    let weight = extra.i16(e * 4)?;
                    let bone = extra.u16(e * 4 + 2)?;
                    flat.push((d.bone(usize::from(bone))?, unpack_weight(weight as u16)));
                    remaining -= i32::from(weight);
                }
                if count > 0 {
                    flat.push((
                        d.bone(last)?,
                        unpack_weight(remaining.clamp(0, 32767) as u16),
                    ));
                }
            } else {
                let count = usize::from(idx[3]);
                if count > 2 {
                    return Err(Error::corrupt(format!(
                        "rmdl: vertex {i} has {} influences",
                        count + 1
                    )));
                }
                let w = [w0, w1];
                let mut remaining = remaining_start;
                for k in 0..count {
                    flat.push((d.bone(usize::from(idx[k]))?, unpack_weight(w[k])));
                    remaining -= i32::from(w[k]);
                }
                flat.push((
                    d.bone(usize::from(idx[count]))?,
                    unpack_weight(remaining.clamp(0, 32767) as u16),
                ));
            }
        } else {
            flat.push((0, 1.0));
        }
        let influences = flat.len() - start;
        if influences > usize::from(u8::MAX) {
            return Err(Error::corrupt(format!(
                "rmdl: vertex {i} has {influences} influences"
            )));
        }
        counts.push(influences as u8);

        part.normals
            .push(unpack_normal32(v.u32(at)?).ok_or_else(|| {
                Error::corrupt(format!("rmdl: vertex {i} has an invalid normal"))
            })?);
        at += 4;
        if layout.color {
            part.colors.push(v.get(at, 4)?.try_into().expect("4 bytes"));
            at += 4;
        }
        for layer in &mut part.uvs {
            layer.push(v.floats::<2>(at)?);
            at += 8;
        }
    }

    // Only a skinned mesh carries bone streams.
    if layout.blend {
        let max = counts.iter().copied().max().unwrap_or(0) as usize;
        part.influences = max as u32;
        part.bone_indices.reserve(n * max);
        part.bone_weights.reserve(n * max);
        let mut cursor = 0;
        for &c in &counts {
            for k in 0..max {
                let (bone, weight) = if k < usize::from(c) {
                    flat[cursor + k]
                } else {
                    (0, 0.0)
                };
                if usize::from(bone) >= d.bone_count.max(1) {
                    return Err(Error::corrupt(format!(
                        "rmdl: vertex bone {bone} but the skeleton has {} bones",
                        d.bone_count
                    )));
                }
                part.bone_indices.push(bone);
                part.bone_weights.push(weight);
            }
            cursor += usize::from(c);
        }
    }

    if !m.index_count.is_multiple_of(3) {
        return Err(Error::corrupt(format!(
            "rmdl: {} indices are not whole triangles",
            m.index_count
        )));
    }
    let raw = b.get(m.indices, m.index_count * 2)?;
    for tri in raw.chunks_exact(6) {
        let t = [0, 2, 4].map(|o| u32::from(u16::from_le_bytes([tri[o], tri[o + 1]])));
        if t.iter().any(|&i| i as usize >= n) {
            return Err(Error::corrupt(format!(
                "rmdl: a triangle refers to vertex {} of {n}",
                t.iter().max().expect("3 values")
            )));
        }
        part.triangles.push(t);
    }
    Ok(part)
}

/// A model: `Data` + `Mesh` + `Skeleton` + `Metadata`.
#[derive(Debug)]
pub struct RmdlAsset {
    format: FormatId,
    key: ValueKey,
    header: RmdlHeader,
    studio: Studio,
    studio_blob: Bytes,
    name: Option<String>,
    guid: u64,
    anim_rigs: Vec<u64>,
    anim_seqs: Vec<u64>,
    volume: Operator,
    stream: Option<String>,
}

impl RmdlAsset {
    /// The parsed header.
    pub fn header(&self) -> &RmdlHeader {
        &self.header
    }

    /// The parsed studio tables.
    pub fn studio(&self) -> &Studio {
        &self.studio
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| hex(self.guid))
    }

    fn flat_models(&self) -> impl Iterator<Item = (&Bodypart, u32, &StudioModel)> {
        self.studio.bodyparts.iter().flat_map(|bp| {
            bp.models
                .iter()
                .enumerate()
                .map(move |(i, m)| (bp, i as u32, m))
        })
    }

    /// The decompressed bytes of a group.
    async fn group_bytes(&self, group: &Group) -> Result<Vec<u8>> {
        let (path, base) = match (&self.stream, self.header.static_data) {
            (Some(s), _) => (s.as_str(), 0),
            (None, Some(at)) => (MEM, at),
            (None, None) => {
                return Err(Error::new(ErrorKind::MissingDependency {
                    item: self.label(),
                    file: "(the model has no vertex data)".into(),
                }));
            }
        };
        let stored = if group.size_compressed != 0 {
            group.size_compressed
        } else {
            group.size_decompressed
        };
        let start = base + u64::from(group.data_offset);
        let block = self
            .volume
            .read_with(path)
            .range(start..start + u64::from(stored))
            .await
            .map_err(|e| {
                let e = Error::from(e);
                if matches!(e.kind(), ErrorKind::NotFound { .. }) {
                    Error::new(ErrorKind::MissingDependency {
                        item: self.label(),
                        file: format!("{path} (no entry in the starpak)"),
                    })
                } else {
                    e
                }
            })?;
        let block = block.to_bytes();
        let decoded_len = u64::from(group.size_decompressed);
        let out = match group.compression {
            0 => block.to_vec(),
            1 => rtech::decompress(&block, 0, Some(decoded_len))
                .map_err(|e| Error::custom("rtech", e))?,
            2 => snowflake::decompress(&block).map_err(|e| Error::custom("snowflake", e))?,
            3 => oodle::decompress(&block, decoded_len).map_err(|e| Error::custom("oodle", e))?,
            other => return Err(unsupported(format!("vertex group compression {other}"))),
        };
        if out.len() as u64 != decoded_len && group.compression != 0 {
            return Err(Error::corrupt(format!(
                "rmdl: a vertex group decoded to {} bytes, expected {decoded_len}",
                out.len()
            )));
        }
        Ok(out)
    }

    /// Hitboxes (`mstudiobbox_v16_t`) as JSON.
    fn hitboxes(&self) -> Result<Vec<Json>> {
        let b = B(&self.studio_blob);
        let sets = usize::from(b.u8(0x36)?);
        let base = b.fixed(0, 0x34)?;
        let mut out = Vec::new();
        for s in 0..sets {
            let set = base + s * 6;
            let set_name = b.cstr(b.fixed(set, set)?, 256)?;
            let count = usize::from(b.u16(set + 2)?);
            let boxes = b.fixed(set, set + 4)?;
            let mut list = Vec::new();
            for i in 0..count {
                let h = boxes + i * 32;
                let name = b.cstr(b.fixed(h, h + 28)?, 256)?;
                list.push(json!({
                    "name": name,
                    "bone": b.u16(h)?,
                    "group": b.u16(h + 2)?,
                    "min": b.floats::<3>(h + 4)?,
                    "max": b.floats::<3>(h + 16)?,
                }));
            }
            out.push(json!({ "name": set_name, "hitboxes": list }));
        }
        Ok(out)
    }

    /// Attachments (56 bytes each) as JSON.
    fn attachments(&self) -> Result<Vec<Json>> {
        let b = B(&self.studio_blob);
        let count = usize::from(b.u8(0x83)?);
        let base = b.fixed(0, 0x84)?;
        (0..count)
            .map(|i| {
                let a = base + i * 56;
                Ok(json!({
                    "name": b.cstr(b.fixed(a, a)?, 256)?,
                    "bone": b.u16(a + 2)?,
                    "flags": b.i32(a + 4)?,
                    "local": b.floats::<12>(a + 8)?,
                }))
            })
            .collect()
    }

    /// Everything the model says about itself as JSON (header, bones, body parts, models, meshes,
    /// materials, skins, LODs, vertex groups, hitboxes, attachments, dependencies), for the
    /// `rmdl.json` target. Guids are `0x`-prefixed hex.
    pub async fn describe(&self) -> Result<Json> {
        let s = &self.studio;
        let h = &self.header;
        let bones: Vec<Json> = s
            .bones
            .iter()
            .map(|b| json!({ "name": b.name, "parent": b.parent, "position": b.position, "rotation": b.rotation, "scale": b.scale }))
            .collect();
        let bodyparts: Vec<Json> = s
            .bodyparts
            .iter()
            .map(|bp| {
                let models: Vec<Json> = bp
                    .models
                    .iter()
                    .map(|m| json!({ "meshes": m.meshes.iter().map(|x| json!({ "material": x.material, "meshId": x.mesh_id })).collect::<Vec<_>>() }))
                    .collect();
                json!({ "name": bp.name, "models": models })
            })
            .collect();
        let groups: Vec<Json> = s
            .groups
            .iter()
            .map(|g| {
                json!({
                    "dataOffset": g.data_offset,
                    "sizeCompressed": g.size_compressed,
                    "sizeDecompressed": g.size_decompressed,
                    "compression": g.compression,
                    "lodIndex": g.lod_index,
                    "lodCount": g.lod_count,
                    "lodMap": g.lod_map,
                })
            })
            .collect();
        let skins: Vec<Json> = (0..s.skins.len() as u32)
            .map(|skin| {
                let row: Vec<String> = s.skins[skin as usize]
                    .iter()
                    .map(|&i| {
                        s.materials
                            .get(usize::from(i))
                            .map_or_else(|| "(missing)".to_owned(), |&g| hex(g))
                    })
                    .collect();
                json!(row)
            })
            .collect();
        Ok(json!({
            "name": self.name,
            "studioName": s.name,
            "guid": hex(self.guid),
            "version": h.version.to_string(),
            "flags": format!("0x{:08X}", s.flags),
            "bounds": { "min": h.bounds.0, "max": h.bounds.1 },
            "streamingSize": h.streaming_size,
            "bones": bones,
            "bodyparts": bodyparts,
            "lods": s.lod_switch,
            "vertexGroups": groups,
            "materials": s.materials.iter().map(|&g| hex(g)).collect::<Vec<_>>(),
            "skins": skins,
            "hitboxSets": self.hitboxes()?,
            "attachments": self.attachments()?,
            "animRigs": self.anim_rigs.iter().map(|&g| hex(g)).collect::<Vec<_>>(),
            "animSeqs": self.anim_seqs.iter().map(|&g| hex(g)).collect::<Vec<_>>(),
        }))
    }
}

#[async_trait]
impl Mesh for RmdlAsset {
    fn info(&self) -> MeshInfo {
        let has_vertex_data = !self.studio.groups.is_empty();
        MeshInfo {
            models: self
                .flat_models()
                .map(|(bp, index, _)| ModelInfo {
                    bodypart: bp.name.clone(),
                    index,
                })
                .collect(),
            lod_count: if has_vertex_data {
                self.studio.lod_slots.len() as u32
            } else {
                0
            },
            skin_count: self.studio.skins.len() as u32,
        }
    }

    async fn read(&self, lod: u32, model: u32, skin: u32, cx: &Cx) -> Result<MeshModel> {
        let not_found = |what: String| Error::new(ErrorKind::NotFound { path: what });
        let (_, _, studio_model) = self
            .flat_models()
            .nth(model as usize)
            .ok_or_else(|| not_found(format!("model {model}")))?;
        let slot = *self
            .studio
            .lod_slots
            .get(lod as usize)
            .ok_or_else(|| not_found(format!("lod {lod}")))?;
        let Some((group_index, in_group)) = slot else {
            return Ok(MeshModel::default());
        };
        cx.check_cancelled()?;
        let group = self.studio.groups[group_index];
        let bytes = self.group_bytes(&group).await?;
        let headers = vg_meshes(&bytes, in_group)?;
        let d = DecodeCx {
            bone_map: &self.studio.bone_map,
            bone_count: self.studio.bones.len(),
            extra_weights: self.studio.flags & FLAG_EXTRA_WEIGHTS != 0,
            wide_bones: self.studio.layout == StudioLayout::V19_2,
        };
        let mut parts = Vec::new();
        for sm in &studio_model.meshes {
            let Some(h) = headers.get(usize::from(sm.mesh_id)) else {
                return Err(Error::corrupt(format!(
                    "rmdl: mesh id {} but LOD {lod} has {} meshes",
                    sm.mesh_id,
                    headers.len()
                )));
            };
            if h.flags == 0 {
                continue;
            }
            parts.push(decode_mesh(
                &bytes,
                h,
                self.studio.material_of(skin, sm)?,
                &d,
            )?);
        }
        Ok(MeshModel { parts })
    }

    async fn materials(&self, skin: u32, cx: &Cx) -> Result<Vec<MeshMaterial>> {
        let mut guids: Vec<u64> = Vec::new();
        for (_, _, m) in self.flat_models() {
            for sm in &m.meshes {
                let guid = self.studio.material_of(skin, sm)?;
                if !guids.contains(&guid) {
                    guids.push(guid);
                }
            }
        }
        let origin = pak_of(&self.key);
        let mut out = Vec::with_capacity(guids.len());
        for guid in guids {
            cx.check_cancelled()?;
            out.push(self.material(guid, &origin, cx).await?);
        }
        Ok(out)
    }
}

impl RmdlAsset {
    /// One material with its textures located; whatever cannot be found is left out with a warning.
    async fn material(&self, guid: u64, origin: &str, cx: &Cx) -> Result<MeshMaterial> {
        let missing = |why: &Error| {
            cx.warn(format!(
                "model {}: material {} is not available ({why})",
                self.label(),
                hex(guid)
            ));
            MeshMaterial {
                guid,
                name: hex(guid),
                textures: Vec::new(),
            }
        };
        let asset = match locate(guid, "matl", origin, cx).await {
            Ok(a) => a,
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => return Ok(missing(&e)),
        };
        let matl_value = match cx
            .mediator
            .identify_as(asset, &FormatId::new("matl"), cx)
            .await
        {
            Ok(v) => v,
            Err(e) if e.is_cancelled() => return Err(e),
            Err(e) => return Ok(missing(&e)),
        };
        let Some(matl) = matl_value.downcast_ref::<MatlAsset>() else {
            return Ok(missing(&Error::corrupt("the material is not a matl")));
        };
        let name = matl.name.as_deref().map_or_else(
            || hex(guid),
            |n| n.rsplit('/').next().unwrap_or(n).to_owned(),
        );
        let slot_names = if matl.bound_textures().next().is_none() {
            Default::default()
        } else {
            match shader_slots(matl, origin, cx).await {
                Ok(names) => names,
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    cx.warn(format!(
                        "material {name}: texture slots are not named ({e}); using slot_<n>"
                    ));
                    Default::default()
                }
            }
        };
        let mut textures = Vec::new();
        for (slot, texture_guid) in matl.bound_textures() {
            let texture = match locate(texture_guid, "txtr", origin, cx).await {
                Ok(t) => Some(t),
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    cx.warn(format!(
                        "material {name}: texture {} is not available ({e})",
                        hex(texture_guid)
                    ));
                    None
                }
            };
            let slot_name = slot_names
                .get(&slot)
                .cloned()
                .unwrap_or_else(|| format!("slot_{slot}"));
            textures.push(MaterialTexture {
                slot: slot_name,
                guid: texture_guid,
                texture,
            });
        }
        Ok(MeshMaterial {
            guid,
            name,
            textures,
        })
    }
}

impl Skeleton for RmdlAsset {
    fn bones(&self) -> &[Bone] {
        &self.studio.bones
    }
}

impl Metadata for RmdlAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("mdl_")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        let mut seen = Vec::new();
        for &guid in self
            .studio
            .materials
            .iter()
            .chain(&self.anim_rigs)
            .chain(&self.anim_seqs)
        {
            if guid != 0 && !seen.contains(&guid) {
                seen.push(guid);
            }
        }
        seen.into_iter()
            .map(|guid| Dependency { guid, name: None })
            .collect()
    }
}

impl Data for RmdlAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_mesh(&self) -> Option<&dyn Mesh> {
        Some(self)
    }
    fn as_skeleton(&self) -> Option<&dyn Skeleton> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// Read edge `rpak.asset -> rmdl`.
pub struct RmdlRead;

#[async_trait]
impl Detect for RmdlRead {
    /// The asset kind is `mdl_` (a container-provided kind) = 10, with the asset version as the
    /// version; the variant is worked out when the model is read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"mdl_").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

/// `count` guids at `.mem` offset `at`.
pub(crate) async fn read_guids(asset: &RpakAsset, at: Option<u64>, count: u32) -> Result<Vec<u64>> {
    let (Some(at), true) = (at, count > 0) else {
        return Ok(Vec::new());
    };
    if count as usize > MAX_GUIDS {
        return Err(Error::corrupt(format!(
            "rmdl lists {count} animation assets"
        )));
    }
    let bytes = asset.read_mem(at, u64::from(count) * 8).await?;
    Ok(bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes")))
        .collect())
}

#[async_trait]
impl ReadHandler for RmdlRead {
    /// Parses the headers, the bones and the body part tables. Vertex data is read later, by `Mesh::read`.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "rmdl must be read from an rpak.asset".into(),
            })
        })?;
        let major = asset.entry.version;
        if !(16..=20).contains(&major) {
            return Err(unsupported(major.to_string()));
        }
        let header_bytes = asset.read_header().await?;
        let data_at = asset
            .data_mem_offset
            .ok_or_else(|| Error::corrupt("rmdl has no CPU data pointer"))?;
        let cpu = asset.read_mem(data_at, CPU_LEN as u64).await?;
        let studio_at = B(&header_bytes).u64(0)?;
        let studio_len = u64::from(B(&cpu).u32(12)?);
        if studio_len == 0 || studio_len > MAX_STUDIO_LEN {
            return Err(Error::corrupt(format!(
                "rmdl studio block of {studio_len} bytes is not usable"
            )));
        }
        let studio_blob = asset.read_mem(studio_at, studio_len).await?;
        let source_filename_offset = B(&studio_blob).u16(0xD8).ok();
        let header = RmdlHeader::parse(
            major,
            &header_bytes,
            &cpu,
            asset.pak_created_time,
            source_filename_offset,
        )?;
        let studio = Studio::parse(major, header.layout, &studio_blob)?;

        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let name = match header.name_offset {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let anim_rigs = read_guids(asset, header.anim_rigs.0, header.anim_rigs.1).await?;
        let anim_seqs = read_guids(asset, header.anim_seqs.0, header.anim_seqs.1).await?;
        let format = FormatId::new("rmdl").with_version(header.version);
        Ok(Arc::new(RmdlAsset {
            format,
            key: source.key().clone(),
            header,
            studio,
            studio_blob,
            name,
            guid: asset.entry.guid,
            anim_rigs,
            anim_seqs,
            volume: asset.volume.clone(),
            stream: asset.stream.clone(),
        }))
    }
}

/// Unpacks a `Vector64` position (x, y: 21 bits, z: 22 bits, steps of 1/1024).
pub(crate) fn unpack_vector64(packed: u64) -> [f32; 3] {
    const STEP: f32 = 0.000_976_562_5;
    let x = (packed & 0x1F_FFFF) as f32;
    let y = ((packed >> 21) & 0x1F_FFFF) as f32;
    let z = (packed >> 42) as f32;
    [x * STEP - 1024.0, y * STEP - 1024.0, z * STEP - 2048.0]
}

/// Unpacks a `Normal32` into a unit normal (the tangent is not needed). `None` if the index of
/// the dropped component is 3.
pub(crate) fn unpack_normal32(packed: u32) -> Option<[f32; 3]> {
    let i1 = ((packed >> 29) & 3) as usize;
    if i1 > 2 {
        return None;
    }
    let negative = (packed >> 28) & 1 != 0;
    let v1 = if negative { -255.0 } else { 255.0 };
    let v2 = ((packed >> 19) & 0x1FF) as f32 - 256.0;
    let v3 = ((packed >> 10) & 0x1FF) as f32 - 256.0;
    let i2 = ((0x124u32 >> (2 * i1 + 2)) & 3) as usize;
    let i3 = ((0x124u32 >> (2 * i1 + 4)) & 3) as usize;
    let len = (255.0f32 * 255.0 + v2 * v2 + v3 * v3).sqrt();
    let mut n = [0.0; 3];
    n[i1] = v1 / len;
    n[i2] = v2 / len;
    n[i3] = v3 / len;
    Some(n)
}

/// `FIX_OFFSET`: bit 0 set means `(v & 0xFFFE) * 16`, otherwise `v`.
pub(crate) fn fix_offset(v: u16) -> u32 {
    u32::from(v & 0xFFFE) << (4 * (v & 1))
}

/// A weight as stored (`UNPACKWEIGHT`): `(w + 1) / 32768`, so 32767 is 1.0.
pub(crate) fn unpack_weight(w: u16) -> f32 {
    (f32::from(w) + 1.0) / 32768.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::rmdl::{
        blend_vertex, flags, normal_z, put16, studio, vg_single as vg_group,
    };

    fn studio_block(major: u32, wide: bool) -> Vec<u8> {
        studio(major, wide, 100)
    }

    #[test]
    fn fix_offset_both_parities() {
        assert_eq!(fix_offset(0), 0);
        assert_eq!(fix_offset(0x0100), 0x0100);
        assert_eq!(fix_offset(0x0101), 0x1000);
        assert_eq!(fix_offset(0x0003), 0x0020);
        assert_eq!(fix_offset(0xFFFF), 0xFFFE << 4);
    }

    #[test]
    fn vector64_known_values() {
        assert_eq!(unpack_vector64(0), [-1024.0, -1024.0, -2048.0]);
        // x = 1024 steps (= 1.0), y = 2048 steps, z = 4096 steps.
        let packed = 1024u64 | (2048u64 << 21) | (4096u64 << 42);
        assert_eq!(unpack_vector64(packed), [-1023.0, -1022.0, -2044.0]);
        let max = unpack_vector64(u64::MAX);
        assert!((max[0] - 1023.999).abs() < 0.01 && (max[2] - 2047.999).abs() < 0.01);
    }

    #[test]
    fn normal32_axis_aligned() {
        let pack = |idx1: u32, neg: u32| (idx1 << 29) | (neg << 28) | (256 << 19) | (256 << 10);
        for (idx1, neg) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)] {
            let n = unpack_normal32(pack(idx1, neg)).unwrap();
            for (axis, v) in n.iter().enumerate() {
                let want = if axis as u32 == idx1 {
                    if neg == 1 {
                        -1.0
                    } else {
                        1.0
                    }
                } else {
                    0.0
                };
                assert!(
                    (v - want).abs() < 1e-6,
                    "idx1 {idx1} neg {neg} axis {axis}: {v}"
                );
            }
        }
        assert!(unpack_normal32(3 << 29).is_none());
        let n = unpack_normal32((2 << 29) | (300 << 19) | (200 << 10)).unwrap();
        assert!(
            (n.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5,
            "unit length"
        );
    }

    #[test]
    fn weights() {
        assert_eq!(unpack_weight(32767), 1.0);
        assert_eq!(unpack_weight(0), 1.0 / 32768.0);
    }

    #[test]
    fn variants() {
        assert_eq!(variant(16, 0, None).unwrap(), Version::new(16));
        assert_eq!(variant(20, 0, None).unwrap(), Version::new(20));
        assert_eq!(variant(19, 0, None).unwrap(), Version::with_minor(19, 0));
        assert_eq!(
            variant(19, CREATED_19_1, None).unwrap(),
            Version::with_minor(19, 1)
        );
        assert_eq!(
            variant(19, 0, Some(244)).unwrap(),
            Version::with_minor(19, 2)
        );
        assert_eq!(
            variant(19, CREATED_19_1, Some(244)).unwrap(),
            Version::with_minor(19, 2)
        );
        assert_eq!(
            variant(19, CREATED_19_3, Some(244)).unwrap(),
            Version::with_minor(19, 3)
        );
        assert_eq!(
            variant(19, CREATED_19_3, None).unwrap(),
            Version::with_minor(19, 3)
        );
        for bad in [8, 15, 21] {
            assert!(matches!(
                variant(bad, 0, None).unwrap_err().kind(),
                ErrorKind::UnsupportedVersion { .. }
            ));
        }
        assert_eq!(
            StudioLayout::for_version(Version::new(16)).unwrap(),
            StudioLayout::V16
        );
        assert_eq!(
            StudioLayout::for_version(Version::new(18)).unwrap(),
            StudioLayout::V17
        );
        assert_eq!(
            StudioLayout::for_version(Version::with_minor(19, 1)).unwrap(),
            StudioLayout::V17
        );
        assert_eq!(
            StudioLayout::for_version(Version::with_minor(19, 2)).unwrap(),
            StudioLayout::V19_2
        );
        assert_eq!(
            StudioLayout::for_version(Version::new(20)).unwrap(),
            StudioLayout::V19_2
        );
    }

    /// A 96-byte asset header and 16-byte CPU data.
    fn headers(studio_len: i32) -> (Vec<u8>, Vec<u8>) {
        let mut h = vec![0u8; ASSET_HEADER_LEN];
        h[0..8].copy_from_slice(&0x1000u64.to_le_bytes());
        h[8..16].copy_from_slice(&0x2000u64.to_le_bytes());
        h[0x18..0x20].copy_from_slice(&u64::MAX.to_le_bytes());
        h[0x20..0x28].copy_from_slice(&0x3000u64.to_le_bytes());
        h[0x28..0x2C].copy_from_slice(&2u32.to_le_bytes());
        h[0x2C..0x30].copy_from_slice(&8192u32.to_le_bytes());
        h[0x30..0x34].copy_from_slice(&(-1.0f32).to_le_bytes());
        h[0x3C..0x40].copy_from_slice(&1.0f32.to_le_bytes());
        h[0x4A..0x4C].copy_from_slice(&3u16.to_le_bytes());
        h[0x50..0x58].copy_from_slice(&0x4000u64.to_le_bytes());
        let mut c = vec![0u8; CPU_LEN];
        c[12..16].copy_from_slice(&studio_len.to_le_bytes());
        (h, c)
    }

    #[test]
    fn asset_header() {
        let (h, c) = headers(500);
        let p = RmdlHeader::parse(20, &h, &c, 0, None).unwrap();
        assert_eq!(p.version, Version::new(20));
        assert_eq!(p.layout, StudioLayout::V19_2);
        assert_eq!(
            (p.studio_offset, p.studio_len, p.streaming_size),
            (0x1000, 500, 8192)
        );
        assert_eq!(p.name_offset, Some(0x2000));
        assert_eq!(p.static_data, None);
        assert_eq!(p.bounds, ([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert_eq!(p.anim_rigs, (Some(0x3000), 2));
        assert_eq!(p.anim_seqs, (Some(0x4000), 3));

        assert!(matches!(
            RmdlHeader::parse(15, &h, &c, 0, None).unwrap_err().kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        assert!(matches!(
            RmdlHeader::parse(16, &h[..90], &c, 0, None)
                .unwrap_err()
                .kind(),
            ErrorKind::Corrupt { .. }
        ));
        assert!(matches!(
            RmdlHeader::parse(16, &h, &c[..8], 0, None)
                .unwrap_err()
                .kind(),
            ErrorKind::Corrupt { .. }
        ));
        let (h0, c0) = headers(0);
        assert!(matches!(
            RmdlHeader::parse(16, &h0, &c0, 0, None).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
        let mut no_studio = h.clone();
        no_studio[0..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            RmdlHeader::parse(16, &no_studio, &c, 0, None)
                .unwrap_err()
                .kind(),
            ErrorKind::Corrupt { .. }
        ));
    }

    fn check_studio(major: u32, layout: StudioLayout) {
        let s = Studio::parse(
            major,
            layout,
            &studio_block(major, layout == StudioLayout::V19_2),
        )
        .unwrap();
        assert_eq!(s.name, "model");
        assert_eq!(s.flags, 0x4000);
        assert_eq!(s.bones.len(), 2);
        assert_eq!(
            (s.bones[0].name.as_str(), s.bones[0].parent),
            ("root", None)
        );
        assert_eq!(
            (s.bones[1].name.as_str(), s.bones[1].parent),
            ("arm", Some(0))
        );
        assert_eq!(s.bones[1].position, [2.0, 2.0, 3.0]);
        assert_eq!(s.bones[1].rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.bones[1].scale, [1.0; 3]);
        assert_eq!(s.materials, vec![0xAAAA, 0xBBBB]);
        assert_eq!(s.skins, vec![vec![0, 1], vec![1, 0]]);
        assert_eq!(s.bodyparts.len(), 1);
        assert_eq!(s.bodyparts[0].name, "weapon");
        assert_eq!(
            s.bodyparts[0].models[0].meshes,
            vec![
                StudioMesh {
                    material: 0,
                    mesh_id: 0
                },
                StudioMesh {
                    material: 1,
                    mesh_id: 1
                }
            ]
        );
        assert_eq!(s.lod_switch, vec![10.0, 50.0]);
        assert_eq!(s.groups.len(), 1);
        assert_eq!(
            (
                s.groups[0].size_compressed,
                s.groups[0].lod_count,
                s.groups[0].lod_map
            ),
            (100, 2, 0b11)
        );
        assert_eq!(s.lod_slots, vec![Some((0, 0)), Some((0, 1))]);
        assert_eq!(s.bone_map, vec![1, 0]);
        // skin 1 swaps the materials
        assert_eq!(
            s.material_of(0, &s.bodyparts[0].models[0].meshes[0])
                .unwrap(),
            0xAAAA
        );
        assert_eq!(
            s.material_of(1, &s.bodyparts[0].models[0].meshes[0])
                .unwrap(),
            0xBBBB
        );
        assert!(matches!(
            s.material_of(2, &s.bodyparts[0].models[0].meshes[0])
                .unwrap_err()
                .kind(),
            ErrorKind::NotFound { .. }
        ));
    }

    #[test]
    fn studio_v16() {
        check_studio(16, StudioLayout::V16);
    }

    #[test]
    fn studio_v18() {
        check_studio(18, StudioLayout::V17);
    }

    #[test]
    fn studio_v19_linear_bones() {
        check_studio(19, StudioLayout::V17);
    }

    #[test]
    fn studio_v20_wide_fields() {
        check_studio(20, StudioLayout::V19_2);
    }

    #[test]
    fn studio_is_bounded() {
        let block = studio_block(16, false);
        for len in [0, 10, 0x74, 0x120, 0x290, 0x328] {
            assert!(
                Studio::parse(16, StudioLayout::V16, &block[..len]).is_err(),
                "len {len}"
            );
        }
        let mut bad_parent = block.clone();
        bad_parent[0x140 + 116..0x140 + 118].copy_from_slice(&5i16.to_le_bytes());
        assert!(matches!(
            Studio::parse(16, StudioLayout::V16, &bad_parent)
                .unwrap_err()
                .kind(),
            ErrorKind::Corrupt { .. }
        ));
        let mut too_many = block;
        put16(&mut too_many, 0x74, 5000);
        assert!(matches!(
            Studio::parse(16, StudioLayout::V16, &too_many)
                .unwrap_err()
                .kind(),
            ErrorKind::Corrupt { .. }
        ));
    }

    #[test]
    fn layouts() {
        assert_eq!(
            vertex_layout(flags(1, true, true, 2), 12 + 8 + 4 + 4 + 16).unwrap(),
            VertexLayout {
                position: 1,
                blend: true,
                color: true,
                uv_layers: 2
            }
        );
        assert_eq!(
            vertex_layout(flags(2, false, false, 1), 8 + 4 + 8)
                .unwrap()
                .position,
            2
        );
        assert!(matches!(
            vertex_layout(flags(3, false, false, 0), 10)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        assert!(matches!(
            vertex_layout(flags(0, false, false, 0), 4)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        // stride that does not match
        assert!(matches!(
            vertex_layout(flags(1, false, false, 0), 20)
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        // an unknown texcoord format
        assert!(vertex_layout(flags(1, false, false, 0) | (5 << 24), 16).is_err());
        // VERT_UNK (0x40) takes no bytes, but a stride that does not fit is still refused
        assert_eq!(
            vertex_layout(flags(1, false, false, 0) | 0x40, 16)
                .unwrap()
                .position,
            1
        );
        assert!(vertex_layout(flags(1, false, false, 0) | 0x40, 24).is_err());
        assert!(
            vertex_layout(flags(1, false, false, 0) | 0x2000, 16).is_err(),
            "unpacked weights"
        );
    }

    fn cx_plain<'a>(map: &'a [u16], bones: usize, extra: bool, wide: bool) -> DecodeCx<'a> {
        DecodeCx {
            bone_map: map,
            bone_count: bones,
            extra_weights: extra,
            wide_bones: wide,
        }
    }

    #[test]
    fn decodes_an_unskinned_mesh() {
        // Vector64 position, no blend, normal, 1 UV: 8 + 4 + 8 = 20
        let f = flags(2, false, false, 1);
        let mut verts = Vec::new();
        for i in 0..3u64 {
            verts
                .extend_from_slice(&((1024 * (i + 1)) | (2048 << 21) | (4096 << 42)).to_le_bytes());
            verts.extend_from_slice(&normal_z().to_le_bytes());
            verts.extend_from_slice(&(0.25f32 * i as f32).to_le_bytes());
            verts.extend_from_slice(&0.5f32.to_le_bytes());
        }
        let g = vg_group(f, 20, 3, &verts, &[0, 1, 2], &[]);
        let headers = vg_meshes(&g, 0).unwrap();
        assert_eq!(headers.len(), 1);
        let part = decode_mesh(&g, &headers[0], 9, &cx_plain(&[], 1, false, false)).unwrap();
        assert_eq!(part.material, 9);
        assert_eq!(
            part.positions,
            vec![
                [-1023.0, -1022.0, -2044.0],
                [-1022.0, -1022.0, -2044.0],
                [-1021.0, -1022.0, -2044.0]
            ]
        );
        assert!(part.normals.iter().all(|n| (n[2] - 1.0).abs() < 1e-6));
        assert_eq!(part.uvs, vec![vec![[0.0, 0.5], [0.25, 0.5], [0.5, 0.5]]]);
        assert_eq!(part.triangles, vec![[0, 1, 2]]);
        assert_eq!(part.influences, 0);
        assert!(part.bone_indices.is_empty());
    }

    #[test]
    fn decodes_blend_weights() {
        let f = flags(1, true, false, 0);
        let mut verts = Vec::new();
        verts.extend(blend_vertex(0, 0, [0, 0, 0, 0])); // one bone, remaining weight 1.0
        verts.extend(blend_vertex(16383, 0, [0, 1, 0, 1])); // two bones
        verts.extend(blend_vertex(10000, 12000, [1, 0, 1, 2])); // three bones
        let g = vg_group(f, 24, 3, &verts, &[0, 1, 2], &[]);
        let h = vg_meshes(&g, 0).unwrap();
        let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 2, false, false)).unwrap();
        assert_eq!(part.influences, 3);
        assert_eq!(part.bone_indices, vec![0, 0, 0, 0, 1, 0, 1, 0, 1]);
        let w = &part.bone_weights;
        assert_eq!(w[0], unpack_weight(32767)); // 1.0
        assert_eq!(w[1], 0.0, "padding");
        for v in 0..3 {
            let sum: f32 = w[v * 3..v * 3 + 3].iter().sum();
            assert!((sum - 1.0).abs() < 4.0 / 32768.0, "vertex {v}: {sum}");
        }
        // The bone map redirects, and a bone outside it is corrupt.
        let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[1, 0], 2, false, false)).unwrap();
        assert_eq!(part.bone_indices[3..6], [1, 0, 0]);
        let err = decode_mesh(&g, &h[0], 1, &cx_plain(&[1], 2, false, false)).unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
        // a skeleton too small for the bones
        let err = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 1, false, false)).unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
    }

    #[test]
    fn decodes_extra_weights() {
        let f = flags(1, true, false, 0);
        // 256-bone form: first 2, last 5, unk, count 3 -> weights: first, extras[w1..w1+1], last
        let extra: Vec<u8> = [(8000i16, 3i16), (4000, 4)]
            .iter()
            .flat_map(|(w, b)| w.to_le_bytes().into_iter().chain(b.to_le_bytes()))
            .collect();
        let verts = blend_vertex(10000, 0, [2, 5, 0, 3]);
        let g = vg_group(f, 24, 1, &verts, &[], &extra);
        let h = vg_meshes(&g, 0).unwrap();
        let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 8, true, false)).unwrap();
        assert_eq!(part.influences, 4);
        assert_eq!(part.bone_indices, vec![2, 3, 4, 5]);
        let sum: f32 = part.bone_weights.iter().sum();
        assert!((sum - 1.0).abs() < 8.0 / 32768.0, "{sum}");
        assert_eq!(part.bone_weights[0], unpack_weight(10000));
        assert_eq!(part.bone_weights[1], unpack_weight(8000));

        // 1024-bone form: first 700, last 900, count 2
        let packed: u32 = 700 | (900 << 10) | (2 << 24);
        let mut idx = packed.to_le_bytes();
        let verts = blend_vertex(20000, 0, idx);
        idx = [0; 4];
        let _ = idx;
        let extra: Vec<u8> = [(6000i16, 800i16)]
            .iter()
            .flat_map(|(w, b)| w.to_le_bytes().into_iter().chain(b.to_le_bytes()))
            .collect();
        let g = vg_group(f, 24, 1, &verts, &[], &extra);
        let h = vg_meshes(&g, 0).unwrap();
        let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 1024, true, true)).unwrap();
        assert_eq!(part.bone_indices, vec![700, 800, 900]);
    }

    #[test]
    fn mesh_decoding_rejects_bad_data() {
        let f = flags(1, false, false, 0);
        let mut verts = Vec::new();
        for _ in 0..3 {
            verts.extend_from_slice(&[0u8; 12]);
            verts.extend_from_slice(&normal_z().to_le_bytes());
        }
        let cx = cx_plain(&[], 1, false, false);
        // not whole triangles
        let g = vg_group(f, 16, 3, &verts, &[0, 1], &[]);
        assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
        // a triangle past the vertices
        let g = vg_group(f, 16, 3, &verts, &[0, 1, 3], &[]);
        assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
        // a vertex buffer that does not fit
        let g = vg_group(f, 16, 4, &verts, &[0, 1, 2], &[]);
        assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
        // a stride that does not match the flags
        let g = vg_group(f, 20, 3, &verts, &[0, 1, 2], &[]);
        assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
        // a group header past the data
        assert!(vg_meshes(&g[..6], 0).is_err());
        assert!(vg_meshes(&g, 3).is_err());
    }
}
