//! `RmdlAsset`: the model as `Data` + `Mesh` + `Skeleton` + `Metadata`.

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{
    Bone, Dependency, MaterialTexture, Mesh, MeshInfo, MeshMaterial, MeshModel, Metadata,
    ModelInfo, Skeleton,
};
use respak_core::{Cx, Data, Error, ErrorKind, FormatId, Result, ValueKey};
use serde_json::{json, Value as Json};

use super::block::{reader, FixedOffset};
use super::header::RmdlHeader;
use super::studio::{Bodypart, Group, Studio, StudioModel, FLAG_EXTRA_WEIGHTS};
use super::vg::{decode_mesh, vg_meshes, DecodeCx};
use super::{unsupported, StudioLayout};
use crate::guid::hex;
use crate::matl::MatlAsset;
use crate::matl::{locate, pak_of, shader_slots};
use crate::rpak::MEM;
use crate::{oodle, rtech, snowflake};

/// A model: `Data` + `Mesh` + `Skeleton` + `Metadata`.
#[derive(Debug)]
pub struct RmdlAsset {
    pub(super) format: FormatId,
    pub(super) key: ValueKey,
    pub(super) header: RmdlHeader,
    pub(super) studio: Studio,
    pub(super) studio_blob: Bytes,
    pub(super) name: Option<String>,
    pub(super) guid: u64,
    pub(super) anim_rigs: Vec<u64>,
    pub(super) anim_seqs: Vec<u64>,
    pub(super) volume: Operator,
    pub(super) stream: Option<String>,
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
        let b = reader(&self.studio_blob);
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
        let b = reader(&self.studio_blob);
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
            .read_as(asset, &FormatId::new("matl"), cx)
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
