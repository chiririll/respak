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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    offset: u32,
    opcode: u32,
}

fn tile_size(opcode: u32) -> usize {
    if opcode == BC7_OP {
        BC7_TILE
    } else {
        BC1_TILE
    }
}

/// The tile table of `quality`. `data` is the pak's data or the decoded stream; the stored table
/// is at its start.
fn tiles(
    header: &UiiaHeader,
    quality: Quality,
    res: &Resolution,
    data: &[u8],
) -> Result<Vec<Tile>> {
    let count = res.tiles() as usize;
    let mut out = Vec::with_capacity(count);
    if header.has_table(quality) {
        let table = data
            .get(..count * 4)
            .ok_or_else(|| Error::corrupt("uiia tile table runs past its data"))?;
        for c in table.chunks_exact(4) {
            let v = u32::from_le_bytes(c.try_into().expect("4 bytes"));
            out.push(Tile {
                offset: v & 0x00FF_FFFF,
                opcode: v >> 24,
            });
        }
        // A copy takes the (already resolved) tile it points at.
        for i in 0..count {
            if out[i].opcode == COPY_OP {
                out[i] = *out
                    .get(out[i].offset as usize)
                    .ok_or_else(|| Error::corrupt("uiia tile copies a tile that does not exist"))?;
            }
        }
    } else {
        let op = header.implied_op(quality);
        for i in 0..count {
            out.push(Tile {
                offset: (i * tile_size(op)) as u32,
                opcode: op,
            });
        }
    }
    Ok(out)
}

/// The bytes of `data` the tiles and the table of `quality` need.
fn needed_len(header: &UiiaHeader, quality: Quality, res: &Resolution, tiles: &[Tile]) -> u64 {
    let table = if header.has_table(quality) {
        u64::from(res.tiles()) * 4
    } else {
        0
    };
    tiles
        .iter()
        .filter(|t| t.opcode == BC1_OP || t.opcode == BC7_OP)
        .map(|t| u64::from(t.offset) + tile_size(t.opcode) as u64)
        .max()
        .unwrap_or(0)
        .max(table)
}

/// Puts the blocks of `buf` (in the order the tiles lay them out) at their swizzled places
/// (rsx `DoTilingWork`). `wb` x `hb` is the image in 4x4 blocks of `bytes` bytes.
fn untile(buf: &[u8], wb: u32, hb: u32, bytes: usize) -> Result<Vec<u8>> {
    let (wb, hb) = (i64::from(wb), i64::from(hb));
    if wb % 8 != 0 || buf.len() as i64 != wb * hb * bytes as i64 {
        return Err(Error::corrupt("uiia tiles do not fill a whole image"));
    }
    // One step of the swizzle: where block `t` of rows of `w` goes (rsx `b_N`).
    let step = |t: i64, w: i64| (t % (2 * w)) / 2 + w * ((t % (2 * w)) % 2) + 2 * w * (t / (2 * w));
    let mut out = vec![0u8; buf.len()];
    let mut at = 0usize;
    for y in 0..hb {
        for x in 0..wb {
            let (mut mx, mut my) = (x, y);

            let w2 = wb / 2;
            let b2 = step(mx / 2 + my * w2, w2);
            let mut c2 = mx % 2 + 2 * (b2 % w2);
            mx = b2 / w2;
            my = c2 / 4;
            c2 %= 4;

            let w4 = wb / 4;
            let b4 = step(my + mx / 2 * w4, w4);
            let mut c4 = mx % 2 + 2 * (b4 / w4);
            // rsx also sets `mx = b4 / w4` here; the last stage overwrites it unread.
            my = c4 / 4;
            c4 %= 4;

            let w8 = wb / 8;
            let b8 = step((c2 + 4 * (b4 % w4)) / 8 + my * w8, w8);
            my = c4 + 4 * (b8 / w8);
            mx = (c2 + 4 * (b4 % w4)) % 8 + 8 * (b8 % w8);

            let dst = (my * wb + mx) as usize * bytes;
            let block = buf
                .get(at..at + bytes)
                .ok_or_else(|| Error::corrupt("uiia tiles are cut short"))?;
            out.get_mut(dst..dst + bytes)
                .ok_or_else(|| Error::corrupt("uiia swizzle leaves the image"))?
                .copy_from_slice(block);
            at += bytes;
        }
    }
    Ok(out)
}

