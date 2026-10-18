//! `subt -> subt.txt`, the rsx text export. CSV and JSON come from `table.csv` / `table.json`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};

use super::SubtAsset;

/// The target takes no options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOptions {}

/// Convert edge `subt -> subt.txt`: the text of each line, one per row; a newline inside a text
/// is written as `\n` so the rows match the entries.
pub struct SubtTxtEncode;

#[async_trait]
impl ConvertHandler for SubtTxtEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let subt = source.downcast_ref::<SubtAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "subt.txt must be made from a subt".into(),
            })
        })?;
        let options = TypedTarget::<NoOptions>::parse(target)?;
        let mut out = String::new();
        for e in subt.entries() {
            out += &e.text.replace('\r', "\\r").replace('\n', "\\n");
            out.push('\n');
        }
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("subt.txt"),
            options.normalized(),
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("subt.txt"),
            Bytes::from(out.into_bytes()),
            key,
        )))
    }
}
