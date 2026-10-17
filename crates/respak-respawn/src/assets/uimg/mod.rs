//! `uimg`: the Respawn UI image atlas asset (`'gmiu'`), read from `rpak.asset`.
//!
//! Ported from rsx `src/game/rtech/assets/ui_image_atlas.{h,cpp}` (`UIImageAtlasAssetHeader_v10_t`,
//! `LoadUIImageAtlasAsset`, `PostLoadUIImageAtlasAsset`). Asset versions 10 and 11 differ only in
//! the stride of the sprite hash table.
//!
//! # Header (0x40 bytes)
//! | field | at |
//! |---|---|
//! | width / height ratio (f32) | 0x00 / 0x04 |
//! | width, height, sprite count, `unk` count (u16) | 0x08, 0x0A, 0x0C, 0x0E |
//! | offsets, dimensions, `unk`, hashes, names (pointers) | 0x10, 0x18, 0x20, 0x28, 0x30 |
//! | atlas texture guid | 0x38 |
//!
//! The pointers arrive as `.mem` offsets. Per sprite there are 32 bytes of layout floats, a
//! 4-byte render size, a hash entry (8 bytes in v10, 12 in v11) and, in the asset's CPU data, 16
//! bytes of bounds: the sprite's rectangle in the atlas as fractions of its size.
//!
//! The atlas pixels are a separate `txtr`, found by guid when the bitmap is asked for.

mod json;

pub use json::{UimgJsonEncode, UimgJsonOptions};

mod read;
pub use read::UimgRead;

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Bitmap, BitmapInfo, Dependency, Metadata, PixelFormat};
use respak_core::reader::Reader;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};
use respak_formats::decode_rgba8;

use crate::matl::{locate, pak_of};
use crate::rpak::{RpakAsset, MEM};

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("uimg", version)
}

const HEADER_LEN: usize = 0x40;
const OFFSET_LEN: u64 = 32;
const DIMENSION_LEN: u64 = 4;
const BOUNDS_LEN: u64 = 16;
/// The longest sprite path read from the names blob (as `txtr::read_name`).
const NAME_MAX: u64 = 512;

/// The decoded header of a UI image atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct UimgHeader {
    /// `rpak.asset` version: 10 or 11.
    pub version: u32,
    /// Probably `1 / width`; rsx never reads it.
    pub width_ratio: f32,
    /// Probably `1 / height`.
    pub height_ratio: f32,
    /// Atlas width in pixels (the sprite bounds are fractions of it).
    pub width: u32,
    /// Atlas height in pixels.
    pub height: u32,
    /// Number of sprites.
    pub count: u32,
    /// Number of entries at the unknown array.
    pub unk_count: u32,
    /// Guid of the atlas `txtr`.
    pub atlas_guid: u64,
    offsets: Option<u64>,
    dimensions: Option<u64>,
    hashes: Option<u64>,
    names: Option<u64>,
}

impl UimgHeader {
    /// Parses the relocated 0x40-byte header of the given asset `version`.
    pub fn parse(version: u32, h: &[u8]) -> Result<Self> {
        if !(10..=11).contains(&version) {
            return Err(unsupported(version.to_string()));
        }
        if h.len() < HEADER_LEN {
            return Err(Error::corrupt(format!(
                "uimg header is {} bytes, expected {HEADER_LEN}",
                h.len()
            )));
        }
        // Null is all ones after relocation.
        let r = Reader::new("uimg", h);
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = r.u64(at)?;
            Ok((v != u64::MAX).then_some(v))
        };
        let header = Self {
            version,
            width_ratio: r.f32(0)?,
            height_ratio: r.f32(4)?,
            width: u32::from(r.u16(0x08)?),
            height: u32::from(r.u16(0x0A)?),
            count: u32::from(r.u16(0x0C)?),
            unk_count: u32::from(r.u16(0x0E)?),
            atlas_guid: r.u64(0x38)?,
            offsets: ptr(0x10)?,
            dimensions: ptr(0x18)?,
            hashes: ptr(0x28)?,
            // A names pointer of 0 is a slot that was never a pointer.
            names: ptr(0x30)?.filter(|&v| v != 0),
        };
        if header.width == 0 || header.height == 0 {
            return Err(Error::corrupt("uimg has a zero dimension"));
        }
        Ok(header)
    }

    /// Bytes of one entry of the hash table.
    fn hash_stride(&self) -> u64 {
        if self.version == 10 {
            8
        } else {
            12
        }
    }
}

