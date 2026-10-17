//! `rmdl -> rmdl.json`: everything a model says about itself, as one JSON file (the "metadata"
//! export). No geometry and no textures; use `cast` for those.
//!
//! The document is built by [`RmdlAsset::describe`]: `name`, `studioName`, `guid`, `version`,
//! `flags`, `bounds`, `streamingSize`, `bones`, `bodyparts` (with models and meshes), `lods`
//! (switch distances), `vertexGroups`, `materials` (the model's table), `skins` (material guids
//! per family), `hitboxSets`, `attachments`, `animRigs`, `animSeqs`. Guids are `0x`-prefixed hex,
//! like `matl.json`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};

use crate::rmdl::RmdlAsset;

/// Options of the `rmdl.json` target: none yet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RmdlJsonOptions {}

/// Convert edge `rmdl -> rmdl.json`.
pub struct RmdlJsonEncode;

#[async_trait]
impl ConvertHandler for RmdlJsonEncode {
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let model = source.downcast_ref::<RmdlAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "rmdl.json must be made from an rmdl".into(),
            })
        })?;
        let options = TypedTarget::<RmdlJsonOptions>::parse(target)?;
        cx.check_cancelled()?;
        let document = model.describe().await?;
        let bytes =
            serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("rmdl.json", e))?;
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("rmdl.json"),
            options.normalized(),
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("rmdl.json"),
            Bytes::from(bytes),
            key,
        )))
    }
}
