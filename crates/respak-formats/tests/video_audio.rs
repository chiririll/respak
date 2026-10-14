//! `wav` and `png.frames`, written from stub `Audio` and `Video` values.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{self, BoxStream, StreamExt};
use opendal::{services::Memory, Operator};
use respak_core::capability::{
    Audio, AudioChunk, AudioTrackInfo, FrameFormat, Video, VideoFrame, VideoInfo,
};
use respak_core::value::BundleData;
use respak_core::{
    CancellationToken, Cx, Data, Error, ErrorKind, Event, FormatId, Mediator, Plugin, Registry, Target, Value, ValueKey,
};
use respak_formats::FormatsPlugin;

/// A video and audio source with scripted content that records how it was used.
struct Stub {
    /// One flat colour per frame, 2x2 pixels.
    colours: Vec<[u8; 3]>,
    /// Fail with `Corrupt` after the last frame (a truncated video).
    truncated: bool,
    /// Pixel bytes of each frame are one byte short.
    short_frames: bool,
    /// `tracks[t]` = (info, chunks).
    tracks: Vec<(AudioTrackInfo, Vec<Vec<f32>>)>,
    /// Triggered when the stream yields its item number `.0`.
    cancel_at: Option<(usize, CancellationToken)>,
    polled: Arc<AtomicUsize>,
    asked: Mutex<Vec<FrameFormat>>,
}

impl Stub {
    fn new() -> Self {
        Self {
            colours: Vec::new(),
            truncated: false,
            short_frames: false,
            tracks: Vec::new(),
            cancel_at: None,
            polled: Arc::new(AtomicUsize::new(0)),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn video(colours: &[[u8; 3]]) -> Self {
        Self { colours: colours.to_vec(), ..Self::new() }
    }

    fn audio(tracks: Vec<(AudioTrackInfo, Vec<Vec<f32>>)>) -> Self {
        Self { tracks, ..Self::new() }
    }

    /// Counts each item the stream yields and triggers the cancel token at the scripted one.
    fn counted<T: Send + 'static>(&self, items: Vec<T>) -> BoxStream<'static, T> {
        let (polled, cancel_at) = (self.polled.clone(), self.cancel_at.clone());
        stream::iter(items)
            .inspect(move |_| {
                let n = polled.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some((at, token)) = &cancel_at {
                    if n == *at {
                        token.cancel();
                    }
                }
            })
            .boxed()
    }
}

#[async_trait]
impl Video for Stub {
    fn info(&self) -> VideoInfo {
        VideoInfo { width: 2, height: 2, fps: (30, 1), frame_count: self.colours.len() as u32, full_range: true }
    }

    async fn frames(&self, format: FrameFormat, _cx: &Cx) -> respak_core::Result<BoxStream<'static, respak_core::Result<VideoFrame>>> {
        self.asked.lock().unwrap().push(format);
        let short = self.short_frames;
        let mut items: Vec<respak_core::Result<VideoFrame>> = self
            .colours
            .iter()
            .enumerate()
            .map(|(index, c)| {
                let mut rgba: Vec<u8> = (0..4).flat_map(|_| [c[0], c[1], c[2], 255]).collect();
                if short {
                    rgba.pop();
                }
                Ok(VideoFrame { index: index as u32, format, planes: vec![Bytes::from(rgba)], strides: vec![8] })
            })
            .collect();
        if self.truncated {
            items.push(Err(Error::new(ErrorKind::Corrupt { context: "truncated".into() })));
        }
        Ok(self.counted(items))
    }
}

#[async_trait]
impl Audio for Stub {
    fn tracks(&self) -> Vec<AudioTrackInfo> {
        self.tracks.iter().map(|(info, _)| *info).collect()
    }

    async fn samples(&self, track: u32, _cx: &Cx) -> respak_core::Result<BoxStream<'static, respak_core::Result<AudioChunk>>> {
        let chunks = self.tracks[track as usize].1.clone();
        Ok(self.counted(chunks.into_iter().map(|samples| Ok(AudioChunk { samples })).collect()))
    }
}

