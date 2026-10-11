//! `dds`: uncompressed RGBA8/BGRA8 only (BC passthrough: slice 08, BC decode: slice 09).
//! Read parses the header only; `read_mip` does a ranged read of that mip.

use std::io::Cursor;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ddsfile::{AlphaMode, D3D10ResourceDimension, D3DFormat, Dds, DxgiFormat, NewD3dParams, NewDxgiParams};
use opendal::Operator;
use respak_core::capability::{Bitmap, BitmapInfo, PixelFormat};
use respak_core::value::BlobData;
use respak_core::{
    magic, u32le, ConvertHandler, Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Target,
    Value, ValueKey,
};

use crate::{bitmap_of, corrupt, file_of, unsupported};

const MAGIC: &[u8] = b"DDS ";
/// Magic + `DDS_HEADER`.
const HEADER_LEN: u64 = 128;
/// The optional `DDS_HEADER_DXT10`.
const HEADER10_LEN: u64 = 20;

/// A DDS file: header parsed, pixels left on the operator. `Data` + `Bitmap`.
#[derive(Debug)]
pub struct DdsImage {
    info: BitmapInfo,
    format: FormatId,
    key: ValueKey,
    op: Operator,
    path: String,
    /// `(offset in the file, length)` of each mip.
    mips: Vec<(u64, u64)>,
}

impl Data for DdsImage {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(self)
    }
}

#[async_trait]
impl Bitmap for DdsImage {
    fn info(&self) -> BitmapInfo {
        self.info.clone()
    }

    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes> {
        let &(offset, len) = self
            .mips
            .get(level as usize)
            .filter(|_| layer == 0)
            .ok_or_else(|| Error::new(ErrorKind::NotFound { path: format!("mip {level} of layer {layer}") }))?;
        Ok(self.op.read_with(&self.path).range(offset..offset + len).await?.to_bytes())
    }
}

/// Read edge `file -> dds`.
pub struct DdsRead;
/// Capability edge `Bitmap -> dds`.
pub struct DdsEncode;

#[async_trait]
impl Detect for DdsRead {
    /// Magic = 10, header size field 124 = +2, extension `dds` = +1.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, MAGIC) {
            return Ok(None);
        }
        let size = u32::from(u32le(input.header, 4) == Some(124)) * 2;
        let ext = u32::from(input.extension == Some("dds"));
        Ok(Some(Detected { score: 10 + size + ext, version: None }))
    }

    fn header_len(&self) -> usize {
        8
    }
}

fn mip_len(width: u32, height: u32, level: u32) -> u64 {
    PixelFormat::Rgba8.mip_len(width >> level, height >> level)
}

#[async_trait]
impl ReadHandler for DdsRead {
    /// Reads the header (at most 148 bytes); pixel data stays on the operator.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = file_of(&source)?;
        let start = file.range.as_ref().map_or(0, |r| r.start);
        let total = match &file.range {
            Some(r) => r.end - r.start,
            None => file.op.stat(&file.path).await?.content_length(),
        };
        let head_len = total.min(HEADER_LEN + HEADER10_LEN);
        let head = file.op.read_with(&file.path).range(start..start + head_len).await?.to_vec();
        let dds = Dds::read(Cursor::new(&head)).map_err(|e| corrupt(format!("dds header: {e}")))?;

        let format = match &dds.header10 {
            Some(h) => match h.dxgi_format {
                DxgiFormat::R8G8B8A8_UNorm => PixelFormat::Rgba8,
                DxgiFormat::B8G8R8A8_UNorm => PixelFormat::Bgra8,
                other => return Err(unsupported("dds", &format!("pixel format {other:?}"))),
            },
            None => match dds.get_d3d_format() {
                Some(D3DFormat::A8B8G8R8) => PixelFormat::Rgba8,
                Some(D3DFormat::A8R8G8B8) => PixelFormat::Bgra8,
                Some(other) => return Err(unsupported("dds", &format!("pixel format {other:?}"))),
                None => return Err(unsupported("dds", "unrecognised pixel format")),
            },
        };
        if dds.get_num_array_layers() != 1 || dds.header.depth.is_some_and(|d| d > 1) {
            return Err(unsupported("dds", "arrays, cube maps and volumes"));
        }

        let (width, height) = (dds.get_width(), dds.get_height());
        let mip_count = dds.get_num_mipmap_levels().max(1);
        let mut offset = start + HEADER_LEN + if dds.header10.is_some() { HEADER10_LEN } else { 0 };
        let mut mips = Vec::new();
        for level in 0..mip_count {
            let len = mip_len(width, height, level);
            mips.push((offset, len));
            offset += len;
        }
        if offset > start + total {
            return Err(corrupt("dds: file is shorter than its mip chain"));
        }

        let info = BitmapInfo { width, height, depth: 1, array_len: 1, faces: 1, mip_count, format };
        Ok(Arc::new(DdsImage {
            info,
            format: FormatId::new("dds"),
            key: source.key().clone(),
            op: file.op.clone(),
            path: file.path.clone(),
            mips,
        }))
    }
}

