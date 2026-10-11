//! The [`Bitmap`] capability and the owned [`BitmapBuf`].

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::{Error, ErrorKind, Result};

/// How pixels are stored. Block-compressed formats (BC) keep their blocks as-is, so converters
/// between BC-capable formats can copy them without decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)] // variant names are the standard format names
pub enum PixelFormat {
    R8,
    Rg8,
    Rgba8,
    Bgra8,
    R16,
    R16F,
    Rgba16F,
    R32F,
    Rgba32F,
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6H,
    Bc7,
}

impl PixelFormat {
    /// Whether pixels are stored as 4×4 compressed blocks.
    pub fn is_block_compressed(self) -> bool {
        matches!(self, Self::Bc1 | Self::Bc2 | Self::Bc3 | Self::Bc4 | Self::Bc5 | Self::Bc6H | Self::Bc7)
    }
}

/// The shape of a bitmap. Mip 0 is the largest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapInfo {
    /// Width of mip 0 in pixels.
    pub width: u32,
    /// Height of mip 0 in pixels.
    pub height: u32,
    /// Depth of mip 0 for volume textures, 1 otherwise.
    pub depth: u32,
    /// Array length, 1 if not an array.
    pub array_len: u32,
    /// Faces per array element: 6 for a cube map, 1 otherwise.
    pub faces: u32,
    /// Number of mip levels, at least 1.
    pub mip_count: u32,
    /// Pixel storage.
    pub format: PixelFormat,
}

impl BitmapInfo {
    /// Number of layers addressable by `read_mip`: `array_len * faces`.
    pub fn layer_count(&self) -> u32 {
        self.array_len * self.faces
    }
}

/// Pixel data with dimensions, depth, array/faces, mips and a pixel format.
///
/// A lazy view over the format's native type: `read_mip` reads (and decodes) only the requested
/// mip, so it is async and may do ranged reads. A converter asks for the mips it needs.
#[async_trait]
pub trait Bitmap: Send + Sync {
    /// The bitmap's shape.
    fn info(&self) -> BitmapInfo;

    /// The raw bytes of one mip of one layer (`layer < info().layer_count()`, face-major within
    /// an array element), in `info().format`.
    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes>;
}

/// An owned bitmap, for converters that create pixel data (decoders, e.g. PNG read).
#[derive(Debug, Clone)]
pub struct BitmapBuf {
    info: BitmapInfo,
    /// `mips[layer][level]`
    mips: Vec<Vec<Bytes>>,
}

impl BitmapBuf {
    /// Builds a bitmap from `mips[layer][level]`. The shape must match `info`.
    pub fn new(info: BitmapInfo, mips: Vec<Vec<Bytes>>) -> Result<Self> {
        let ok = mips.len() == info.layer_count() as usize
            && mips.iter().all(|l| l.len() == info.mip_count as usize);
        if !ok {
            return Err(Error::new(ErrorKind::Corrupt {
                context: "bitmap buffer does not match its info".into(),
            }));
        }
        Ok(Self { info, mips })
    }
}

#[async_trait]
impl Bitmap for BitmapBuf {
    fn info(&self) -> BitmapInfo {
        self.info.clone()
    }

    async fn read_mip(&self, level: u32, layer: u32) -> Result<Bytes> {
        self.mips
            .get(layer as usize)
            .and_then(|l| l.get(level as usize))
            .cloned()
            .ok_or_else(|| {
                Error::new(ErrorKind::NotFound { path: format!("mip {level} of layer {layer}") })
            })
    }
}