/// Holds an `Arc<Stub>` so a test keeps its handle on the stub.
struct Holder(Arc<Stub>, FormatId, ValueKey);

impl std::fmt::Debug for Holder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Holder")
    }
}

impl Data for Holder {
    fn format(&self) -> &FormatId {
        &self.1
    }
    fn key(&self) -> &ValueKey {
        &self.2
    }
    fn as_video(&self) -> Option<&dyn Video> {
        Some(&*self.0)
    }
    fn as_audio(&self) -> Option<&dyn Audio> {
        Some(&*self.0)
    }
}

struct StubPlugin;

impl Plugin for StubPlugin {
    fn id(&self) -> &str {
        "stub"
    }
    fn register(&self, r: &mut Registry) {
        r.format("stub").provides::<dyn Video>().provides::<dyn Audio>();
    }
}

fn cx() -> Cx {
    let mut r = Registry::new();
    r.add(&FormatsPlugin);
    r.add(&StubPlugin);
    r.check().unwrap();
    let mem = || Operator::new(Memory::default()).unwrap();
    Cx::new(Mediator::new(r), mem(), mem())
}

fn value(stub: Stub) -> (Arc<Stub>, Value) {
    let stub = Arc::new(stub);
    let value: Value = Arc::new(Holder(stub.clone(), FormatId::new("stub"), ValueKey::file("clip")));
    (stub, value)
}

fn target(format: &str, options: serde_json::Value) -> Target {
    Target { format: FormatId::new(format.to_owned()), options, via: Vec::new() }
}

fn stereo() -> AudioTrackInfo {
    AudioTrackInfo { channels: 2, sample_rate: 48_000 }
}

fn mono() -> AudioTrackInfo {
    AudioTrackInfo { channels: 1, sample_rate: 22_050 }
}

// ---------------------------------------------------------------- wav

/// (channels, sample rate, samples) of a 16-bit PCM wav, checking every header field.
fn parse_wav(bytes: &[u8]) -> (u16, u32, Vec<i16>) {
    let u16at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32at = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32at(4) as usize, bytes.len() - 8, "RIFF size");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(u32at(16), 16, "fmt chunk size");
    assert_eq!(u16at(20), 1, "PCM");
    let (channels, rate) = (u16at(22), u32at(24));
    assert_eq!(u32at(28), rate * u32::from(channels) * 2, "byte rate");
    assert_eq!(u16at(32), channels * 2, "block align");
    assert_eq!(u16at(34), 16, "bits per sample");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u32at(40) as usize, bytes.len() - 44, "data size");
    let samples = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    (channels, rate, samples)
}

async fn wav_of(stub: Stub, options: serde_json::Value) -> respak_core::Result<Value> {
    let cx = cx();
    let (_, value) = value(stub);
    cx.mediator.convert(value, target("wav", options), &cx).await
}

fn bytes_of(value: &Value) -> Bytes {
    value.as_blob().expect("a blob").bytes().clone()
}

#[tokio::test]
async fn a_wav_has_the_right_header_and_samples() {
    let out = wav_of(Stub::audio(vec![(stereo(), vec![vec![0.0, 0.5, -0.5, 1.0]])]), serde_json::json!({})).await.unwrap();
    assert_eq!(out.format().name(), "wav");
    let (channels, rate, samples) = parse_wav(&bytes_of(&out));
    assert_eq!((channels, rate), (2, 48_000));
    assert_eq!(samples, [0, 16_384, -16_384, 32_767]);
}

#[tokio::test]
async fn samples_round_to_nearest_and_clamp() {
    let input = vec![0.6 / 32768.0, -0.6 / 32768.0, 1.5, -2.0, f32::NAN, 0.99999, -1.0];
    let out = wav_of(Stub::audio(vec![(mono(), vec![input])]), serde_json::json!({})).await.unwrap();
    let (_, _, samples) = parse_wav(&bytes_of(&out));
    // Truncating would give 0 and 0 for the first two.
    assert_eq!(samples, [1, -1, 32_767, -32_768, 0, 32_767, -32_768]);
}