/// Decodes all tiles of one kind to an RGBA8 image of `tiles_x * 32` x `tiles_y * 32`. Tiles of
/// another kind are the transparent tile of this one.
fn decode_kind(res: &Resolution, tiles: &[Tile], data: &[u8], opcode: u32) -> Result<Vec<u8>> {
    let size = tile_size(opcode);
    // Transparent tile: BC1 `0000 FFFFFFFF` blocks, or BC7 mode 4 blocks (rsx `UIImageTileBc1/7`).
    let block: &[u8] = if opcode == BC7_OP {
        &[0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    } else {
        &[0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    };
    let mut buf = Vec::with_capacity(tiles.len() * size);
    for t in tiles {
        if t.opcode == opcode {
            let at = t.offset as usize;
            buf.extend_from_slice(
                data.get(at..at + size)
                    .ok_or_else(|| Error::corrupt("uiia tile runs past its data"))?,
            );
        } else {
            for _ in 0..size / block.len() {
                buf.extend_from_slice(block);
            }
        }
    }
    let (w, h) = (res.tiles_x * TILE_PX, res.tiles_y * TILE_PX);
    let swizzled = untile(&buf, w / 4, h / 4, block.len())?;
    let format = if opcode == BC7_OP {
        PixelFormat::Bc7
    } else {
        PixelFormat::Bc1
    };
    decode_rgba8(format, w, h, &swizzled)
}

/// The finished RGBA8 image of one resolution (rsx `CreateTextureForImage`).
fn compose(res: &Resolution, tiles: &[Tile], data: &[u8]) -> Result<Vec<u8>> {
    let has = |op| tiles.iter().any(|t| t.opcode == op);
    let (bc1, bc7) = (
        has(BC1_OP).then(|| decode_kind(res, tiles, data, BC1_OP)),
        has(BC7_OP).then(|| decode_kind(res, tiles, data, BC7_OP)),
    );
    let stride = (res.tiles_x * TILE_PX) as usize * 4;
    let shifted = match (bc1, bc7) {
        (None, None) => return Err(Error::corrupt("uiia has no tiles")),
        (Some(bc1), None) => bc1?,
        (None, Some(bc7)) => bc7?,
        (Some(bc1), Some(bc7)) => {
            let (mut base, bc7) = (bc1?, bc7?);
            for i in tiles
                .iter()
                .enumerate()
                .filter(|(_, t)| t.opcode == BC7_OP)
                .map(|(i, _)| i)
            {
                let (x, y) = (i as u32 % res.tiles_x, i as u32 / res.tiles_x);
                for row in 0..TILE_PX {
                    let at = ((y * TILE_PX + row) as usize) * stride + (x * TILE_PX) as usize * 4;
                    base[at..at + TILE_PX as usize * 4]
                        .copy_from_slice(&bc7[at..at + TILE_PX as usize * 4]);
                }
            }
            base
        }
    };
    // Drop the shared border pixel of every tile.
    let mut out = vec![0u8; res.width as usize * res.height as usize * 4];
    for y in 0..res.tiles_y {
        let size_y = if y * VISIBLE_PX + 30 < res.height {
            VISIBLE_PX
        } else {
            res.height.saturating_sub(y * VISIBLE_PX)
        };
        for x in 0..res.tiles_x {
            let size_x = if x * VISIBLE_PX + 30 < res.width {
                VISIBLE_PX
            } else {
                res.width.saturating_sub(x * VISIBLE_PX)
            };
            for row in 0..size_y {
                let src = ((y * TILE_PX + row) as usize) * stride + (x * TILE_PX) as usize * 4;
                let dst = (((y * VISIBLE_PX + row) * res.width) + x * VISIBLE_PX) as usize * 4;
                out[dst..dst + size_x as usize * 4]
                    .copy_from_slice(&shifted[src..src + size_x as usize * 4]);
            }
        }
    }
    Ok(out)
}

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

/// Read edge `rpak.asset -> uiia`.
pub struct UiiaRead;

#[async_trait]
impl Detect for UiiaRead {
    /// The asset kind is `uiia` (a container-provided kind) = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"uiia").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

#[async_trait]
impl ReadHandler for UiiaRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "uiia must be read from an rpak.asset".into(),
            })
        })?;
        let data_offset = asset
            .data_mem_offset
            .ok_or_else(|| Error::corrupt("uiia has no CPU data"))?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        if data_offset
            .checked_add(CPU_LEN)
            .is_none_or(|end| end > mem_len)
        {
            return Err(Error::corrupt(
                "uiia CPU data runs past the end of the pak's pages",
            ));
        }
        let cpu = asset.read_mem(data_offset, CPU_LEN).await?;
        let header = UiiaHeader::parse(asset.entry.version, &asset.read_header().await?, &cpu)?;

        // Pointers inside page data are still `{page, offset}` pairs.
        let slot = |at: usize| asset.mem_offset_of(cpu[at..at + 8].try_into().expect("8 bytes"));
        let pixels = slot(0x28).ok_or_else(|| Error::corrupt("uiia has no image data"))?;
        let name = match slot(0x30) {
            Some(at) => read_name(&asset.volume, at, mem_len).await,
            None => None,
        }
        .map(|n| {
            if n.starts_with("ui_image/") {
                n
            } else {
                format!("ui_image/{n}")
            }
        });
        let label = name.as_deref().unwrap_or("(unnamed)");

        // The high resolution lives in the starpak; without it the low one is all there is.
        let mut quality = Quality::Low;
        if let Some(path) = asset.stream.as_deref().filter(|_| header.streamed_size > 0) {
            match asset.volume.stat(path).await {
                Ok(meta) if meta.content_length() >= header.streamed_size => {
                    quality = Quality::High
                }
                Ok(_) => {
                    return Err(Error::corrupt(format!(
                        "uiia {label}: its streamed data does not fit the starpak entry"
                    )))
                }
                Err(e) => {
                    let e = Error::from(e);
                    if matches!(
                        e.kind(),
                        ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
                    ) {
                        cx.warn(format!("uiia {label}: the high resolution is skipped: {path} is not available ({e})"));
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        let res = if quality == Quality::High {
            header.high
        } else {
            header.low
        };
        let info = BitmapInfo {
            width: res.width,
            height: res.height,
            depth: 1,
            array_len: 1,
            faces: 1,
            mip_count: 1,
            format: PixelFormat::Rgba8,
        };
        let format = FormatId::new("uiia").with_version(Version::new(header.version));
        Ok(Arc::new(UiiaAsset {
            format,
            key: source.key().clone(),
            header,
            name,
            guid: asset.entry.guid,
            quality,
            info,
            volume: asset.volume.clone(),
            data_offset: pixels,
            stream: asset.stream.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_grid() {
        // No extra pixel bits: (n + 2 + 29) / 31.
        let r = Resolution::new(62, 31, 0);
        assert_eq!((r.tiles_x, r.tiles_y), (3, 2));
        let r = Resolution::new(62, 31, 0xC0);
        assert_eq!((r.tiles_x, r.tiles_y), (2, 1));
    }

    #[test]
    fn untile_visits_every_block_once() {
        for (wb, hb) in [(8, 8), (16, 8), (32, 16), (16, 24)] {
            let buf: Vec<u8> = (0..wb * hb).flat_map(|i: u32| i.to_le_bytes()).collect();
            let out = untile(&buf, wb, hb, 4).unwrap();
            let mut seen: Vec<u32> = out
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect();
            seen.sort_unstable();
            assert!(
                seen.iter().copied().eq(0..wb * hb),
                "{wb}x{hb} is not a permutation"
            );
        }
    }

    #[test]
    fn implied_tables() {
        let mut h = UiiaHeader {
            version: 2,
            flags: 0b10, // low BC7, no stored table
            streamed_size: 0,
            low: Resolution::new(31, 31, 0xC0),
            high: Resolution::new(31, 31, 0xC0),
            layout: [0.0; 8],
        };
        let res = h.low;
        let t = tiles(&h, Quality::Low, &res, &[]).unwrap();
        assert_eq!(
            t,
            [Tile {
                offset: 0,
                opcode: BC7_OP
            }]
        );
        h.flags = 0;
        assert_eq!(
            tiles(&h, Quality::Low, &res, &[]).unwrap()[0].opcode,
            BC1_OP
        );
    }

    #[test]
    fn stored_table_copies_tiles() {
        let h = UiiaHeader {
            version: 2,
            flags: 0b11,
            streamed_size: 0,
            low: Resolution::new(62, 31, 0xC0),
            high: Resolution::new(62, 31, 0xC0),
            layout: [0.0; 8],
        };
        let mut data = Vec::new();
        data.extend_from_slice(&(BC1_OP << 24 | 8).to_le_bytes());
        data.extend_from_slice(&(COPY_OP << 24).to_le_bytes());
        let t = tiles(&h, Quality::Low, &h.low, &data).unwrap();
        assert_eq!(
            t[1],
            Tile {
                offset: 8,
                opcode: BC1_OP
            }
        );
        assert_eq!(needed_len(&h, Quality::Low, &h.low, &t), 8 + 512);
    }

    #[test]
    fn transparent_image() {
        let res = Resolution::new(31, 31, 0xC0);
        let tiles = [Tile {
            offset: 0,
            opcode: BC1_OP,
        }];
        // A BC1 tile of all-transparent blocks: 512 bytes of the pattern.
        let data: Vec<u8> = [0u8, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF].repeat(64);
        let rgba = compose(&res, &tiles, &data).unwrap();
        assert_eq!(rgba.len(), 31 * 31 * 4);
        assert!(rgba.chunks_exact(4).all(|p| p[3] == 0));
    }
}
