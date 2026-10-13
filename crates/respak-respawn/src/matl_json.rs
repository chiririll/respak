//! `matl -> matl.json`: a material with its textures (PROJECT.md §5.5, "linking").
//!
//! The result is a bundle the engine writes into the material's own folder:
//! `material.json` plus `textures/<slot name>.<ext>`. Each texture is looked up by guid
//! (`rpak.guid -> rpak.asset`, which may find it in another pak) and converted through the
//! mediator to the format of the `textures` option (default `png`), so a texture shared by
//! two materials is written once per material.
//!
//! Slot names come from the material's shader (shader set -> pixel shader -> DXBC). If any
//! step fails the slots are called `slot_<n>` and a warning says why.
//!
//! The JSON, in the order of rsx's material export where the field exists there:
//! `name`, `guid`, `version`, `width`, `height`, `depth`, `shaderSet`, `surfaceProp`,
//! `surfaceProp2`, `glueFlags`, `glueFlags2`, `samplers`, `features`, the five depth/colpass
//! material guids, and `textures: { <name>: { slot, guid, file } }` (`file` is `null` for a
//! texture that could not be converted; a warning says why). Guids are `0x`-prefixed hex.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::value::{BlobData, BundleData};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as Json};

use crate::locate::GuidRef;
use crate::matl::MatlAsset;
use crate::rpak::RpakAsset;
use crate::{dxbc, shader};

/// The target the material's textures are converted to: `{"type":"png", ...options}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextureTarget {
    /// Target format of the textures.
    #[serde(rename = "type")]
    pub format: String,
    /// The format's own options.
    #[serde(flatten)]
    pub options: Map<String, Json>,
}

impl Default for TextureTarget {
    fn default() -> Self {
        Self { format: "png".into(), options: Map::new() }
    }
}

/// Options of the `matl.json` target: `{"type":"matl.json","textures":{"type":"dds"}}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatlJsonOptions {
    /// What the textures are converted to; default `png`.
    pub textures: TextureTarget,
}

/// Convert edge `matl -> matl.json`.
pub struct MatlJsonEncode;

fn hex(guid: u64) -> String {
    format!("0x{guid:016X}")
}

/// The virtual path of the pak a value from a pak volume lives in.
fn pak_of(key: &ValueKey) -> String {
    key.to_string().rsplit_once('/').map_or(String::new(), |(pak, _)| pak.to_owned())
}

fn is_cancelled(e: &Error) -> bool {
    matches!(e.kind(), ErrorKind::Cancelled)
}

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom { source: None, message: message.into() })
}

/// The pak asset `guid` of `kind` refers to, searched from the pak at `origin`.
async fn locate(guid: u64, kind: &str, origin: &str, cx: &Cx) -> Result<Value> {
    cx.mediator.identify(Arc::new(GuidRef::new(guid, kind, origin)), cx).await
}

/// Bind point -> shader variable name, from the material's shader.
async fn shader_slots(matl: &MatlAsset, origin: &str, cx: &Cx) -> Result<BTreeMap<u32, String>> {
    if matl.header.shader_set == 0 {
        return Err(custom("the material has no shader set"));
    }
    let shds = locate(matl.header.shader_set, "shds", origin, cx).await?;
    let shds_asset = shds.downcast_ref::<RpakAsset>().ok_or_else(|| custom("the shader set is not a pak asset"))?;
    let pixel = shader::pixel_shader_guid(shds_asset.entry.version, &shds_asset.read_header().await?)?;
    let shdr = locate(pixel, "shdr", &pak_of(shds.key()), cx).await?;
    let shdr_asset = shdr.downcast_ref::<RpakAsset>().ok_or_else(|| custom("the pixel shader is not a pak asset"))?;
    dxbc::texture_slots(&shader::bytecode(shdr_asset).await?)
}

fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

