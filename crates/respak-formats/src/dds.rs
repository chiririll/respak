//! `dds`: uncompressed RGBA8 only (BC passthrough: slice 08, BC decode: slice 09).
//! Read parses the header only; `read_mip` does a ranged read of that mip.

use async_trait::async_trait;
use respak_core::capability::BitmapInfo;
use respak_core::{ConvertHandler, Cx, Detect, Detected, Probe, ReadHandler, Result, Target, Value};

/// A DDS file: header parsed, pixels left on the operator. `Data` + `Bitmap`.
#[derive(Debug)]
pub struct DdsImage {
    info: BitmapInfo,
    // + the file value (op, path), data offset
}

pub struct DdsRead;
pub struct DdsEncode;

#[async_trait]
impl Detect for DdsRead {
    /// Magic `DDS ` = 10, plus extension `dds` = +1.
    async fn score(&self, input: &Probe<'_>, cx: &Cx) -> Result<Option<Detected>> {
        todo!()
    }
    fn header_len(&self) -> usize {
        4
    }
}

#[async_trait]
impl ReadHandler for DdsRead {
    /// Any pixel format other than RGBA8/BGRA8 -> `UnsupportedVersion`-style error for now.
    async fn read(&self, source: Value, detected: &Detected, cx: &Cx) -> Result<Value> {
        todo!()
    }
}

#[async_trait]
impl ConvertHandler for DdsEncode {
    /// Bitmap -> `blob` tagged `dds` (legacy header with RGBA masks, all mips of layer 0).
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        todo!()
    }
}
