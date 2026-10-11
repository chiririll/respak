//! `png`: read from `file` (whole file; PNG is one deflate stream), decoded to RGBA8; encode
//! from any `Bitmap` that is RGBA8/BGRA8 (mip 0, layer 0).

use async_trait::async_trait;
use respak_core::capability::BitmapBuf;
use respak_core::{ConvertHandler, Cx, Detect, Detected, Probe, ReadHandler, Result, Target, Value};

/// A decoded PNG: `Data` + `Bitmap` (RGBA8, 1 mip).
#[derive(Debug)]
pub struct PngImage {
    bitmap: BitmapBuf,
    // format id "png", key = the file's key
}

pub struct PngRead;
pub struct PngEncode;

#[async_trait]
impl Detect for PngRead {
    /// Magic `89 50 4E 47 0D 0A 1A 0A` = 10, plus extension `png` = +1.
    async fn score(&self, input: &Probe<'_>, cx: &Cx) -> Result<Option<Detected>> {
        todo!()
    }
    fn header_len(&self) -> usize {
        8
    }
}

#[async_trait]
impl ReadHandler for PngRead {
    async fn read(&self, source: Value, detected: &Detected, cx: &Cx) -> Result<Value> {
        todo!()
    }
}

#[async_trait]
impl ConvertHandler for PngEncode {
    /// Bitmap -> `blob` tagged `png`.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        todo!()
    }
}
