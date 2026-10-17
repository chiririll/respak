//! The studiohdr tables a mesh export needs: bones, body parts, materials, skins, LODs, groups.

use respak_core::capability::Bone;
use respak_core::reader::Reader;
use respak_core::{Error, ErrorKind, Result};

use super::block::{reader, FixedOffset};
use super::header::StudioLayout;

const MAX_BONES: usize = 2048;
const MAX_LODS: usize = 8;
const MAX_ITEMS: usize = 65_535;
/// `STUDIOHDR_FLAGS_USES_EXTRA_BONE_WEIGHTS`.
pub(super) const FLAG_EXTRA_WEIGHTS: i32 = 0x4000;

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
pub(crate) fn parse_bones(b: &Reader, major: u32, layout: StudioLayout) -> Result<Vec<Bone>> {
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
        let b = reader(blob);
        let flags = b.i32(0)?;
        let name = b.cstr(0x0A, 33)?;
        let bones = parse_bones(&b, major, layout)?;
        let (materials, skins) = parse_materials(&b)?;
        let bodyparts = parse_bodyparts(&b)?;
        let (lod_switch, groups, lod_slots) = parse_lods(&b, layout.shift())?;
        let bone_map = parse_bone_map(&b, layout.shift())?;
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
    pub(super) fn material_of(&self, skin: u32, m: &StudioMesh) -> Result<u64> {
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

/// The material guids and the skin rows (`skinref[family][material]`) of a studio block.
fn parse_materials(b: &Reader) -> Result<(Vec<u64>, Vec<Vec<u16>>)> {
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
    Ok((materials, skins))
}

/// Body parts, their models and meshes.
fn parse_bodyparts(b: &Reader) -> Result<Vec<Bodypart>> {
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
    Ok(bodyparts)
}

/// LOD switch distances, the vertex groups, and which group holds each LOD.
#[allow(clippy::type_complexity)]
fn parse_lods(
    b: &Reader,
    shift: usize,
) -> Result<(Vec<f32>, Vec<Group>, Vec<Option<(usize, u32)>>)> {
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
    Ok((lod_switch, groups, lod_slots))
}

/// The bone remap table (`boneStateCount`): bytes up to 19.1, words from 19.2.
fn parse_bone_map(b: &Reader, shift: usize) -> Result<Vec<u16>> {
    let state_count = usize::from(b.u16(0xB2)?);
    let state_base = b.fixed(0xB0, 0xB0)?;
    (0..state_count)
        .map(|i| {
            if shift > 0 {
                b.u16(state_base + i * 2)
            } else {
                Ok(u16::from(b.u8(state_base + i)?))
            }
        })
        .collect()
}