fn dxgi_format(f: PixelFormat) -> Option<DxgiFormat> {
    Some(match f {
        PixelFormat::R8 => DxgiFormat::R8_UNorm,
        PixelFormat::Rg8 => DxgiFormat::R8G8_UNorm,
        PixelFormat::Rgba8 => DxgiFormat::R8G8B8A8_UNorm,
        PixelFormat::Rgba8Srgb => DxgiFormat::R8G8B8A8_UNorm_sRGB,
        PixelFormat::Bgra8 => DxgiFormat::B8G8R8A8_UNorm,
        PixelFormat::R16 => DxgiFormat::R16_UNorm,
        PixelFormat::R16F => DxgiFormat::R16_Float,
        PixelFormat::Rgba16F => DxgiFormat::R16G16B16A16_Float,
        PixelFormat::R32F => DxgiFormat::R32_Float,
        PixelFormat::Rgba32F => DxgiFormat::R32G32B32A32_Float,
        PixelFormat::Bc1 => DxgiFormat::BC1_UNorm,
        PixelFormat::Bc1Srgb => DxgiFormat::BC1_UNorm_sRGB,
        PixelFormat::Bc2 => DxgiFormat::BC2_UNorm,
        PixelFormat::Bc2Srgb => DxgiFormat::BC2_UNorm_sRGB,
        PixelFormat::Bc3 => DxgiFormat::BC3_UNorm,
        PixelFormat::Bc3Srgb => DxgiFormat::BC3_UNorm_sRGB,
        PixelFormat::Bc4 => DxgiFormat::BC4_UNorm,
        PixelFormat::Bc4Snorm => DxgiFormat::BC4_SNorm,
        PixelFormat::Bc5 => DxgiFormat::BC5_UNorm,
        PixelFormat::Bc5Snorm => DxgiFormat::BC5_SNorm,
        PixelFormat::Bc6H => DxgiFormat::BC6H_UF16,
        PixelFormat::Bc6HSigned => DxgiFormat::BC6H_SF16,
        PixelFormat::Bc7 => DxgiFormat::BC7_UNorm,
        PixelFormat::Bc7Srgb => DxgiFormat::BC7_UNorm_sRGB,
        _ => return None,
    })
}

#[async_trait]
impl ConvertHandler for DdsEncode {
    /// Bitmap -> a blob tagged `dds`. Block-compressed data is copied as it is.
    ///
    /// A single-layer RGBA8/BGRA8 bitmap gets a legacy header with colour masks (readable by
    /// every viewer); everything else (BC, arrays, other formats) gets a DX10 header. All mips
    /// of every layer are written, layers outer and mips inner.
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let bitmap = bitmap_of(&source)?;
        let info = bitmap.info();
        if info.faces != 1 || info.depth != 1 {
            return Err(unsupported("dds", "cube maps and volumes"));
        }
        let mips = info.mip_count.max(1);
        let legacy = match info.format {
            PixelFormat::Rgba8 if info.array_len == 1 => Some(D3DFormat::A8B8G8R8),
            PixelFormat::Bgra8 if info.array_len == 1 => Some(D3DFormat::A8R8G8B8),
            _ => None,
        };
        let mut dds = match legacy {
            Some(format) => Dds::new_d3d(NewD3dParams {
                height: info.height,
                width: info.width,
                depth: None,
                format,
                mipmap_levels: (mips > 1).then_some(mips),
                caps2: None,
            }),
            None => {
                let format = dxgi_format(info.format)
                    .ok_or_else(|| unsupported("dds", &format!("pixel format {:?}", info.format)))?;
                Dds::new_dxgi(NewDxgiParams {
                    height: info.height,
                    width: info.width,
                    depth: None,
                    format,
                    mipmap_levels: (mips > 1).then_some(mips),
                    array_layers: (info.array_len > 1).then_some(info.array_len),
                    caps2: None,
                    is_cubemap: false,
                    resource_dimension: D3D10ResourceDimension::Texture2D,
                    alpha_mode: AlphaMode::Unknown,
                })
            }
        }
        .map_err(|e| Error::custom("dds encode", e))?;

        let mut data = Vec::with_capacity(dds.data.len());
        for layer in 0..info.array_len {
            for level in 0..mips {
                let mip = bitmap.read_mip(level, layer).await?;
                let want = info.format.mip_len(info.width >> level, info.height >> level);
                if mip.len() as u64 != want {
                    return Err(corrupt(format!("bitmap mip {level} of layer {layer} has the wrong size")));
                }
                data.extend_from_slice(&mip);
            }
        }
        if data.len() != dds.data.len() {
            return Err(corrupt("bitmap mip chain does not match its info"));
        }
        dds.data = data;

        let mut out = Vec::new();
        dds.write(&mut out).map_err(|e| Error::custom("dds encode", e))?;
        let key = ValueKey::converted(Arc::new(source.key().clone()), target.format.unversioned(), &target.options);
        Ok(Arc::new(BlobData::with_format(FormatId::new("dds"), Bytes::from(out), key)))
    }
}
