//! `uiia`: the Respawn UI image asset (`'aiiu'`), read from `rpak.asset`.
//!
//! Ported from rsx `src/game/rtech/assets/ui_image.{h,cpp}` (`UIImageAssetHeader_v2_t`,
//! `UIImageAssetData_v2_t`, `LoadUIImageAsset`, `DoTilingWork`, `CreateTextureForImage`). Only
//! version 2 exists. This is one tiled image, not an atlas (that is `'gmiu'`).
//!
//! # Header (0x40 bytes) and CPU data (0x40 bytes)
//! | field | at |
//! |---|---|
//! | streamed size in 4096-byte units (u16) | header 0x10 |
//! | flags (u8) | header 0x12 |
//! | high / low resolution width, height (u16) | CPU 0x20 / 0x24 |
//! | data pointer, name pointer | CPU 0x28, 0x30 |
//!
//! Flags: bit 0 / 1 = the low-resolution tile table has BC1 / BC7 tiles, bit 2 / 3 the same for
//! the high resolution, bits 4-5 = the codec of the streamed data (1 RTech, 2 Snowflake, 3
//! Oodle), bit 6 / 7 = no extra pixel in width / height.
//!
//! # Tiles
//! The image is cut into tiles of 32x32 pixels of which 31x31 are shown (the border pixel is
//! shared with the neighbour). With `off` = 2 unless the size bit is set, a dimension of `n`
//! pixels takes `(n + off + 29) / 31` tiles. A tile is 512 bytes of BC1 or 1024 of BC7. The
//! table (one u32 per tile: offset in 24 bits, opcode in 8) is only stored when both table flags
//! of a resolution are set; otherwise tile `i` is BC7 if the BC7 flag is set, BC1 if not, at
//! `i * size`. Opcodes: 0x40 BC1, 0x41 BC7, 0xC0 a copy of tile `offset`, anything else is
//! transparent.
//!
//! The low resolution is in the pak's pages, the high one in the starpak (`rpak.asset.stream`),
//! `streamed size << 12` bytes, compressed with the codec of the flags. The tiles are laid out
//! tile after tile and then swizzled block by block (`untile`).

mod json;

pub use json::{UiiaJsonEncode, UiiaJsonOptions};

mod pixels;
use pixels::*;
mod read;
pub use read::UiiaRead;

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Bitmap, BitmapInfo, Dependency, Metadata, PixelFormat};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};
use respak_formats::decode_rgba8;

use crate::rpak::{RpakAsset, MEM};
use crate::txtr::read_name;
use crate::{oodle, rtech, snowflake};

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("uiia", version)
}

const HEADER_LEN: usize = 0x40;
const CPU_LEN: u64 = 0x40;
const BC1_TILE: usize = 512;
const BC7_TILE: usize = 1024;
const BC1_OP: u32 = 0x40;
const BC7_OP: u32 = 0x41;
const COPY_OP: u32 = 0xC0;
/// A tile is 32x32 pixels, 31x31 of them visible.
const TILE_PX: u32 = 32;
const VISIBLE_PX: u32 = 31;
/// More tiles than this cannot be a UI image (BC7 would take over 256 MB).
const MAX_TILES: u32 = 1 << 18;

/// Which resolution of the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// In the pak's pages.
    Low,
    /// In the starpak.
    High,
}

/// Size and tile grid of one resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Tiles across.
    pub tiles_x: u32,
    /// Tiles down.
    pub tiles_y: u32,
}

impl Resolution {
    fn new(width: u32, height: u32, flags: u8) -> Self {
        // Bits 6 and 7 of the flags say the width / height has no extra pixel.
        let off_x = if flags & 0x40 == 0 { 2 } else { 0 };
        let off_y = if flags & 0x80 == 0 { 2 } else { 0 };
        Self {
            width,
            height,
            tiles_x: (width + off_x + 29) / VISIBLE_PX,
            tiles_y: (height + off_y + 29) / VISIBLE_PX,
        }
    }

    fn tiles(&self) -> u32 {
        self.tiles_x * self.tiles_y
    }
}

/// The decoded header and CPU data of a UI image.
#[derive(Debug, Clone, PartialEq)]
pub struct UiiaHeader {
    /// `rpak.asset` version (2).
    pub version: u32,
    /// Raw flags byte.
    pub flags: u8,
    /// Size of the streamed data in bytes (0 = none).
    pub streamed_size: u64,
    /// The low resolution (in the pak).
    pub low: Resolution,
    /// The high resolution (in the starpak).
    pub high: Resolution,
    /// The eight floats at header 0x20, four x/y pairs placing the image on a virtual canvas:
    /// origin, size as a fraction of the canvas, half-texel offset (`-0.5 / (size - 1)`) and
    /// `canvas size / (image size - 1)`.
    pub layout: [f32; 8],
}

