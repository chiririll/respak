//! Video files (`avi`, ...): written from any `Video`, with the sound of any `Audio` it also is.
//!
//! This module is the part that does not know any codec or container library. A [`Codec`] turns
//! frames into packets, a [`Container`] lays packets and PCM out in a file, and
//! [`VideoFileEncode`] drives them: it pulls frames (and audio) from the source, runs the encoder,
//! and interleaves what comes out. A backend crate supplies the codecs and containers and
//! registers the formats (`respak-nihav`).
//!
//! The result is a [`LazyBlob`]: nothing is decoded or encoded until the writer reads it. Most
//! containers seek back to patch their header, so the file is muxed into an anonymous temporary
//! file and the blob serves it from there; the output writer then copies it in chunks. Memory use
//! is a few frames, however long the video is.

use std::collections::VecDeque;
use std::fs::File;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use respak_core::capability::{AudioChunk, AudioTrackInfo, FrameFormat, VideoFrame, VideoInfo};
use respak_core::value::{LazyBlob, LazySource};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, Event, FormatId, Result, Target, TypedTarget, Value,
    ValueKey,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::wav::pcm16;
use crate::{corrupt, video_of};

/// One encoded frame.
#[derive(Debug, Clone)]
pub struct Packet {
    /// The codec's data for one frame.
    pub data: Vec<u8>,
    /// Whether the frame can be decoded without the ones before it.
    pub keyframe: bool,
}

/// What a [`Codec`] is opened for.
#[derive(Debug, Clone, Copy)]
pub struct CodecSetup {
    /// Width of the source frames.
    pub width: u32,
    /// Height of the source frames.
    pub height: u32,
    /// Frame rate as numerator and denominator.
    pub fps: (u32, u32),
    /// Whether `Yuv420` samples of the source use the full 0..=255 range.
    pub full_range: bool,
    /// Quality, 1..=100; what it means is up to the codec.
    pub quality: u8,
}

/// An encoder running on one video.
pub trait FrameEncoder: Send {
    /// The frame format the encoder takes; the source converts to it.
    fn wants(&self) -> FrameFormat;

    /// The size of the coded frames, which may be larger than the source's (some codecs need
    /// multiples of 16); the container declares this size.
    fn coded_size(&self) -> (u32, u32);

    /// Encodes `frame`, appending the packets that are ready to `out` (possibly none).
    fn encode(&mut self, frame: &VideoFrame, out: &mut Vec<Packet>) -> Result<()>;

    /// Ends the video, appending the packets that were held back.
    fn finish(&mut self, out: &mut Vec<Packet>) -> Result<()>;
}

/// A video codec.
pub trait Codec: Send + Sync {
    /// The name used in the `codec` option, e.g. `mjpeg`.
    fn name(&self) -> &'static str;

    /// Opens an encoder for a video of the given shape.
    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>>;
}

/// The streams of the file a [`Container`] is asked to write.
#[derive(Debug, Clone, Copy)]
pub struct MuxSetup<'a> {
    /// The [`Codec::name`] of the video.
    pub codec: &'a str,
    /// The coded size of the video ([`FrameEncoder::coded_size`]).
    pub size: (u32, u32),
    /// Frame rate as numerator and denominator.
    pub fps: (u32, u32),
    /// The audio track, which is written as 16-bit little-endian PCM.
    pub audio: Option<AudioTrackInfo>,
}

/// Where a [`Container`] takes its packets from.
pub trait PacketSink {
    /// The next video packet.
    fn video(&mut self, packet: Packet) -> Result<()>;

    /// The next run of audio: interleaved 16-bit little-endian samples, a whole number of frames.
    fn audio(&mut self, pcm: &[u8]) -> Result<()>;
}

/// A container format that writes a file.
pub trait Container: Send + Sync {
    /// The format name, e.g. `avi`.
    fn name(&self) -> &'static str;

    /// The [`Codec::name`]s it can hold; the first is the default.
    fn codecs(&self) -> &[&'static str];

    /// Whether the format can carry sound; one that cannot (`y4m`) is written without it.
    fn has_audio(&self) -> bool {
        true
    }

    /// Writes a file to `file` (empty and seekable, so a header may be patched at the end): calls
    /// `feed` with the sink the streams go to, and finishes the file after it returns.
    fn mux(
        &self,
        setup: &MuxSetup<'_>,
        file: &mut File,
        feed: &mut dyn FnMut(&mut dyn PacketSink) -> Result<()>,
    ) -> Result<()>;
}

/// Options of a video file target: `{"type":"avi","codec":"mjpeg","quality":90}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct VideoFileOptions {
    /// The video codec; the container's default when absent.
    pub codec: Option<String>,
    /// Quality, 1..=100.
    pub quality: u8,
    /// The audio track to include; the first one when absent. A source without sound gives a file
    /// without sound.
    pub audio_track: Option<u32>,
    /// No sound.
    pub mute: bool,
}