#[tokio::test]
async fn chunk_boundaries_do_not_change_the_output() {
    let all: Vec<f32> = (0..60).map(|i| (i as f32 / 30.0) - 1.0).collect();
    let one = wav_of(Stub::audio(vec![(stereo(), vec![all.clone()])]), serde_json::json!({})).await.unwrap();
    let parts = vec![all[..10].to_vec(), all[10..14].to_vec(), all[14..].to_vec()];
    let many = wav_of(Stub::audio(vec![(stereo(), parts)]), serde_json::json!({})).await.unwrap();
    assert_eq!(bytes_of(&one), bytes_of(&many));
}

#[tokio::test]
async fn the_track_option_picks_the_track() {
    let tracks = vec![(stereo(), vec![vec![0.5, 0.5]]), (mono(), vec![vec![-0.5, -0.25]])];
    let out = wav_of(Stub::audio(tracks), serde_json::json!({ "track": 1 })).await.unwrap();
    let (channels, rate, samples) = parse_wav(&bytes_of(&out));
    assert_eq!((channels, rate), (1, 22_050));
    assert_eq!(samples, [-16_384, -8_192]);
}

#[tokio::test]
async fn a_missing_track_or_no_audio_says_so() {
    let err = wav_of(Stub::audio(vec![(stereo(), vec![vec![0.0, 0.0]])]), serde_json::json!({ "track": 1 })).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::InvalidOptions { message } if message.contains("track 1") && message.contains('1')), "{err}");

    let err = wav_of(Stub::new(), serde_json::json!({})).await.unwrap_err();
    assert!(err.to_string().contains("no audio"), "{err}");

    let err = wav_of(Stub::audio(vec![(stereo(), vec![vec![0.0]])]), serde_json::json!({ "volume": 2 })).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::InvalidOptions { .. }), "{err}");
}

