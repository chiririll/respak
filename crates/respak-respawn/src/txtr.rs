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
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::rpak::{RpakAsset, MEM};
use crate::{oodle, rtech};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

fn unsupported(version: impl Into<String>) -> Error {
    Error::new(ErrorKind::UnsupportedVersion {
        format: "txtr".into(),
        version: version.into(),
    })
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
    /// 2 bits per streamed mip (index 0 = the largest): 0 none, 1 RTech, 2 Snowflake, 3 Oodle.
    /// Always 0 for v8.
    pub codecs: u16,
    /// For each compressed streamed mip, its stored size in 4096-byte units minus one.
    pub compressed_blocks: [u16; MAX_COMPRESSED],
}

/// The header has room for this many compressed streamed mips.
const MAX_COMPRESSED: usize = 7;

impl TxtrHeader {
    /// Parses a relocated 56-byte header of the given asset `version`.
    pub fn parse(version: u32, h: &[u8]) -> Result<Self> {
        if !(8..=10).contains(&version) {
            return Err(unsupported(version.to_string()));
        }
        if h.len() != HEADER_LEN {
            return Err(corrupt(format!(
                "txtr header is {} bytes, expected {HEADER_LEN}",
                h.len()
            )));
        }
        let ptr = |at: usize| {
            let v = u64::from_le_bytes(h[at..at + 8].try_into().expect("8 bytes"));
            // Null is all ones after relocation, or zero when the slot was never a pointer.
            (v != u64::MAX && v != 0).then_some(v)
        };
        let (name_offset, w, ht, fmt, array, swizzle, perm, streamed, opt) = if version == 8 {
            (
                ptr(8),
                le16(h, 0x10),
                le16(h, 0x12),
                le16(h, 0x16),
                h[0x1E],
                h[0x1C],
                h[0x21],
                h[0x22],
                h[0x1D],
            )
        } else {
            (
                ptr(0),
                le16(h, 0x0A),
                le16(h, 0x0C),
                le16(h, 0x08),
                h[0x10],
                h[0x12],
                h[0x18],
                h[0x19],
                h[0x1A],
            )
        };
        // v9 keeps the codec bits at 0x1C and the sizes at 0x1E; v10 moved both two bytes up.
        // v8 has no compression.
        let comp_at = match version {
            8 => None,
            9 => Some(0x1C),
            _ => Some(0x1E),
        };
        let codecs = comp_at.map_or(0, |at| le16(h, at) as u16);
        let mut compressed_blocks = [0u16; MAX_COMPRESSED];
        if let Some(at) = comp_at {
            for (i, b) in compressed_blocks.iter_mut().enumerate() {
                *b = le16(h, at + 2 + 2 * i) as u16;
            }
        }
        if swizzle != 0 {
            return Err(unsupported(format!(
                "{version} (swizzle {swizzle}, console layout)"
            )));
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
            codecs,
            compressed_blocks,
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

/// Which file of the asset a mip is stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Store {
    /// The pak's pages (`.mem`), from the asset's CPU data.
    Mem,
    /// The mandatory starpak entry.
    Stream,
    /// The optional starpak entry.
    Opt,
}

/// Where one available mip lives. `offset` is relative to the asset's CPU data (`Mem`) or to
/// the start of its starpak entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MipLoc {
    store: Store,
    offset: u64,
    /// Distance between array slices (the mip padded to 16 bytes, or its compressed block).
    stride: u64,
    /// Bytes of one slice that are pixels.
    len: u64,
    /// The codec of a compressed mip (1 RTech, 2 Snowflake, 3 Oodle); 0 = stored as is.
    codec: u8,
}

/// The mips held by one starpak entry, as `(shift, location)`, in storage order.
///
/// v9/v10 store the largest mip first, v8 the smallest first. An uncompressed mip takes
/// `align16(size)` per array slice; a compressed one its block of `(compressedBytes + 1) << 12`
/// bytes (rsx `TextureAsset`, `SORT_MIXED` / `SORT_BOTTOM_TOP`).
fn layout_entry(
    h: &TxtrHeader,
    pixel: PixelFormat,
    store: Store,
    shifts: std::ops::Range<u32>,
) -> Result<Vec<(u32, MipLoc)>> {
    let order: Vec<u32> = if h.version == 8 {
        shifts.rev().collect()
    } else {
        shifts.collect()
    };
    let mut cursor = 0u64;
    let mut out = Vec::new();
    for shift in order {
        let len = pixel.mip_len((h.width >> shift).max(1), (h.height >> shift).max(1));
        let codec = ((h.codecs >> (2 * shift)) & 3) as u8;
        let stride = if codec == 0 {
            len.div_ceil(16).max(1) * 16
        } else {
            let block = h.compressed_blocks.get(shift as usize).ok_or_else(|| {
                corrupt(format!("txtr: no compressed size for streamed mip {shift}"))
            })?;
            if h.array_size > 1 {
                return Err(unsupported(format!(
                    "{} (compressed streamed mips of an array)",
                    h.version
                )));
            }
            (u64::from(*block) + 1) << 12
        };
        out.push((
            shift,
            MipLoc {
                store,
                offset: cursor,
                stride,
                len,
                codec,
            },
        ));
        cursor += stride * u64::from(h.array_size);
    }
    Ok(out)
}

/// Total bytes of the mips from [`layout_entry`].
fn entry_len(h: &TxtrHeader, mips: &[(u32, MipLoc)]) -> u64 {
    mips.iter()
        .map(|(_, m)| m.stride * u64::from(h.array_size))
        .sum()
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
    stream: Option<String>,
    opt: Option<String>,
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
        let (Some(mip), true) = (
            self.mips.get(level as usize),
            layer < self.header.array_size,
        ) else {
            return Err(Error::new(ErrorKind::NotFound {
                path: format!("mip {level} of layer {layer}"),
            }));
        };
        let slice = mip.offset + mip.stride * u64::from(layer);
        if mip.codec == 0 {
            // Only the pixels are read; the padding is skipped.
            let (path, at) = match mip.store {
                Store::Mem => (MEM, self.data_offset + slice),
                Store::Stream => (self.stream.as_deref().unwrap_or_default(), slice),
                Store::Opt => (self.opt.as_deref().unwrap_or_default(), slice),
            };
            return Ok(self
                .volume
                .read_with(path)
                .range(at..at + mip.len)
                .await?
                .to_bytes());
        }
        let path = match mip.store {
            Store::Opt => self.opt.as_deref(),
            _ => self.stream.as_deref(),
        };
        // The decoder gets the whole padded block and the exact decoded length (rsx
        // `DecompressStreamedBuffer`).
        let block = self
            .volume
            .read_with(path.unwrap_or_default())
            .range(slice..slice + mip.stride)
            .await?
            .to_bytes();
        match mip.codec {
            1 => {
                let decoded = rtech::decompress(&block, 0, Some(mip.len))
                    .map_err(|e| Error::custom("rtech", e))?;
                Ok(Bytes::from(decoded))
            }
            2 => Err(unsupported("Snowflake-compressed streamed mips")),
            3 => {
                // The padded block holds the stream and zeros after it.
                let decoded =
                    oodle::decompress(&block, mip.len).map_err(|e| Error::custom("oodle", e))?;
                Ok(Bytes::from(decoded))
            }
            _ => Err(unsupported("streamed mips with an unknown codec")),
        }
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
        Ok((asset.entry.kind == *b"txtr").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

/// The NUL-terminated name at `.mem` offset `at`, if it fits in 512 bytes.
pub(crate) async fn read_name(volume: &Operator, at: u64, mem_len: u64) -> Option<String> {
    if at >= mem_len {
        return None;
    }
    let end = (at + 512).min(mem_len);
    let bytes = volume.read_with(MEM).range(at..end).await.ok()?.to_vec();
    let nul = bytes.iter().position(|&b| b == 0)?;
    let name = String::from_utf8_lossy(&bytes[..nul]).into_owned();
    (!name.is_empty()).then_some(name)
}

/// The length of `path` in the volume. Errors the volume raised itself (`MissingDependency`)
/// come back as they were.
async fn stat_len(volume: &Operator, path: &str) -> Result<u64> {
    Ok(volume.stat(path).await?.content_length())
}

#[async_trait]
impl ReadHandler for TxtrRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "txtr must be read from an rpak.asset".into(),
            })
        })?;
        let header = TxtrHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let pixel = pixel_format(header.format_index).ok_or_else(|| {
            unsupported(format!(
                "{} (pixel format #{})",
                header.version, header.format_index
            ))
        })?;

        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let data_offset = asset
            .data_mem_offset
            .ok_or_else(|| corrupt("txtr has no data pointer"))?;

        let name = match header.name_offset {
            Some(at) => read_name(&asset.volume, at, mem_len).await,
            None => None,
        }
        .map(|n| {
            if n.to_ascii_lowercase().starts_with("texture/") {
                n
            } else {
                format!("texture/{n}")
            }
        });
        let label = name.as_deref().unwrap_or("(unnamed)");

        // The permanent mips (shifts `streamed + optional ..`), smallest first in the CPU data.
        let first_permanent = header.first_permanent_shift();
        let total = header.total_mips();
        let mut cursor = 0u64;
        let mut by_shift = Vec::new();
        for shift in (first_permanent..total).rev() {
            let len = pixel.mip_len(
                (header.width >> shift).max(1),
                (header.height >> shift).max(1),
            );
            let stride = len.div_ceil(16).max(1) * 16;
            by_shift.push((
                shift,
                MipLoc {
                    store: Store::Mem,
                    offset: cursor,
                    stride,
                    len,
                    codec: 0,
                },
            ));
            cursor += stride * u64::from(header.array_size);
        }
        if data_offset
            .checked_add(cursor)
            .is_none_or(|end| end > mem_len)
        {
            return Err(corrupt("txtr mips run past the end of the pak's pages"));
        }

        // The streamed mips. A starpak that is missing, or has no entry for this asset, makes the
        // mandatory mips `MissingDependency`; the optional ones are dropped with a warning.
        if header.streamed > 0 {
            let path = asset.stream.as_deref().ok_or_else(|| {
                Error::new(ErrorKind::MissingDependency {
                    item: label.to_owned(),
                    file: "(no mandatory starpak reference)".into(),
                })
            })?;
            let size = match stat_len(&asset.volume, path).await {
                Ok(n) => n,
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    return Err(Error::new(ErrorKind::MissingDependency {
                        item: label.to_owned(),
                        file: format!("{path} (no entry in the starpak)"),
                    }));
                }
                Err(e) => return Err(e),
            };
            let mips = layout_entry(
                &header,
                pixel,
                Store::Stream,
                header.optional..header.optional + header.streamed,
            )?;
            if entry_len(&header, &mips) > size {
                return Err(corrupt(format!("texture {label}: its streamed mips do not fit the starpak entry ({size} bytes)")));
            }
            by_shift.extend(mips);
        }
        let mut first = 0;
        if header.optional > 0 {
            let usable = match asset.opt.as_deref() {
                None => Err("no optional starpak reference".to_owned()),
                Some(path) => match stat_len(&asset.volume, path).await {
                    Ok(n) => Ok(n),
                    Err(e)
                        if matches!(
                            e.kind(),
                            ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
                        ) =>
                    {
                        Err(format!("{path} is not available ({e})"))
                    }
                    Err(e) => return Err(e),
                },
            };
            match usable {
                Ok(size) => {
                    let mips = layout_entry(&header, pixel, Store::Opt, 0..header.optional)?;
                    if entry_len(&header, &mips) > size {
                        return Err(corrupt(format!("texture {label}: its optional mips do not fit the starpak entry ({size} bytes)")));
                    }
                    by_shift.extend(mips);
                }
                Err(why) => {
                    first = header.optional;
                    cx.warn(format!(
                        "texture {label}: {} optional mip(s) are skipped: {why}",
                        header.optional
                    ));
                }
            }
        }

        // Level 0 is the largest available mip.
        by_shift.sort_by_key(|(shift, _)| *shift);
        let mips: Vec<MipLoc> = by_shift.into_iter().map(|(_, m)| m).collect();
        let info = BitmapInfo {
            width: (header.width >> first).max(1),
            height: (header.height >> first).max(1),
            depth: 1,
            array_len: header.array_size,
            faces: 1,
            mip_count: total - first,
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
            stream: asset.stream.clone(),
            opt: asset.opt.clone(),
        }))
    }
}