impl Default for VideoFileOptions {
    fn default() -> Self {
        Self {
            codec: None,
            quality: 90,
            audio_track: None,
            mute: false,
        }
    }
}

/// Capability edge `Video -> <container>`: one per container format.
pub struct VideoFileEncode {
    container: Arc<dyn Container>,
    codecs: Vec<Arc<dyn Codec>>,
}

impl VideoFileEncode {
    /// An edge that writes `container` files with any of `codecs` the container can hold.
    pub fn new(container: Arc<dyn Container>, codecs: Vec<Arc<dyn Codec>>) -> Self {
        let codecs = codecs
            .into_iter()
            .filter(|c| container.codecs().contains(&c.name()))
            .collect();
        Self { container, codecs }
    }

    fn pick(&self, options: &VideoFileOptions) -> Result<Arc<dyn Codec>> {
        let invalid = |message: String| Error::new(ErrorKind::InvalidOptions { message });
        let want = options
            .codec
            .as_deref()
            .or_else(|| self.container.codecs().first().copied());
        let available = || {
            self.codecs
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match want {
            Some(name) => self
                .codecs
                .iter()
                .find(|c| c.name() == name)
                .cloned()
                .ok_or_else(|| {
                    invalid(format!(
                        "`{}` cannot hold codec `{name}` (available: {})",
                        self.container.name(),
                        available()
                    ))
                }),
            None => Err(invalid(format!(
                "`{}` has no codec available",
                self.container.name()
            ))),
        }
    }
}

#[async_trait]
impl ConvertHandler for VideoFileEncode {
    /// Checks the options and the source now and returns a [`LazyBlob`]; the video is decoded and
    /// encoded when the blob is first read. A bad codec or track is `InvalidOptions`.
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<VideoFileOptions>::parse(target)?;
        let invalid = |message: String| Error::new(ErrorKind::InvalidOptions { message });
        let video = video_of(&source)?;
        let info = video.info();
        let opts = options.options().clone();
        if !(1..=100).contains(&opts.quality) {
            return Err(invalid("`quality` is 1 to 100".into()));
        }
        let codec = self.pick(&opts)?;
        if opts.audio_track.is_some() && !self.container.has_audio() {
            return Err(invalid(format!(
                "`{}` has no sound, so there is no audio track to pick",
                self.container.name()
            )));
        }
        let track = match (opts.mute || !self.container.has_audio(), source.as_audio()) {
            (true, _) | (_, None) => None,
            (false, Some(audio)) => {
                let tracks = audio.tracks();
                match opts.audio_track {
                    Some(t) if t as usize >= tracks.len() => {
                        return Err(invalid(format!(
                            "no audio track {t}: the source has {}",
                            tracks.len()
                        )))
                    }
                    Some(t) => Some((t, tracks[t as usize])),
                    None => tracks.first().map(|t| (0, *t)),
                }
            }
        };

        let format = FormatId::new(self.container.name());
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            format.unversioned(),
            options.normalized(),
        );
        let container = self.container.clone();
        let init = Box::new(move |cx: Cx| {
            Box::pin(async move { produce(source, info, codec, container, opts, track, cx).await })
                as futures::future::BoxFuture<'static, Result<LazySource>>
        });
        Ok(Arc::new(LazyBlob::new(format, key, init)))
    }
}

/// The whole conversion, run by the first read of the blob.
async fn produce(
    source: Value,
    info: VideoInfo,
    codec: Arc<dyn Codec>,
    container: Arc<dyn Container>,
    options: VideoFileOptions,
    track: Option<(u32, AudioTrackInfo)>,
    cx: Cx,
) -> Result<LazySource> {
    let setup = CodecSetup {
        width: info.width,
        height: info.height,
        fps: info.fps,
        full_range: info.full_range,
        quality: options.quality,
    };
    let encoder = codec.open(&setup)?;
    let frames = video_of(&source)?.frames(encoder.wants(), &cx).await?;
    let audio = match (track, source.as_audio()) {
        (Some((index, shape)), Some(audio)) => Some((shape, audio.samples(index, &cx).await?)),
        _ => None,
    };
    let job = Job {
        info,
        codec_name: codec.name(),
        encoder,
        container,
        frames,
        audio,
        handle: Handle::current(),
        cx: cx.clone(),
    };
    // The source stays alive until the file is done: its frames are decoded from it.
    let file = cx
        .blocking(move || job.run().map(|file| (file, source)))
        .await??
        .0;
    Ok(LazySource::File(file))
}

