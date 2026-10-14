//! The [`Audio`] capability.

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::error::Result;
use crate::Cx;

/// The shape of one audio track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrackInfo {
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Samples per second, per channel.
    pub sample_rate: u32,
}

/// A run of decoded samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    /// Interleaved samples (`L R L R ...`), nominally in `-1.0..=1.0`.
    pub samples: Vec<f32>,
}

/// Sound: one or more tracks, each a stream of samples.
///
/// A lazy view over the format's native type. Decoding is CPU-bound; an implementation runs it
/// through [`Cx::blocking`] and checks cancellation between chunks.
#[async_trait]
pub trait Audio: Send + Sync {
    /// The tracks, in order. Empty if there is no sound.
    fn tracks(&self) -> Vec<AudioTrackInfo>;

    /// Decodes track `track` (`< tracks().len()`) from the start. A damaged or truncated track
    /// yields chunks up to the damage, then one `Err`.
    async fn samples(&self, track: u32, cx: &Cx) -> Result<BoxStream<'static, Result<AudioChunk>>>;
}
