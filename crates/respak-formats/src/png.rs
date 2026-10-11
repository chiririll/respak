//! `png`: read from `file` (whole file, PNG is one deflate stream), decoded to RGBA8; encode
//! from any `Bitmap` that is RGBA8/BGRA8 (mip 0, layer 0).

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::value::BlobData;
use respak_core::{
    magic, ConvertHandler, Cx, Data, Detect, Detected, FormatId, Probe, ReadHandler, Result, Target, Value, ValueKey,
};

use crate::{bitmap_of, corrupt, file_of, read_all, swap_rb, unsupported};

const MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A decoded PNG: `Data` + `Bitmap` (RGBA8, 1 mip).
#[derive(Debug)]
pub struct PngImage {
    bitmap: BitmapBuf,
    format: FormatId,
    key: ValueKey,
}

impl Data for PngImage {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn approx_size(&self) -> usize {
        let i = self.bitmap.info();
        i.width as usize * i.height as usize * 4
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(&self.bitmap)
    }
}

/// Read edge `file -> png`.
pub struct PngRead;
/// Capability edge `Bitmap -> png`.
pub struct PngEncode;

#[async_trait]
impl Detect for PngRead {
    /// Magic = 10, `IHDR` as the first chunk = +2, extension `png` = +1. The extension alone
    /// does not make a PNG.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, &MAGIC) {
            return Ok(None);
        }
        let ihdr = u32::from(magic(input.header, 12, b"IHDR")) * 2;
        let ext = u32::from(input.extension == Some("png"));
        Ok(Some(Detected { score: 10 + ihdr + ext, version: None }))
    }

    fn header_len(&self) -> usize {
        16
    }
}

#[async_trait]
impl ReadHandler for PngRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let bytes = read_all(file_of(&source)?).await?;
        let bitmap = decode(&bytes)?;
        Ok(Arc::new(PngImage { bitmap, format: FormatId::new("png"), key: source.key().clone() }))
    }
}

fn decode(bytes: &[u8]) -> Result<BitmapBuf> {
    let bad = |e: png::DecodingError| corrupt(format!("png: {e}"));
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(bad)?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or_else(|| corrupt("png: image too large"))?];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(frame.buffer_size());

    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        // Palettes are expanded by the decoder.
        png::ColorType::Indexed => return Err(corrupt("png: palette was not expanded")),
    };
    let info = BitmapInfo {
        width: frame.width,
        height: frame.height,
        depth: 1,
        array_len: 1,
        faces: 1,
        mip_count: 1,
        format: PixelFormat::Rgba8,
    };
    BitmapBuf::new(info, vec![vec![Bytes::from(rgba)]])
}

#[async_trait]
impl ConvertHandler for PngEncode {
    /// Bitmap -> a blob tagged `png`.
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let bitmap = bitmap_of(&source)?;
        let info = bitmap.info();
        if info.layer_count() != 1 || info.depth != 1 {
            return Err(unsupported("png", "arrays, cube maps and volumes"));
        }
        let mut pixels = bitmap.read_mip(0, 0).await?.to_vec();
        match info.format {
            PixelFormat::Rgba8 => {}
            PixelFormat::Bgra8 => swap_rb(&mut pixels),
            other => return Err(unsupported("png", &format!("pixel format {other:?}"))),
        }
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, info.width, info.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| respak_core::Error::custom("png encode", e))?;
        writer.write_image_data(&pixels).map_err(|e| respak_core::Error::custom("png encode", e))?;
        writer.finish().map_err(|e| respak_core::Error::custom("png encode", e))?;

        let key = ValueKey::converted(Arc::new(source.key().clone()), target.format.unversioned(), &target.options);
        Ok(Arc::new(BlobData::with_format(FormatId::new("png"), Bytes::from(out), key)))
    }
}
