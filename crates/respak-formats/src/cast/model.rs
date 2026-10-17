//! `Mesh -> cast`.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Mesh, Skeleton};
use respak_core::value::{BlobData, BundleData};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde_json::Value as Json;

use super::{custom, file_safe, stem_of, CastOptions, CastWriter};

/// Capability edge `Mesh -> cast`: a composite, `<bodypart>_<i>_LOD<n>.cast` per (LOD, model)
/// plus `textures/<material>/<slot>.<ext>`.
pub struct CastEncode;

#[async_trait]
impl ConvertHandler for CastEncode {
    /// Reads the requested models, converts the textures through the mediator, and writes the
    /// files. Checks cancellation per model and per texture. A model without geometry is skipped;
    /// a mesh with no LODs at all gives `<stem>.cast` with the skeleton only. A texture that cannot
    /// be converted is left out with a warning.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<CastOptions>::parse(target)?;
        let opts = options.options();
        let mesh: &dyn Mesh = source
            .as_mesh()
            .ok_or_else(|| custom(format!("`{}` has no Mesh capability", source.format())))?;
        let skeleton: &dyn Skeleton = source
            .as_skeleton()
            .ok_or_else(|| custom(format!("`{}` has no Skeleton capability", source.format())))?;
        let bones = skeleton.bones();
        let info = mesh.info();
        if opts.skin >= info.skin_count {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!(
                    "skin {}: the model has {} skin(s)",
                    opts.skin, info.skin_count
                ),
            }));
        }
        let lods = if info.lod_count == 0 {
            Vec::new()
        } else {
            opts.lods.resolve(info.lod_count)?
        };
        let stem = source
            .as_metadata()
            .and_then(|m| m.name())
            .map_or_else(|| "model".to_owned(), stem_of);

        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("cast"),
            options.normalized(),
        );
        let cast_value = |bytes: Vec<u8>| -> Value {
            Arc::new(BlobData::with_format(
                FormatId::new("cast"),
                Bytes::from(bytes),
                key.clone(),
            ))
        };
        let mut items: Vec<(String, Value)> = Vec::new();

        if lods.is_empty() {
            let mut writer = CastWriter::new();
            writer.model(&stem, bones);
            items.push((format!("{stem}.cast"), cast_value(writer.finish())));
            return Ok(Arc::new(BundleData::named(items, key.clone())));
        }

        // Materials and their textures: each texture is converted once and shared by name.
        let materials = mesh.materials(opts.skin, cx).await?;
        let image = Target {
            format: FormatId::new(opts.image.clone()),
            options: Json::Null,
            via: Vec::new(),
        };
        let mut texture_paths: BTreeMap<u64, String> = BTreeMap::new();
        if opts.textures {
            for material in &materials {
                for texture in &material.textures {
                    cx.check_cancelled()?;
                    if texture_paths.contains_key(&texture.guid) {
                        continue;
                    }
                    let Some(value) = &texture.texture else {
                        continue;
                    };
                    match cx.mediator.convert(value.clone(), image.clone(), cx).await {
                        Ok(converted) => {
                            let ext = cx
                                .mediator
                                .extension_of(converted.format().name())
                                .unwrap_or(converted.format().name())
                                .to_owned();
                            if converted.as_blob().is_some() {
                                let path = format!(
                                    "textures/{}/{}.{ext}",
                                    file_safe(&material.name),
                                    file_safe(&texture.slot)
                                );
                                items.push((path.clone(), converted));
                                texture_paths.insert(texture.guid, path);
                            } else {
                                cx.warn(format!(
                                    "material {}: texture {:#018x} did not produce a file",
                                    material.name, texture.guid
                                ));
                            }
                        }
                        Err(e) if e.is_cancelled() => return Err(e),
                        Err(e) => cx.warn(format!(
                            "material {}: texture {:#018x} ({}) is skipped: {e}",
                            material.name, texture.guid, texture.slot
                        )),
                    }
                }
            }
        }
        let files: Vec<(u64, String)> = texture_paths.into_iter().collect();

        for &lod in &lods {
            for (model, model_info) in info.models.iter().enumerate() {
                cx.check_cancelled()?;
                let data = mesh.read(lod, model as u32, opts.skin, cx).await?;
                if data.parts.is_empty() {
                    continue;
                }
                let name = format!(
                    "{}_{}_LOD{lod}",
                    file_safe(&model_info.bodypart),
                    model_info.index
                );
                let mut writer = CastWriter::new();
                writer.model(&name, bones);
                for material in materials
                    .iter()
                    .filter(|m| data.parts.iter().any(|p| p.material == m.guid))
                {
                    writer.material(material, &files);
                }
                for part in &data.parts {
                    let material_name = materials
                        .iter()
                        .find(|m| m.guid == part.material)
                        .map_or_else(|| format!("{:#018x}", part.material), |m| m.name.clone());
                    writer.mesh(&format!("{name}_{material_name}"), part, bones.len())?;
                }
                items.push((format!("{name}.cast"), cast_value(writer.finish())));
            }
        }
        Ok(Arc::new(BundleData::named(items, key.clone())))
    }
}