impl UiiaHeader {
    /// Parses the relocated header and the 0x40 bytes of CPU data.
    pub fn parse(version: u32, header: &[u8], cpu: &[u8]) -> Result<Self> {
        if version != 2 {
            return Err(unsupported(version.to_string()));
        }
        if header.len() != HEADER_LEN || cpu.len() != CPU_LEN as usize {
            return Err(Error::corrupt(format!(
                "uiia header is {} bytes and CPU data {}, expected {HEADER_LEN}",
                header.len(),
                cpu.len()
            )));
        }
        let le16 = |b: &[u8], at: usize| u32::from(u16::from_le_bytes([b[at], b[at + 1]]));
        let flags = header[0x12];
        let low = Resolution::new(le16(cpu, 0x24), le16(cpu, 0x26), flags);
        let high = Resolution::new(le16(cpu, 0x20), le16(cpu, 0x22), flags);
        for (what, r) in [("low", low), ("high", high)] {
            if r.width == 0 || r.height == 0 || r.tiles() > MAX_TILES {
                return Err(Error::corrupt(format!(
                    "uiia {what} resolution {}x{} is not usable",
                    r.width, r.height
                )));
            }
        }
        let layout = std::array::from_fn(|i| {
            f32::from_le_bytes([
                header[0x20 + 4 * i],
                header[0x21 + 4 * i],
                header[0x22 + 4 * i],
                header[0x23 + 4 * i],
            ])
        });
        Ok(Self {
            version,
            flags,
            streamed_size: u64::from(le16(header, 0x10)) << 12,
            low,
            high,
            layout,
        })
    }

    /// The codec of the streamed data: 0 none, 1 RTech, 2 Snowflake, 3 Oodle.
    pub fn codec(&self) -> u8 {
        (self.flags >> 4) & 3
    }

    /// Whether the table of `quality` is stored (both table flags set) rather than implied.
    fn has_table(&self, quality: Quality) -> bool {
        let shift = if quality == Quality::Low { 0 } else { 2 };
        (self.flags >> shift) & 3 == 3
    }

    /// The tile kind implied when there is no table.
    fn implied_op(&self, quality: Quality) -> u32 {
        let shift = if quality == Quality::Low { 1 } else { 3 };
        if (self.flags >> shift) & 1 != 0 {
            BC7_OP
        } else {
            BC1_OP
        }
    }
}

/// One entry of the tile table.
/// A UI image: `Data` + `Bitmap` + `Metadata`.
#[derive(Debug)]
pub struct UiiaAsset {
    format: FormatId,
    key: ValueKey,
    header: UiiaHeader,
    name: Option<String>,
    guid: u64,
    quality: Quality,
    info: BitmapInfo,
    volume: Operator,
    /// `.mem` offset of the low-resolution data.
    data_offset: u64,
    stream: Option<String>,
}

impl UiiaAsset {
    /// The parsed header.
    pub fn header(&self) -> &UiiaHeader {
        &self.header
    }

    /// The resolution the bitmap holds: high when the starpak is there, else low.
    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// The asset's guid.
    pub fn guid(&self) -> u64 {
        self.guid
    }

    /// The data of the tiles: the pak's pages, or the decoded streamed buffer.
    async fn tile_data(&self, res: &Resolution) -> Result<Vec<u8>> {
        if self.quality == Quality::High {
            let path = self.stream.as_deref().unwrap_or_default();
            let size = self.header.streamed_size;
            let block = self.volume.read_with(path).range(0..size).await?.to_bytes();
            return match self.header.codec() {
                0 => Ok(block.to_vec()),
                1 => rtech::decompress(&block, 0, None).map_err(|e| Error::custom("rtech", e)),
                2 => snowflake::decompress(&block).map_err(|e| Error::custom("snowflake", e)),
                // rsx decodes to `streamedSize` bytes as well.
                _ => oodle::decompress(&block, size).map_err(|e| Error::custom("oodle", e)),
            };
        }
        // The table (if any) tells how far the tiles reach; read that much of the pages.
        let mem_len = self.volume.stat(MEM).await?.content_length();
        let available = mem_len.saturating_sub(self.data_offset);
        let table = if self.header.has_table(Quality::Low) {
            u64::from(res.tiles()) * 4
        } else {
            0
        };
        let head = if table == 0 {
            Vec::new()
        } else {
            self.volume
                .read_with(MEM)
                .range(self.data_offset..self.data_offset + table.min(available))
                .await?
                .to_vec()
        };
        let list = tiles(&self.header, Quality::Low, res, &head)?;
        let len = needed_len(&self.header, Quality::Low, res, &list);
        if len > available {
            return Err(Error::corrupt(
                "uiia tiles run past the end of the pak's pages",
            ));
        }
        Ok(self
            .volume
            .read_with(MEM)
            .range(self.data_offset..self.data_offset + len)
            .await?
            .to_vec())
    }
}

#[async_trait]
impl Bitmap for UiiaAsset {
    fn info(&self) -> BitmapInfo {
        self.info.clone()
    }

    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes> {
        if level != 0 || layer != 0 {
            return Err(Error::new(ErrorKind::NotFound {
                path: format!("mip {level} of layer {layer}"),
            }));
        }
        let res = if self.quality == Quality::High {
            self.header.high
        } else {
            self.header.low
        };
        let data = self.tile_data(&res).await?;
        let list = tiles(&self.header, self.quality, &res, &data)?;
        Ok(Bytes::from(compose(&res, &list, &data)?))
    }
}

impl Metadata for UiiaAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("uiia")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for UiiaAsset {
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
