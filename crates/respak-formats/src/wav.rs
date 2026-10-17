//! `wav`: written from any `Audio` (16-bit PCM). Not readable.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::{Deserialize, Serialize};

use crate::audio_of;

/// Size of the RIFF header written before the samples.
const HEADER_LEN: usize = 44;

/// Capability edge `Audio -> wav`.
pub struct WavEncode;

/// Options of the `wav` target: `{"type":"wav","track":1}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WavOptions {
    /// Which audio track to write; default 0.
    pub track: u32,
}

/// A sample in `-1.0..=1.0` as 16-bit PCM: rounded to the nearest value, clamped, `NaN` is 0.
pub(crate) fn pcm16(sample: f32) -> i16 {
    (sample * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

/// The 44-byte header of a PCM wav holding `data_len` bytes of samples.
fn header(channels: u16, sample_rate: u32, data_len: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    let block_align = channels * 2;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(data_len + HEADER_LEN as u32 - 8).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

#[async_trait]
impl ConvertHandler for WavEncode {
    /// Streams the track's chunks into one RIFF file. No audio at all is a `Custom` error that
    /// says so; a track that does not exist is `InvalidOptions`. A chunk that is not a whole
    /// number of frames is `Corrupt`. Checks cancellation per chunk.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<WavOptions>::parse(target)?;
        let audio = audio_of(&source)?;
        let tracks = audio.tracks();
        if tracks.is_empty() {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: format!("`{}` has no audio", source.format()),
            }));
        }
        let track = options.options().track;
        let info = *tracks.get(track as usize).ok_or_else(|| {
            Error::new(ErrorKind::InvalidOptions {
                message: format!(
                    "track {track}: the source has {} audio track(s)",
                    tracks.len()
                ),
            })
        })?;
        if info.channels == 0 || info.sample_rate == 0 {
            return Err(Error::corrupt(format!(
                "audio track {track} has {} channels at {} Hz",
                info.channels, info.sample_rate
            )));
        }

        let mut chunks = audio.samples(track, cx).await?;
        let mut data: Vec<u8> = Vec::new();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk?;
            cx.check_cancelled()?;
            if chunk.samples.len() % usize::from(info.channels) != 0 {
                return Err(Error::corrupt(format!(
                    "audio chunk of {} samples is not whole frames of {} channels",
                    chunk.samples.len(),
                    info.channels
                )));
            }
            data.extend(chunk.samples.iter().flat_map(|&s| pcm16(s).to_le_bytes()));
        }
        let data_len = u32::try_from(data.len())
            .ok()
            .filter(|n| n.checked_add(HEADER_LEN as u32).is_some())
            .ok_or_else(|| Error::unsupported("wav", "audio over 4 GiB"))?;

        let mut out = Vec::with_capacity(HEADER_LEN + data.len());
        out.extend_from_slice(&header(info.channels, info.sample_rate, data_len));
        out.extend_from_slice(&data);
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            target.format.unversioned(),
            options.normalized(),
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("wav"),
            Bytes::from(out),
            key,
        )))
    }
}
