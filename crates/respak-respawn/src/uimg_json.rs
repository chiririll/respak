//! `uimg -> uimg.json`: a UI image atlas with its sprites.
//!
//! The result is a bundle the engine writes into the atlas's own folder: `atlas.<ext>` (the atlas
//! texture converted through the mediator to the format of the `image` option, default `png`),
//! `sprites/<sprite path>.<ext>` for every sprite when the `sprites` option is on (default off),
//! and `uimg.json`: `name`, `guid`, `version`, `width`, `height`, `widthRatio`, `heightRatio`,
//! `atlas` (`guid`, `file`), `unkCount` and `sprites`, in stored order. A sprite has `index`,
//! `path`, `pathHash`, `x`, `y`, `width`, `height` (its pixels in the atlas, as rsx cuts them),
//! `uv`, `renderSize`, `layout` and `file`. A `file` is `null` when it was not written; a warning
//! says why. Guids and hashes are `0x`-prefixed hex.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::value::{BlobData, BundleData};
use respak_core::{ConvertHandler, Cx, Data, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::matl_json::{hex, TextureTarget};
use crate::uimg::{Rect, Sprite, UimgAsset};

/// Options of the `uimg.json` target: `{"type":"uimg.json","image":{"type":"dds"},"sprites":true}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UimgJsonOptions {
    /// What the atlas and the sprites are converted to; default `png`.
    pub image: TextureTarget,
    /// Whether every sprite is cut out as its own image; default off.
    pub sprites: bool,
}

/// Convert edge `uimg -> uimg.json`.
pub struct UimgJsonEncode;

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom { source: None, message: message.into() })
}

/// A cut-out sprite, for the image encoders.
#[derive(Debug)]
struct SpriteImage {
    format: FormatId,
    key: ValueKey,
    bitmap: BitmapBuf,
}

impl Data for SpriteImage {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(&self.bitmap)
    }
}

