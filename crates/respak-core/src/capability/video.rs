//! The [`Video`] capability.

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;

use crate::error::Result;
use crate::Cx;

/// Pixel layout of the frames a consumer asks for. The consumer chooses; the video converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameFormat {
    /// One plane, 4 bytes per pixel (R, G, B, A), tightly packed.
    Rgba8,
    /// Three planes (Y, U, V), 8 bit, as decoded (no conversion): U and V have half the width and
    /// height of Y, rounded up.
    Yuv420,
}

/// The shape of a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frame rate as numerator and denominator.
    pub fps: (u32, u32),
    /// Number of frames.
    pub frame_count: u32,
    /// Whether [`FrameFormat::Yuv420`] samples use the full 0..=255 range (otherwise 16..=235).
    pub full_range: bool,
}

/// One decoded frame.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Position in the video, from 0.
    pub index: u32,
    /// How `planes` are laid out.
    pub format: FrameFormat,
    /// One plane for [`FrameFormat::Rgba8`], three (Y, U, V) for [`FrameFormat::Yuv420`].
    pub planes: Vec<Bytes>,
    /// Bytes per row of each plane.
    pub strides: Vec<usize>,
}

/// A video: frames in order.
///
/// A lazy view over the format's native type. Frames depend on each other, so there is no random
/// access: a consumer takes the stream from the start. Decoding is CPU-bound; an implementation
/// runs it through [`Cx::blocking`] and checks cancellation between frames.
#[async_trait]
pub trait Video: Send + Sync {
    /// The video's shape.
    fn info(&self) -> VideoInfo;

    /// Decodes the frames in order, converted to `format`. The stream ends after the last frame;
    /// a damaged or truncated video yields frames up to the damage, then one `Err`.
    async fn frames(
        &self,
        format: FrameFormat,
        cx: &Cx,
    ) -> Result<BoxStream<'static, Result<VideoFrame>>>;
}
