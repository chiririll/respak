//! `stlt -> stlt.json`: a settings layout.
//!
//! `name`, `guid`, `layout`: `elementCount`, `totalSize`, `extraDataSizeIndex`, `alignment` and
//! `fields` (`name`, `type`, `offset`, `help`, and `layout` for arrays: the same object again).

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::guid::hex;
use crate::stlt::{FieldType, Layout, StltAsset};

/// The target takes no options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOptions {}

/// Convert edge `stlt -> stlt.json`.
pub struct StltJsonEncode;

fn layout_json(l: &Layout) -> Json {
    let fields: Vec<Json> = l
        .fields
        .iter()
        .map(|f| {
            let mut field = json!({
                "name": f.name,
                "type": f.ty.name(),
                "offset": f.offset,
                "help": f.help,
            });
            if matches!(f.ty, FieldType::Array | FieldType::DynamicArray) {
                field["layout"] = l
                    .subs
                    .get(usize::from(f.sub))
                    .map_or(Json::Null, layout_json);
            }
            field
        })
        .collect();
    json!({
        "elementCount": l.element_count,
        "totalSize": l.total_size,
        "extraDataSizeIndex": l.extra_data_size_index,
        "alignment": l.alignment,
        "fields": fields,
    })
}

#[async_trait]
impl ConvertHandler for StltJsonEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let stlt = source.downcast_ref::<StltAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "stlt.json must be made from a stlt".into(),
            })
        })?;
        let options = TypedTarget::<NoOptions>::parse(target)?;
        let document = json!({
            "name": stlt.layout().name,
            "guid": hex(stlt.guid()),
            "layout": layout_json(stlt.layout()),
        });
        let bytes =
            serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("stlt.json", e))?;
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("stlt.json"),
            options.normalized(),
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("stlt.json"),
            Bytes::from(bytes),
            key,
        )))
    }
}
