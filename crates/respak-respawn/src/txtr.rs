//! `txtr`: the Respawn texture asset, read from `rpak.asset`.
//!
//! Ported from rsx `src/game/rtech/assets/texture.{h,cpp}` (`TextureAssetHeader_v8_t`/`v9`/`v10`,
//! `LoadTextureAsset`, `s_PakToDxgiFormat`, `s_pBytesPerPixel`). `txtr` reads the `rpak.asset`
//! data type of this crate but never parses pak structures: the header arrives with its
//! pointers already turned into `.mem` offsets by the pak's volume.
//!
//! # Header (56 bytes, `rpak.asset` version 8, 9 or 10)
//! | field | v8 | v9 | v10 |
//! |---|---|---|---|
//! | name pointer | 0x08 | 0x00 | 0x00 |
//! | width / height (u16) | 0x10 / 0x12 | 0x0A / 0x0C | 0x0A / 0x0C |
//! | pixel format (u16) | 0x16 | 0x08 | 0x08 |
//! | array size (u8, 0 = 1) | 0x1E | 0x10 | 0x10 |
//! | swizzle (u8, must be 0) | 0x1C | 0x12 | 0x12 |
//! | permanent / streamed / optional mips (u8) | 0x21 / 0x22 / 0x1D | 0x18 / 0x19 / 0x1A | 0x18 / 0x19 / 0x1A |
//! | unknown mips (v10, must be 0) | | | 0x1C |
//!
//! # Mips
//! With shift `i` (0 = largest) the mip is `max(1, w >> i)` by `max(1, h >> i)`. The optional
//! starpak holds shifts `0..opt`, the mandatory starpak the next `streamed`, and the smallest
//! `permanent` mips are stored in the asset's CPU data, smallest first, each mip
//! `align16(size) * array` bytes with the array slices back to back.
//!
//! This slice exposes the permanent mips only (streamed mips are slice 10): the [`Bitmap`] is
//! as big as the largest permanent mip, and a warning says how many mips are missing.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Bitmap, BitmapInfo, Dependency, Metadata, PixelFormat};
use respak_core::{Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey, Version};

use crate::rpak::{RpakAsset, MEM};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

fn unsupported(version: impl Into<String>) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: "txtr".into(), version: version.into() })
}

const HEADER_LEN: usize = 56;
/// More mips than this cannot be a 16-bit-sized texture.
const MAX_MIPS: u32 = 16;

/// The pixel format enum of the pak (`eTextureFormat`) mapped to ours, for the formats we can
/// carry. `None` for the rest (wide integer formats, ASTC, depth).
pub fn pixel_format(index: u16) -> Option<PixelFormat> {
    use PixelFormat::*;
    Some(match index {
        0 => Bc1,
        1 => Bc1Srgb,
        2 => Bc2,
        3 => Bc2Srgb,
        4 => Bc3,
        5 => Bc3Srgb,
        6 => Bc4,
        7 => Bc4Snorm,
        8 => Bc5,
        9 => Bc5Snorm,
        10 => Bc6H,
        11 => Bc6HSigned,
        12 => Bc7,
        13 => Bc7Srgb,
        14 => Rgba32F,
        20 => Rgba16F,
        31 => Rgba8,
        32 => Rgba8Srgb,
        41 => R32F,
        44 => Rg8,
        48 => R16F,
        49 => R16,
        53 => R8,
        _ => return None,
    })
}

fn le16(b: &[u8], at: usize) -> u32 {
    u32::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

/// A decoded texture header with the mip layout worked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxtrHeader {
    /// `rpak.asset` version: 8, 9 or 10.
    pub version: u32,
    /// Raw `eTextureFormat` value.
    pub format_index: u16,
    /// Width of mip 0 (the largest, possibly streamed).
    pub width: u32,
    /// Height of mip 0.
    pub height: u32,
    /// Array size, at least 1.
    pub array_size: u32,
    /// Mips stored in the asset's CPU data.
    pub permanent: u32,
    /// Mips in the mandatory starpak.
    pub streamed: u32,
    /// Mips in the optional starpak.
    pub optional: u32,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
}

impl TxtrHeader {
    /// Parses a relocated 56-byte header of the given asset `version`.
    pub fn parse(version: u32, h: &[u8]) -> Result<Self> {
        if !(8..=10).contains(&version) {
            return Err(unsupported(version.to_string()));
        }
        if h.len() != HEADER_LEN {
            return Err(corrupt(format!("txtr header is {} bytes, expected {HEADER_LEN}", h.len())));
        }
        let ptr = |at: usize| {
            let v = u64::from_le_bytes(h[at..at + 8].try_into().expect("8 bytes"));
            // Null is all ones after relocation, or zero when the slot was never a pointer.
            (v != u64::MAX && v != 0).then_some(v)
        };
        let (name_offset, w, ht, fmt, array, swizzle, perm, streamed, opt) = if version == 8 {
            (ptr(8), le16(h, 0x10), le16(h, 0x12), le16(h, 0x16), h[0x1E], h[0x1C], h[0x21], h[0x22], h[0x1D])
        } else {
            (ptr(0), le16(h, 0x0A), le16(h, 0x0C), le16(h, 0x08), h[0x10], h[0x12], h[0x18], h[0x19], h[0x1A])
        };
        if swizzle != 0 {
            return Err(unsupported(format!("{version} (swizzle {swizzle}, console layout)")));
        }
        if version == 10 && h[0x1C] != 0 {
            return Err(unsupported("10 (unknown mip levels)"));
        }
        let header = Self {
            version,
            format_index: fmt as u16,
            width: w,
            height: ht,
            array_size: u32::from(array).max(1),
            permanent: u32::from(perm),
            streamed: u32::from(streamed),
            optional: u32::from(opt),
            name_offset,
        };
        if w == 0 || ht == 0 {
            return Err(corrupt("txtr has a zero dimension"));
        }
        let total = header.total_mips();
        if total == 0 || total > MAX_MIPS || header.permanent == 0 {
            return Err(corrupt(format!("txtr mip counts are not usable (permanent {perm}, streamed {streamed}, optional {opt})")));
        }
        Ok(header)
    }

