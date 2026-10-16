//! `png.frames`: written from any `Video`, one PNG per frame. Not readable.
//!
//! The frames are kept in memory until the whole video is done (the engine writes a composite
//! output in one go), so a long video needs memory for all of its PNGs.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use respak_core::capability::FrameFormat;
use respak_core::value::{BlobData, BundleData};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, Event, FormatId, Result, Target, TypedTarget, Value,
    ValueKey,
};
use serde::{Deserialize, Serialize};

use crate::png::encode_rgba8;
use crate::{corrupt, video_of};

/// Capability edge `Video -> png.frames`.
pub struct FramesEncode;

/// Options of the `png.frames` target: `{"type":"png.frames"}`, optionally
/// `{"type":"png.frames","from":3000,"step":500,"count":10}`. Every frame is still decoded (a
/// frame is predicted from the one before it), but only the selected ones are encoded and kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FramesOptions {
    /// First frame to keep, numbered as in the file names (`frame_00001.png` is 1).
    pub from: u32,
    /// Keep every `step`-th frame from `from` on.
    pub step: u32,
    /// Keep at most this many frames; all of them when absent.
    pub count: Option<u32>,
}

impl Default for FramesOptions {
    fn default() -> Self {
        Self {
            from: 1,
            step: 1,
            count: None,
        }
    }
}

#[async_trait]
impl ConvertHandler for FramesEncode {
    /// Asks the video for `Rgba8` frames and returns a composite output
    /// (`BundleData::named`: `frame_00001.png`, ...), so the engine gives it its own folder.
    /// Checks cancellation per frame and emits `Progress` before each. A video that fails
    /// part-way (damaged or truncated) fails the whole conversion; no partial folder is returned.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<FramesOptions>::parse(target)?;
        let video = video_of(&source)?;
        let select = options.options();
        let info = video.info();
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            target.format.unversioned(),
            options.normalized(),
        );
        let png = FormatId::new("png");
        let expected = info.width as usize * info.height as usize * 4;

        let invalid = |message: &str| {
            Error::new(ErrorKind::InvalidOptions {
                message: message.into(),
            })
        };
        if select.from == 0 {
            return Err(invalid("`from` counts frames from 1"));
        }
        if select.step == 0 {
            return Err(invalid("`step` must be at least 1"));
        }
        if select.count == Some(0) {
            return Err(invalid("`count` must be at least 1"));
        }

        let mut frames = video.frames(FrameFormat::Rgba8, cx).await?;
        let mut items: Vec<(String, Value)> = Vec::new();
        let mut seen = 0u64;
        while let Some(frame) = frames.next().await {
            let frame = frame?;
            cx.check_cancelled()?;
            seen += 1;
            cx.mediator.publish(Event::Progress {
                task: "png frames".into(),
                done: u64::from(frame.index),
                total: u64::from(info.frame_count),
            });
            let number = frame.index + 1;
            if number < select.from || (number - select.from) % select.step != 0 {
                continue;
            }
            let pixels = match (frame.format, frame.planes.as_slice()) {
                (FrameFormat::Rgba8, [pixels]) if pixels.len() == expected => pixels.clone(),
                _ => {
                    return Err(corrupt(format!(
                        "frame {}: expected {expected} bytes of RGBA8 for {}x{}",
                        frame.index, info.width, info.height
                    )))
                }
            };
            let (width, height) = (info.width, info.height);
            let bytes = cx
                .blocking(move || encode_rgba8(&pixels, width, height))
                .await??;
            let item_key = ValueKey::converted(
                Arc::new(key.clone()),
                png.clone(),
                &serde_json::json!({ "frame": frame.index }),
            );
            items.push((
                format!("frame_{:05}.png", frame.index + 1),
                Arc::new(BlobData::with_format(
                    png.clone(),
                    Bytes::from(bytes),
                    item_key,
                )),
            ));
            if select
                .count
                .is_some_and(|count| items.len() >= count as usize)
            {
                break;
            }
        }
        if seen == 0 {
            return Err(corrupt("the video has no frames"));
        }
        if items.is_empty() {
            return Err(invalid(&format!(
                "no frame selected: the video has {seen} frames"
            )));
        }
        Ok(Arc::new(BundleData::named(items, key)))
    }
}