#[async_trait]
impl ConvertHandler for MatlJsonEncode {
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let matl = source.downcast_ref::<MatlAsset>().ok_or_else(|| custom("matl.json must be made from a matl"))?;
        let options = TypedTarget::<MatlJsonOptions>::parse(target)?;
        let texture_target = Target {
            format: FormatId::new(options.options().textures.format.clone()),
            options: if options.options().textures.options.is_empty() {
                Json::Null
            } else {
                Json::Object(options.options().textures.options.clone())
            },
            via: Vec::new(),
        };
        let label = matl.name.clone().unwrap_or_else(|| hex(matl.header.guid));
        let origin = pak_of(source.key());

        let slot_names = match shader_slots(matl, &origin, cx).await {
            Ok(names) => names,
            Err(e) if is_cancelled(&e) => return Err(e),
            Err(e) => {
                cx.warn(format!("material {label}: texture slots are not named ({e}); using slot_<n>"));
                BTreeMap::new()
            }
        };

        let mut items: Vec<(String, Value)> = Vec::new();
        let mut textures = Map::new();
        let mut used = HashSet::new();
        for (slot, guid) in matl.bound_textures() {
            cx.check_cancelled()?;
            let mut name = slot_names.get(&slot).map_or_else(|| format!("slot_{slot}"), |n| file_safe(n));
            if !used.insert(name.clone()) {
                name = format!("{name}_{slot}");
                used.insert(name.clone());
            }
            let converted = async {
                let asset = locate(guid, "txtr", &origin, cx).await?;
                cx.mediator.convert(asset, texture_target.clone(), cx).await
            }
            .await;
            let file = match converted {
                Ok(value) => {
                    let ext = cx.mediator.extension_of(value.format().name()).unwrap_or(value.format().name()).to_owned();
                    let base = format!("textures/{name}");
                    if let Some(bundle) = value.downcast_ref::<BundleData>() {
                        for (n, item) in bundle.names.iter().zip(&bundle.items) {
                            items.push((format!("{base}/{n}"), item.clone()));
                        }
                        Some(base)
                    } else if value.as_blob().is_some() {
                        let path = format!("{base}.{ext}");
                        items.push((path.clone(), value));
                        Some(path)
                    } else {
                        cx.warn(format!("material {label}: texture {} did not produce a file", hex(guid)));
                        None
                    }
                }
                Err(e) if is_cancelled(&e) => return Err(e),
                Err(e) => {
                    cx.warn(format!("material {label}: texture {} ({name}) is skipped: {e}", hex(guid)));
                    None
                }
            };
            textures.insert(name, json!({ "slot": slot, "guid": hex(guid), "file": file }));
        }

        let h = &matl.header;
        let document = json!({
            "name": matl.name,
            "guid": hex(h.guid),
            "version": h.version.to_string(),
            "width": h.size[0],
            "height": h.size[1],
            "depth": h.size[2],
            "shaderSet": hex(h.shader_set),
            "surfaceProp": matl.surface_props[0],
            "surfaceProp2": matl.surface_props[1],
            "glueFlags": format!("0x{:08X}", h.glue[0]),
            "glueFlags2": format!("0x{:08X}", h.glue[1]),
            "samplers": format!("0x{:08X}", u32::from_le_bytes(h.samplers)),
            "features": format!("0x{:08X}", h.features),
            "depthShadowMaterial": hex(h.depth[0]),
            "depthPrepassMaterial": hex(h.depth[1]),
            "depthVSMMaterial": hex(h.depth[2]),
            "depthShadowTightMaterial": hex(h.depth[3]),
            "colpassMaterial": hex(h.depth[4]),
            "textures": textures,
        });
        let bytes = serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("matl.json", e))?;

        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("matl.json"), options.normalized());
        let json_format = FormatId::new("matl.json");
        let main: Value = Arc::new(BlobData::with_format(json_format, Bytes::from(bytes), key.clone()));
        items.insert(0, ("material.json".to_owned(), main));
        Ok(Arc::new(BundleData::named(items, key)))
    }
}