    /// All mips of the texture.
    pub fn total_mips(&self) -> u32 {
        self.permanent + self.streamed + self.optional
    }

    /// The shift (0 = largest) of the largest mip that is stored in the CPU data.
    pub fn first_permanent_shift(&self) -> u32 {
        self.streamed + self.optional
    }
}

/// Where one available mip lives, relative to the asset's CPU data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MipLoc {
    offset: u64,
    /// Distance between array slices (the mip padded to 16 bytes).
    stride: u64,
    /// Bytes of one slice that are pixels.
    len: u64,
}

/// A texture: `Data` + `Bitmap` + `Metadata`.
#[derive(Debug)]
pub struct TxtrAsset {
    format: FormatId,
    key: ValueKey,
    header: TxtrHeader,
    name: Option<String>,
    guid: u64,
    info: BitmapInfo,
    mips: Vec<MipLoc>,
    volume: Operator,
    data_offset: u64,
}

impl TxtrAsset {
    /// The parsed header.
    pub fn header(&self) -> &TxtrHeader {
        &self.header
    }
}

#[async_trait]
impl Bitmap for TxtrAsset {
    fn info(&self) -> BitmapInfo {
        self.info.clone()
    }

    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes> {
        let (Some(mip), true) = (self.mips.get(level as usize), layer < self.header.array_size) else {
            return Err(Error::new(ErrorKind::NotFound { path: format!("mip {level} of layer {layer}") }));
        };
        let at = self.data_offset + mip.offset + mip.stride * u64::from(layer);
        Ok(self.volume.read_with(MEM).range(at..at + mip.len).await?.to_bytes())
    }
}

impl Metadata for TxtrAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("txtr")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for TxtrAsset {
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

/// Read edge `rpak.asset -> txtr`.
pub struct TxtrRead;

#[async_trait]
impl Detect for TxtrRead {
    /// The asset kind is `txtr` (a container-provided kind) = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"txtr")
            .then(|| Detected { score: 10, version: Some(Version::new(asset.entry.version)) }))
    }
}

/// The NUL-terminated name at `.mem` offset `at`, if it fits in 512 bytes.
async fn read_name(volume: &Operator, at: u64, mem_len: u64) -> Option<String> {
    if at >= mem_len {
        return None;
    }
    let end = (at + 512).min(mem_len);
    let bytes = volume.read_with(MEM).range(at..end).await.ok()?.to_vec();
    let nul = bytes.iter().position(|&b| b == 0)?;
    let name = String::from_utf8_lossy(&bytes[..nul]).into_owned();
    (!name.is_empty()).then_some(name)
}

#[async_trait]
impl ReadHandler for TxtrRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source
            .downcast_ref::<RpakAsset>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "txtr must be read from an rpak.asset".into() }))?;
        let header = TxtrHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let pixel = pixel_format(header.format_index)
            .ok_or_else(|| unsupported(format!("{} (pixel format #{})", header.version, header.format_index)))?;

        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let data_offset = asset.data_mem_offset.ok_or_else(|| corrupt("txtr has no data pointer"))?;

        // The permanent mips, smallest first in the data.
        let first = header.first_permanent_shift();
        let total = header.total_mips();
        let mut cursor = 0u64;
        let mut by_shift = Vec::new();
        for shift in (first..total).rev() {
            let len = pixel.mip_len(header.width >> shift, header.height >> shift);
            let stride = len.div_ceil(16).max(1) * 16;
            by_shift.push((shift, MipLoc { offset: cursor, stride, len }));
            cursor += stride * u64::from(header.array_size);
        }
        if data_offset.checked_add(cursor).is_none_or(|end| end > mem_len) {
            return Err(corrupt("txtr mips run past the end of the pak's pages"));
        }
        // Level 0 is the largest available mip.
        let mips: Vec<MipLoc> = by_shift.into_iter().rev().map(|(_, m)| m).collect();

        let missing = header.streamed + header.optional;
        let name = match header.name_offset {
            Some(at) => read_name(&asset.volume, at, mem_len).await,
            None => None,
        }
        .map(|n| if n.to_ascii_lowercase().starts_with("texture/") { n } else { format!("texture/{n}") });
        if missing > 0 {
            tracing::warn!(
                "texture {}: {missing} streamed mip(s) are not available yet, only the {} permanent mip(s) are used",
                name.as_deref().unwrap_or("(unnamed)"),
                header.permanent
            );
        }
        let info = BitmapInfo {
            width: (header.width >> first).max(1),
            height: (header.height >> first).max(1),
            depth: 1,
            array_len: header.array_size,
            faces: 1,
            mip_count: header.permanent,
            format: pixel,
        };
        let format = FormatId::new("txtr").with_version(Version::new(header.version));
        Ok(Arc::new(TxtrAsset {
            format,
            key: source.key().clone(),
            header,
            name,
            guid: asset.entry.guid,
            info,
            mips,
            volume: asset.volume.clone(),
            data_offset,
        }))
    }
}
