//! `stgs -> stgs.json`: a settings object.
//!
//! `name`, `guid`, `version`, `layout` (guid of the `stlt`), `layoutName` (`null` when the layout
//! was not found), `uniqueId`, `settings` (the values by field name; `null` when the layout was
//! not found; `float2` / `float3` are arrays, arrays of any kind are arrays of objects),
//! `values` (the value buffer as hex), `modFlags`, `modNames` and `modValues` (`index`, `name`,
//! `type`, `offset` into the buffer, `raw`, `value`: an int, float, bool or string, `null` for
//! `number` and unknown types, which depend on the field). Guids are `0x`-prefixed hex.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::Metadata;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::matl_json::hex;
use crate::stgs::{ModType, StgsAsset, StgsMod};

/// The target takes no options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOptions {}

/// Convert edge `stgs -> stgs.json`.
pub struct StgsJsonEncode;

fn mod_value(m: &StgsMod) -> Json {
    match m.kind {
        Some(ModType::IntAdd | ModType::IntMultiply) => json!(m.raw as i32),
        Some(ModType::FloatAdd | ModType::FloatMultiply) => json!(f32::from_bits(m.raw)),
        Some(ModType::Bool) => json!(m.raw & 0xFF != 0),
        Some(ModType::String) => json!(m.string),
        Some(ModType::Number) | None => Json::Null,
    }
}

#[async_trait]
impl ConvertHandler for StgsJsonEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let stgs = source.downcast_ref::<StgsAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "stgs.json must be made from a stgs".into(),
            })
        })?;
        let options = TypedTarget::<NoOptions>::parse(target)?;
        let h = stgs.header();
        let values: String = stgs.values().iter().map(|b| format!("{b:02x}")).collect();
        let mods: Vec<Json> = stgs
            .mods()
            .iter()
            .map(|m| {
                json!({
                    "index": m.name_index,
                    "name": stgs.mod_names().get(usize::from(m.name_index)),
                    "type": m.kind.map(ModType::name),
                    "offset": m.value_offset,
                    "raw": format!("0x{:08X}", m.raw),
                    "value": mod_value(m),
                })
            })
            .collect();
        let document = json!({
            "name": Metadata::name(stgs),
            "guid": hex(stgs.guid()),
            "version": h.version,
            "layout": hex(h.layout),
            "layoutName": stgs.layout_name(),
            "uniqueId": h.unique_id,
            "settings": stgs.settings(),
            "values": values,
            "modFlags": h.single_player_mods,
            "modNames": stgs.mod_names(),
            "modValues": mods,
        });
        let bytes =
            serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("stgs.json", e))?;
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("stgs.json"),
            options.normalized(),
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("stgs.json"),
            Bytes::from(bytes),
            key,
        )))
    }
}