struct Job {
    info: VideoInfo,
    codec_name: &'static str,
    encoder: Box<dyn FrameEncoder>,
    container: Arc<dyn Container>,
    frames: BoxStream<'static, Result<VideoFrame>>,
    audio: Option<(AudioTrackInfo, BoxStream<'static, Result<AudioChunk>>)>,
    handle: Handle,
    cx: Cx,
}

impl Job {
    /// Encodes and muxes everything into a temporary file. Runs on a blocking thread, so it
    /// waits for the (async) frame and sample streams with `Handle::block_on`.
    fn run(self) -> Result<File> {
        let Self {
            info,
            codec_name,
            mut encoder,
            container,
            mut frames,
            audio,
            handle,
            cx,
        } = self;
        let mut file =
            tempfile::tempfile().map_err(|e| storage(e, "creating the temporary file"))?;
        let setup = MuxSetup {
            codec: codec_name,
            size: encoder.coded_size(),
            fps: info.fps,
            audio: audio.as_ref().map(|(shape, _)| *shape),
        };
        let mut audio = audio.map(|(shape, stream)| AudioFeed::new(shape, stream));
        let mut packets = Vec::new();

        container.mux(&setup, &mut file, &mut |sink| {
            let mut seen = 0u32;
            while let Some(frame) = handle.block_on(frames.next()) {
                cx.check_cancelled()?;
                let frame = frame?;
                cx.mediator.publish(Event::Progress {
                    task: "video encode".into(),
                    done: u64::from(frame.index),
                    total: u64::from(info.frame_count),
                });
                encoder.encode(&frame, &mut packets)?;
                for packet in packets.drain(..) {
                    sink.video(packet)?;
                }
                seen += 1;
                if let Some(feed) = audio.as_mut() {
                    // The audio that plays during this frame follows it.
                    let pcm = handle.block_on(feed.take_until(seen, info.fps))?;
                    if !pcm.is_empty() {
                        sink.audio(&pcm)?;
                    }
                }
            }
            if seen == 0 {
                return Err(corrupt("the video has no frames"));
            }
            encoder.finish(&mut packets)?;
            for packet in packets.drain(..) {
                sink.video(packet)?;
            }
            if let Some(feed) = audio.as_mut() {
                let rest = handle.block_on(feed.take_rest())?;
                if !rest.is_empty() {
                    sink.audio(&rest)?;
                }
            }
            Ok(())
        })?;
        Ok(file)
    }
}

fn storage(e: std::io::Error, what: &str) -> Error {
    Error::new(ErrorKind::Custom {
        source: Some(Box::new(e)),
        message: what.into(),
    })
}

/// An audio track, handed out in the runs that play during each video frame.
struct AudioFeed {
    shape: AudioTrackInfo,
    stream: Option<BoxStream<'static, Result<AudioChunk>>>,
    pending: VecDeque<f32>,
    /// Sample frames handed out so far.
    given: u64,
}

impl AudioFeed {
    fn new(shape: AudioTrackInfo, stream: BoxStream<'static, Result<AudioChunk>>) -> Self {
        Self {
            shape,
            stream: Some(stream),
            pending: VecDeque::new(),
            given: 0,
        }
    }

    /// The audio up to the end of video frame number `frames` (the frames shown so far), as PCM.
    /// Shorter if the track ends first.
    async fn take_until(&mut self, frames: u32, fps: (u32, u32)) -> Result<Vec<u8>> {
        // Sample frames that should have played when `frames` frames have been shown, rounded.
        let (num, den) = (u128::from(fps.0.max(1)), u128::from(fps.1));
        let wanted =
            (u128::from(frames) * u128::from(self.shape.sample_rate) * den + num / 2) / num;
        let missing = (wanted as u64).saturating_sub(self.given);
        self.take(missing).await
    }

    async fn take_rest(&mut self) -> Result<Vec<u8>> {
        self.take(u64::MAX).await
    }

    async fn take(&mut self, frames: u64) -> Result<Vec<u8>> {
        let channels = usize::from(self.shape.channels.max(1));
        let wanted = frames.saturating_mul(channels as u64);
        while (self.pending.len() as u64) < wanted {
            let Some(stream) = self.stream.as_mut() else {
                break;
            };
            match stream.next().await {
                Some(chunk) => self.pending.extend(chunk?.samples),
                None => self.stream = None,
            }
        }
        let n = (self.pending.len() as u64).min(wanted) as usize / channels * channels;
        self.given += (n / channels) as u64;
        let mut pcm = Vec::with_capacity(n * 2);
        for sample in self.pending.drain(..n) {
            pcm.extend_from_slice(&pcm16(sample).to_le_bytes());
        }
        Ok(pcm)
    }
}
