//! `png`: read from `file` (whole file, PNG is one deflate stream), decoded to RGBA8; encode
//! from any `Bitmap` that is RGBA8/BGRA8 (mip 0, layer 0).

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use bytes::Bytes;
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::value::{BlobData, BundleData};
use respak_core::{
    magic, ConvertHandler, Cx, Data, Detect, Detected, Error, ErrorKind, Event, FormatId, Probe, ReadHandler, Result, Target,
    TypedTarget, Value, ValueKey,
};

use crate::bc::decode_rgba8;
use crate::{bitmap_of, corrupt, file_of, read_all, unsupported};

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

/// Which mip to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Mips {
    /// A mip level, 0 = the largest.
    Level(u32),
    /// `"first"` = the largest mip; `"all"` = every mip, written into a folder.
    Keyword(String),
}

impl Default for Mips {
    fn default() -> Self {
        Self::Keyword("first".into())
    }
}

/// Options of the `png` target: `{"type":"png","mips":2}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PngOptions {
    /// Which mip to write; default `"first"`.
    pub mips: Mips,
}

/// What `mips` asks for.
enum Selection {
    /// One mip of layer 0.
    One(u32),
    /// Every mip of every layer, as a composite output.
    All,
}

impl PngOptions {
    fn selection(&self) -> Result<Selection> {
        let invalid = |message: String| Error::new(ErrorKind::InvalidOptions { message });
        match &self.mips {
            Mips::Level(n) => Ok(Selection::One(*n)),
            Mips::Keyword(k) if k == "first" => Ok(Selection::One(0)),
            Mips::Keyword(k) if k == "all" => Ok(Selection::All),
            Mips::Keyword(k) => {
                Err(invalid(format!("mips must be \"first\", \"all\" or a mip level number, not \"{k}\"")))
            }
        }
    }
}

/// One mip of one layer as a PNG file.
async fn encode_mip(bitmap: &dyn Bitmap, level: u32, layer: u32, cx: &Cx) -> Result<Vec<u8>> {
    let info = bitmap.info();
    let (width, height) = ((info.width >> level).max(1), (info.height >> level).max(1));
    let raw = bitmap.read_mip(level, layer).await?;
    // Decoding and compressing are CPU work: off the async threads.
    cx.blocking(move || {
        let pixels = decode_rgba8(info.format, width, height, &raw)?;
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| Error::custom("png encode", e))?;
        writer.write_image_data(&pixels).map_err(|e| Error::custom("png encode", e))?;
        writer.finish().map_err(|e| Error::custom("png encode", e))?;
        Ok(out)
    })
    .await?
}

#[async_trait]
impl ConvertHandler for PngEncode {
    /// Bitmap -> a blob tagged `png`: one mip (option `mips`), layer 0, decoded to RGBA8. With
    /// `mips: "all"`: a bundle of `mip<N>.png` (`layer<L>_mip<N>.png` for arrays), which the
    /// engine writes into a folder.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<PngOptions>::parse(target)?;
        let selection = options.options().selection()?;
        let bitmap = bitmap_of(&source)?;
        let info = bitmap.info();
        if info.depth != 1 {
            return Err(unsupported("png", "volume textures"));
        }
        let key = ValueKey::converted(Arc::new(source.key().clone()), target.format.unversioned(), options.normalized());
        let png = FormatId::new("png");
        let blob = |bytes: Vec<u8>, key: ValueKey| -> Value {
            Arc::new(BlobData::with_format(png.clone(), Bytes::from(bytes), key))
        };

        match selection {
            Selection::One(level) => {
                if level >= info.mip_count {
                    return Err(Error::new(ErrorKind::InvalidOptions {
                        message: format!("mips {level}: the bitmap has {} mip(s)", info.mip_count),
                    }));
                }
                if info.layer_count() > 1 {
                    cx.warn(format!("png: the bitmap has {} layers, writing layer 0 only", info.layer_count()));
                }
                Ok(blob(encode_mip(bitmap, level, 0, cx).await?, key))
            }
            Selection::All => {
                let layers = info.layer_count();
                let mut items = Vec::new();
                let total = u64::from(layers) * u64::from(info.mip_count);
                for layer in 0..layers {
                    for level in 0..info.mip_count {
                        cx.check_cancelled()?;
                        cx.mediator.publish(Event::Progress {
                            task: "png mips".into(),
                            done: items.len() as u64,
                            total,
                        });
                        let name = if layers == 1 {
                            format!("mip{level}.png")
                        } else {
                            format!("layer{layer}_mip{level}.png")
                        };
                        let item_key = ValueKey::converted(
                            Arc::new(key.clone()),
                            FormatId::new("png"),
                            &serde_json::json!({ "layer": layer, "mip": level }),
                        );
                        items.push((name, blob(encode_mip(bitmap, level, layer, cx).await?, item_key)));
                    }
                }
                Ok(Arc::new(BundleData::named(items, key)))
            }
        }
    }
}