#[tokio::test]
async fn a_chunk_that_is_not_whole_frames_is_corrupt() {
    // Three samples for two channels: the last frame is incomplete.
    let err = wav_of(Stub::audio(vec![(stereo(), vec![vec![0.0, 0.0, 0.0]])]), serde_json::json!({})).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_triggered_token_stops_the_wav_between_chunks() {
    let cx = cx();
    let mut stub = Stub::audio(vec![(mono(), vec![vec![0.1; 4]; 5])]);
    stub.cancel_at = Some((2, cx.cancel.clone()));
    let (stub, value) = value(stub);
    let err = cx.mediator.convert(value, target("wav", serde_json::json!({})), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    assert_eq!(stub.polled.load(Ordering::SeqCst), 2, "no chunk is pulled after the cancel");
}

// ---------------------------------------------------------------- png.frames

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn frames_target() -> Target {
    target("png.frames", serde_json::json!({}))
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];

#[tokio::test]
async fn every_frame_becomes_a_numbered_png_in_one_bundle() {
    let cx = cx();
    let (stub, value) = value(Stub::video(&[RED, GREEN, BLUE]));
    let out = cx.mediator.convert(value, frames_target(), &cx).await.unwrap();

    let bundle = out.downcast_ref::<BundleData>().expect("a bundle");
    assert_eq!(bundle.names, ["frame_00001.png", "frame_00002.png", "frame_00003.png"]);
    for (item, colour) in bundle.items.iter().zip([RED, GREEN, BLUE]) {
        assert_eq!(item.format().name(), "png");
        let (w, h, pixels) = decode_png(&bytes_of(item));
        assert_eq!((w, h), (2, 2));
        let expected: Vec<u8> = (0..4).flat_map(|_| [colour[0], colour[1], colour[2], 255]).collect();
        assert_eq!(pixels, expected);
    }
    assert_eq!(*stub.asked.lock().unwrap(), [FrameFormat::Rgba8], "frames are asked for as RGBA8, once");
}

#[tokio::test]
async fn from_step_and_count_select_frames_but_keep_their_numbers() {
    let cx = cx();
    let (_, value_) = value(Stub::video(&[RED, GREEN, BLUE, RED, GREEN, BLUE]));
    let options = serde_json::json!({ "from": 2, "step": 2, "count": 2 });
    let out = cx.mediator.convert(value_, target("png.frames", options), &cx).await.unwrap();
    let bundle = out.downcast_ref::<BundleData>().expect("a bundle");
    assert_eq!(bundle.names, ["frame_00002.png", "frame_00004.png"]);
    for (item, colour) in bundle.items.iter().zip([GREEN, RED]) {
        let (_, _, pixels) = decode_png(&bytes_of(item));
        assert_eq!(&pixels[..3], &colour);
    }
}

#[tokio::test]
async fn the_video_is_not_read_past_the_last_selected_frame() {
    let cx = cx();
    let (stub, value_) = value(Stub::video(&[RED, GREEN, BLUE, RED, GREEN]));
    let options = serde_json::json!({ "from": 2, "count": 1 });
    cx.mediator.convert(value_, target("png.frames", options), &cx).await.unwrap();
    assert_eq!(stub.polled.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn bad_frame_selections_are_invalid_options() {
    let cx = cx();
    for options in [
        serde_json::json!({ "from": 0 }),
        serde_json::json!({ "step": 0 }),
        serde_json::json!({ "count": 0 }),
        serde_json::json!({ "from": 9 }),
    ] {
        let (_, value_) = value(Stub::video(&[RED, GREEN]));
        let err = cx.mediator.convert(value_, target("png.frames", options.clone()), &cx).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::InvalidOptions { .. }), "{options}: {err}");
    }
}

#[tokio::test]
async fn frames_report_progress_before_each_frame() {
    let cx = cx();
    let mut rx = cx.mediator.subscribe();
    let (_, value) = value(Stub::video(&[RED, GREEN, BLUE]));
    cx.mediator.convert(value, frames_target(), &cx).await.unwrap();
    let mut progress = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if let Event::Progress { task, done, total } = e {
            assert_eq!(task, "png frames");
            progress.push((done, total));
        }
    }
    assert_eq!(progress, [(0, 3), (1, 3), (2, 3)]);
}

#[tokio::test]
async fn a_triggered_token_stops_the_frames_and_returns_no_bundle() {
    let cx = cx();
    let mut stub = Stub::video(&[RED, GREEN, BLUE, RED, GREEN]);
    stub.cancel_at = Some((2, cx.cancel.clone()));
    let (stub, value) = value(stub);
    let err = cx.mediator.convert(value, frames_target(), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    assert_eq!(stub.polled.load(Ordering::SeqCst), 2, "no frame is pulled after the cancel");
}

#[tokio::test]
async fn a_truncated_video_fails_instead_of_writing_a_partial_folder() {
    let cx = cx();
    let mut stub = Stub::video(&[RED, GREEN]);
    stub.truncated = true;
    let (_, value) = value(stub);
    let err = cx.mediator.convert(value, frames_target(), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_frame_of_the_wrong_size_is_corrupt() {
    let cx = cx();
    let mut stub = Stub::video(&[RED]);
    stub.short_frames = true;
    let (_, value) = value(stub);
    let err = cx.mediator.convert(value, frames_target(), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_video_without_frames_is_corrupt_and_unknown_options_are_rejected() {
    let cx = cx();
    let (_, value_) = value(Stub::video(&[]));
    let err = cx.mediator.convert(value_, frames_target(), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");

    let (_, value_) = value(Stub::video(&[RED]));
    let err = cx.mediator.convert(value_, target("png.frames", serde_json::json!({ "fps": 5 })), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::InvalidOptions { .. }), "{err}");
}