/// One sprite of the atlas, in stored order.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    /// The sprite's path, or `0x<hash>` when the asset carries no names.
    pub path: String,
    /// Whether `path` comes from the names blob.
    pub named: bool,
    /// The stored hash of the path (how it is computed is unknown).
    pub hash: u32,
    /// Rectangle in the atlas as fractions of its size: min x, min y, size x, size y.
    pub uv: [f32; 4],
    /// Layout floats, as stored (rsx's names: crop inset left / top, end anchor x / y, start
    /// anchor x / y, scale ratio x / y).
    pub layout: [f32; 8],
    /// Size the game draws the sprite at.
    pub render: [u16; 2],
}

/// A sprite's rectangle in whole pixels of an atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Sprite {
    /// The sprite's rectangle in an atlas of `width` x `height` pixels. The position is truncated
    /// and the size rounded, as rsx does; `None` for an empty sprite.
    pub fn rect(&self, width: u32, height: u32) -> Option<Rect> {
        let [min_x, min_y, size_x, size_y] = self.uv;
        let rect = Rect {
            x: u32::from((min_x * width as f32) as u16),
            y: u32::from((min_y * height as f32) as u16),
            width: u32::from((size_x * width as f32 + 0.5) as u16),
            height: u32::from((size_y * height as f32 + 0.5) as u16),
        };
        (rect.width != 0 && rect.height != 0).then_some(rect)
    }
}

/// A UI image atlas: `Data` + `Bitmap` + `Metadata`.
pub struct UimgAsset {
    format: FormatId,
    key: ValueKey,
    header: UimgHeader,
    sprites: Vec<Sprite>,
    guid: u64,
    info: BitmapInfo,
    /// The pak the asset is in, where the atlas texture is looked for first.
    origin: String,
    cx: Cx,
}

impl std::fmt::Debug for UimgAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UimgAsset")
            .field("key", &self.key)
            .field("header", &self.header)
            .field("sprites", &self.sprites.len())
            .finish()
    }
}

impl UimgAsset {
    /// The parsed header.
    pub fn header(&self) -> &UimgHeader {
        &self.header
    }

    /// The sprites in stored order.
    pub fn sprites(&self) -> &[Sprite] {
        &self.sprites
    }

    /// The asset's guid.
    pub fn guid(&self) -> u64 {
        self.guid
    }

    /// The atlas texture's mip 0 as RGBA8, with its own size (which can differ from the header's).
    pub(crate) async fn atlas_rgba(&self) -> Result<(u32, u32, Vec<u8>)> {
        let asset = locate(self.header.atlas_guid, "txtr", &self.origin, &self.cx).await?;
        let texture = self.cx.mediator.identify(asset, &self.cx).await?;
        let bitmap = texture.as_bitmap().ok_or_else(|| {
            Error::corrupt(format!(
                "the atlas {:#018x} is not a texture",
                self.header.atlas_guid
            ))
        })?;
        let info = bitmap.info();
        let raw = bitmap.read_mip(0, 0).await?;
        Ok((
            info.width,
            info.height,
            decode_rgba8(info.format, info.width, info.height, &raw)?,
        ))
    }
}

/// `src` (`sw` x `sh` RGBA8) resampled to `dw` x `dh` with the nearest pixel.
fn resize(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(dw as usize * dh as usize * 4);
    for y in 0..dh {
        let sy = (u64::from(y) * u64::from(sh) / u64::from(dh)) as usize;
        for x in 0..dw {
            let sx = (u64::from(x) * u64::from(sw) / u64::from(dw)) as usize;
            let at = (sy * sw as usize + sx) * 4;
            out.extend_from_slice(&src[at..at + 4]);
        }
    }
    out
}

#[async_trait]
impl Bitmap for UimgAsset {
    fn info(&self) -> BitmapInfo {
        self.info.clone()
    }

    /// The atlas texture, at the size the header gives (the texture can be smaller when its
    /// streamed mips are missing).
    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes> {
        if level != 0 || layer != 0 {
            return Err(Error::new(ErrorKind::NotFound {
                path: format!("mip {level} of layer {layer}"),
            }));
        }
        let (w, h, rgba) = self.atlas_rgba().await?;
        if (w, h) == (self.header.width, self.header.height) {
            return Ok(Bytes::from(rgba));
        }
        self.cx.warn(format!(
            "uimg {:#018x}: the atlas texture is {w}x{h} but the header says {}x{}; it is resized",
            self.guid, self.header.width, self.header.height
        ));
        Ok(Bytes::from(resize(
            &rgba,
            w,
            h,
            self.header.width,
            self.header.height,
        )))
    }
}

impl Metadata for UimgAsset {
    fn name(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> Option<&str> {
        Some("uimg")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![Dependency {
            guid: self.header.atlas_guid,
            name: None,
        }]
    }
}

impl Data for UimgAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

#[cfg(test)]
mod tests;
