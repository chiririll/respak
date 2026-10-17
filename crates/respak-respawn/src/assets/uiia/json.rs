//! `uiia -> uiia.json`: a UI image with its description.
//!
//! The result is a bundle the engine writes into the image's own folder: `image.<ext>` (the
//! image converted through the mediator to the format of the `image` option, default `png`) and
//! `uiia.json`:
//! `name`, `guid`, `version`, `width`, `height` (of the image written), `quality` (`high` from the
//! starpak, `low` from the pak), `lowResolution` / `highResolution` (`width`, `height`, `tilesX`,
//! `tilesY`), `streamedSize`, `compression`, `canvas` (`width`, `height`, `origin`, `extent` as a fraction of the
//! canvas, `texelOffset`, `scale`: where the image sits on its virtual canvas), `flags` and `image` (the file). Guids are
//! `0x`-prefixed hex.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Bitmap, Metadata};
use respak_core::value::{BlobData, BundleData};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::matl::TextureTarget;
use crate::uiia::{Quality, Resolution, UiiaAsset, UiiaHeader};

/// Options of the `uiia.json` target: `{"type":"uiia.json","image":{"type":"dds"}}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiiaJsonOptions {
    /// What the image is converted to; default `png`.
    pub image: TextureTarget,
}

/// Convert edge `uiia -> uiia.json`.
pub struct UiiaJsonEncode;

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom {
        source: None,
        message: message.into(),
    })
}

fn resolution(r: &Resolution) -> Json {
    json!({ "width": r.width, "height": r.height, "tilesX": r.tiles_x, "tilesY": r.tiles_y })
}

/// The virtual canvas the image is placed on (see `UiiaHeader::layout`), in pixels of the high
/// resolution; `null` when the header carries no layout.
fn canvas(h: &UiiaHeader) -> Json {
    let l = h.layout.map(f64::from);
    if l[6] <= 0.0 || l[7] <= 0.0 {
        return Json::Null;
    }
    let (w, ht) = (f64::from(h.high.width), f64::from(h.high.height));
    json!({
        "width": (l[6] * (w - 1.0)).round(),
        "height": (l[7] * (ht - 1.0)).round(),
        "origin": [l[0], l[1]],
        "extent": [l[2], l[3]],
        "texelOffset": [l[4], l[5]],
        "scale": [l[6], l[7]],
    })
}

#[async_trait]
impl ConvertHandler for UiiaJsonEncode {
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let uiia = source
            .downcast_ref::<UiiaAsset>()
            .ok_or_else(|| custom("uiia.json must be made from a uiia"))?;
        let options = TypedTarget::<UiiaJsonOptions>::parse(target)?;
        let image_target = Target {
            format: FormatId::new(options.options().image.format.clone()),
            options: if options.options().image.options.is_empty() {
                Json::Null
            } else {
                Json::Object(options.options().image.options.clone())
            },
            via: Vec::new(),
        };

        let image = cx
            .mediator
            .convert(source.clone(), image_target, cx)
            .await?;
        let ext = cx
            .mediator
            .extension_of(image.format().name())
            .unwrap_or(image.format().name())
            .to_owned();
        let mut items: Vec<(String, Value)> = Vec::new();
        let file = if let Some(bundle) = image.downcast_ref::<BundleData>() {
            for (n, item) in bundle.names.iter().zip(&bundle.items) {
                items.push((format!("image/{n}"), item.clone()));
            }
            "image".to_owned()
        } else if image.as_blob().is_some() {
            let path = format!("image.{ext}");
            items.push((path.clone(), image));
            path
        } else {
            return Err(custom("the image did not produce a file"));
        };

        let h = uiia.header();
        let info = uiia.info();
        let document = json!({
            "name": uiia.name(),
            "guid": format!("0x{:016X}", uiia.guid()),
            "version": h.version.to_string(),
            "width": info.width,
            "height": info.height,
            "quality": if uiia.quality() == Quality::High { "high" } else { "low" },
            "lowResolution": resolution(&h.low),
            "highResolution": resolution(&h.high),
            "streamedSize": h.streamed_size,
            "compression": h.codec(),
            "canvas": canvas(h),
            "flags": format!("0x{:02X}", h.flags),
            "image": file,
        });
        let bytes =
            serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("uiia.json", e))?;

        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("uiia.json"),
            options.normalized(),
        );
        let main: Value = Arc::new(BlobData::with_format(
            FormatId::new("uiia.json"),
            Bytes::from(bytes),
            key.clone(),
        ));
        items.insert(0, ("uiia.json".to_owned(), main));
        Ok(Arc::new(BundleData::named(items, key)))
    }
}