/// A sprite path made safe for a relative file path: no empty, `.` or `..` parts, no characters
/// Windows refuses. `None` when nothing is left.
fn safe_path(path: &str) -> Option<String> {
    let parts: Vec<String> = path
        .split(['/', '\\'])
        .filter(|p| !matches!(*p, "" | "." | ".."))
        .map(|p| p.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Rows `rect` of `rgba` (`width` pixels wide) as one buffer.
fn crop(rgba: &[u8], width: u32, rect: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for row in rect.y..rect.y + rect.height {
        let at = (row as usize * width as usize + rect.x as usize) * 4;
        out.extend_from_slice(&rgba[at..at + rect.width as usize * 4]);
    }
    out
}

/// `rect` of the header's atlas in the atlas texture of `w` x `h` pixels, cut to the texture.
fn scaled(rect: Rect, header: (u32, u32), tex: (u32, u32)) -> Option<Rect> {
    let sx = |v: u32| (u64::from(v) * u64::from(tex.0)).div_ceil(u64::from(header.0)) as u32;
    let sy = |v: u32| (u64::from(v) * u64::from(tex.1)).div_ceil(u64::from(header.1)) as u32;
    let (x, y) = (sx(rect.x), sy(rect.y));
    let (width, height) = (sx(rect.width).min(tex.0.saturating_sub(x)), sy(rect.height).min(tex.1.saturating_sub(y)));
    (width != 0 && height != 0).then_some(Rect { x, y, width, height })
}

/// Puts a converted image into the bundle at `base`; the name of the file, or `None`.
fn place(value: &Value, base: &str, items: &mut Vec<(String, Value)>, cx: &Cx) -> Option<String> {
    if let Some(bundle) = value.downcast_ref::<BundleData>() {
        for (n, item) in bundle.names.iter().zip(&bundle.items) {
            items.push((format!("{base}/{n}"), item.clone()));
        }
        return Some(base.to_owned());
    }
    value.as_blob()?;
    let ext = cx.mediator.extension_of(value.format().name()).unwrap_or(value.format().name()).to_owned();
    let path = format!("{base}.{ext}");
    items.push((path.clone(), value.clone()));
    Some(path)
}

fn layout(s: &Sprite) -> Json {
    let l = s.layout;
    json!({
        "cropInsetLeft": l[0], "cropInsetTop": l[1],
        "endAnchorX": l[2], "endAnchorY": l[3],
        "startAnchorX": l[4], "startAnchorY": l[5],
        "scaleRatioX": l[6], "scaleRatioY": l[7],
    })
}

#[async_trait]
impl ConvertHandler for UimgJsonEncode {
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let uimg = source.downcast_ref::<UimgAsset>().ok_or_else(|| custom("uimg.json must be made from a uimg"))?;
        let options = TypedTarget::<UimgJsonOptions>::parse(target)?;
        let image = &options.options().image;
        let image_target = Target {
            format: FormatId::new(image.format.clone()),
            options: if image.options.is_empty() { Json::Null } else { Json::Object(image.options.clone()) },
            via: Vec::new(),
        };
        let h = uimg.header();
        let label = hex(uimg.guid());
        let mut items: Vec<(String, Value)> = Vec::new();

        // The atlas.
        let atlas = match cx.mediator.convert(source.clone(), image_target.clone(), cx).await {
            Ok(value) => {
                let file = place(&value, "atlas", &mut items, cx);
                if file.is_none() {
                    cx.warn(format!("uimg {label}: the atlas did not produce a file"));
                }
                file
            }
            Err(e) if matches!(e.kind(), ErrorKind::Cancelled) => return Err(e),
            Err(e) => {
                cx.warn(format!("uimg {label}: the atlas {} is skipped: {e}", hex(h.atlas_guid)));
                None
            }
        };

        // The sprites, cut from one decode of the atlas.
        let pixels = if options.options().sprites {
            match uimg.atlas_rgba().await {
                Ok(p) => Some(p),
                Err(e) if matches!(e.kind(), ErrorKind::Cancelled) => return Err(e),
                Err(e) => {
                    cx.warn(format!("uimg {label}: the sprites are skipped: the atlas {} is not available ({e})", hex(h.atlas_guid)));
                    None
                }
            }
        } else {
            None
        };
        if let Some((w, ht, _)) = &pixels {
            if (*w, *ht) != (h.width, h.height) {
                cx.warn(format!("uimg {label}: the atlas texture is {w}x{ht} but the header says {}x{}; sprites are scaled", h.width, h.height));
            }
        }

        let mut used = HashSet::new();
        let mut sprites = Vec::with_capacity(uimg.sprites().len());
        for (index, s) in uimg.sprites().iter().enumerate() {
            cx.check_cancelled()?;
            let rect = s.rect(h.width, h.height);
            let mut file = None;
            if let Some((tw, th, rgba)) = &pixels {
                let cut = rect.and_then(|r| scaled(r, (h.width, h.height), (*tw, *th)));
                match cut {
                    None => cx.warn(format!("uimg {label}: sprite {index} ({}) is empty and is skipped", s.path)),
                    Some(cut) => {
                        let mut path = safe_path(&s.path).unwrap_or_else(|| format!("0x{:08X}", s.hash));
                        if !used.insert(path.to_lowercase()) {
                            path = format!("{path}_{index}");
                            used.insert(path.to_lowercase());
                        }
                        let info = BitmapInfo {
                            width: cut.width,
                            height: cut.height,
                            depth: 1,
                            array_len: 1,
                            faces: 1,
                            mip_count: 1,
                            format: PixelFormat::Rgba8,
                        };
                        let bitmap = BitmapBuf::new(info, vec![vec![Bytes::from(crop(rgba, *tw, cut))]])?;
                        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("uimg.sprite"), &json!({ "index": index }));
                        let sprite: Value = Arc::new(SpriteImage { format: FormatId::new("uimg.sprite"), key, bitmap });
                        match cx.mediator.convert(sprite, image_target.clone(), cx).await {
                            Ok(value) => {
                                file = place(&value, &format!("sprites/{path}"), &mut items, cx);
                                if file.is_none() {
                                    cx.warn(format!("uimg {label}: sprite {index} ({}) did not produce a file", s.path));
                                }
                            }
                            Err(e) if matches!(e.kind(), ErrorKind::Cancelled) => return Err(e),
                            Err(e) => cx.warn(format!("uimg {label}: sprite {index} ({}) is skipped: {e}", s.path)),
                        }
                    }
                }
            }
            let (x, y, width, height) = rect.map_or((0, 0, 0, 0), |r| (r.x, r.y, r.width, r.height));
            sprites.push(json!({
                "index": index,
                "path": s.path,
                "pathHash": format!("0x{:08X}", s.hash),
                "x": x, "y": y, "width": width, "height": height,
                "uv": { "minX": s.uv[0], "minY": s.uv[1], "sizeX": s.uv[2], "sizeY": s.uv[3] },
                "renderSize": { "width": s.render[0], "height": s.render[1] },
                "layout": layout(s),
                "file": file,
            }));
        }

        let document = json!({
            "name": Json::Null,
            "guid": label,
            "version": h.version,
            "width": h.width,
            "height": h.height,
            "widthRatio": h.width_ratio,
            "heightRatio": h.height_ratio,
            "atlas": { "guid": hex(h.atlas_guid), "file": atlas },
            "unkCount": h.unk_count,
            "sprites": sprites,
        });
        let bytes = serde_json::to_vec_pretty(&document).map_err(|e| Error::custom("uimg.json", e))?;

        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("uimg.json"), options.normalized());
        let main: Value = Arc::new(BlobData::with_format(FormatId::new("uimg.json"), Bytes::from(bytes), key.clone()));
        items.insert(0, ("uimg.json".to_owned(), main));
        Ok(Arc::new(BundleData::named(items, key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_cannot_leave_the_folder() {
        assert_eq!(safe_path("../../a/./b\\c").as_deref(), Some("a/b/c"));
        assert_eq!(safe_path("/rui/icon:1").as_deref(), Some("rui/icon_1"));
        assert_eq!(safe_path("../.."), None);
    }

    #[test]
    fn rects_follow_the_texture_size() {
        let r = Rect { x: 4, y: 8, width: 8, height: 8 };
        assert_eq!(scaled(r, (64, 64), (64, 64)), Some(r));
        assert_eq!(scaled(r, (64, 64), (128, 128)), Some(Rect { x: 8, y: 16, width: 16, height: 16 }));
        // Cut at the texture's edge.
        assert_eq!(scaled(Rect { x: 60, y: 0, width: 8, height: 8 }, (64, 64), (64, 64)), Some(Rect { x: 60, y: 0, width: 4, height: 8 }));
        assert_eq!(scaled(Rect { x: 70, y: 0, width: 8, height: 8 }, (64, 64), (64, 64)), None);
    }
}
