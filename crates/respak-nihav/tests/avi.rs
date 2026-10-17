//! `avi`, `mov` and `y4m`: video and PCM audio, written lazily from a stub `Video` + `Audio`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{self, BoxStream, StreamExt};
use opendal::{services::Memory, Operator};
use respak_core::capability::{
    read_all, Audio, AudioChunk, AudioTrackInfo, FrameFormat, Video, VideoFrame, VideoInfo,
};
use respak_core::{
    Cx, Data, ErrorKind, FormatId, Plugin, Registry, Target, Value, ValueKey,
};
use respak_nihav::NihavPlugin;

const FRAMES: u32 = 8;
const FPS: u32 = 25;
const RATE: u32 = 8_000;

/// 37 x 21: not a multiple of 16, so the encoder has to pad.
struct Clip {
    asked: AtomicUsize,
    full_range: bool,
}

#[async_trait]
impl Video for Clip {
    fn info(&self) -> VideoInfo {
        VideoInfo {
            width: 37,
            height: 21,
            fps: (FPS, 1),
            frame_count: FRAMES,
            full_range: self.full_range,
        }
    }

    async fn frames(
        &self,
        format: FrameFormat,
        _cx: &Cx,
    ) -> respak_core::Result<BoxStream<'static, respak_core::Result<VideoFrame>>> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let frames = (0..FRAMES).map(move |index| {
            if format == FrameFormat::Rgba8 {
                let px: Vec<u8> = (0..37 * 21 * 4)
                    .map(|i| (i as u32 * 7 + index * 11) as u8)
                    .collect();
                return Ok(VideoFrame {
                    index,
                    format,
                    planes: vec![Bytes::from(px)],
                    strides: vec![37 * 4],
                });
            }
            let y: Vec<u8> = (0..37 * 21)
                .map(|i| (i as u32 * 3 + index * 20) as u8)
                .collect();
            let c = |v: u8| Bytes::from(vec![v; 19 * 11]);
            Ok(VideoFrame {
                index,
                format,
                planes: vec![Bytes::from(y), c(100), c(160)],
                strides: vec![37, 19, 19],
            })
        });
        Ok(stream::iter(frames).boxed())
    }
}

#[async_trait]
impl Audio for Clip {
    fn tracks(&self) -> Vec<AudioTrackInfo> {
        vec![AudioTrackInfo {
            channels: 1,
            sample_rate: RATE,
        }]
    }

    async fn samples(
        &self,
        _track: u32,
        _cx: &Cx,
    ) -> respak_core::Result<BoxStream<'static, respak_core::Result<AudioChunk>>> {
        // Exactly the length of the video, in chunks that do not line up with the frames.
        let total = (RATE * FRAMES / FPS) as usize;
        let samples: Vec<f32> = (0..total)
            .map(|i| ((i % 100) as f32 / 100.0) - 0.5)
            .collect();
        let chunks: Vec<_> = samples
            .chunks(333)
            .map(|c| {
                Ok(AudioChunk {
                    samples: c.to_vec(),
                })
            })
            .collect();
        Ok(stream::iter(chunks).boxed())
    }
}

struct Holder(Arc<Clip>, FormatId, ValueKey);

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
        r.format("stub")
            .provides::<dyn Video>()
            .provides::<dyn Audio>();
    }
}

fn cx() -> Cx {
    let mut r = Registry::new();
    r.add(&NihavPlugin);
    r.add(&StubPlugin);
    r.check().unwrap();
    let mem = || Operator::new(Memory::default()).unwrap();
    respak_core::testing::cx(r, mem(), mem())
}

fn clip(full_range: bool) -> (Arc<Clip>, Value) {
    let clip = Arc::new(Clip {
        asked: AtomicUsize::new(0),
        full_range,
    });
    let value: Value = Arc::new(Holder(
        clip.clone(),
        FormatId::new("stub"),
        ValueKey::file("clip"),
    ));
    (clip, value)
}

fn avi(options: serde_json::Value) -> Target {
    Target {
        format: FormatId::new("avi"),
        options,
        via: Vec::new(),
    }
}

/// The chunks inside the `movi` list of a single-RIFF AVI, as (id, payload).
fn movi_chunks(file: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&file[0..4], b"RIFF");
    assert_eq!(&file[8..12], b"AVI ");
    let at = file
        .windows(8)
        .position(|w| &w[4..8] == b"movi")
        .expect("a movi list")
        - 4;
    let end = at + 8 + u32::from_le_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
    let (mut pos, mut out) = (at + 12, Vec::new());
    while pos + 8 <= end.min(file.len()) {
        let id: [u8; 4] = file[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(file[pos + 4..pos + 8].try_into().unwrap()) as usize;
        out.push((id, &file[pos + 8..pos + 8 + size]));
        pos += 8 + size + (size & 1);
    }
    out
}

#[tokio::test]
async fn nothing_is_decoded_until_the_file_is_read() {
    let cx = cx();
    let (clip, source) = clip(true);
    let out = cx
        .mediator
        .convert(source, avi(serde_json::json!({})), &cx)
        .await
        .unwrap();
    assert_eq!(out.format().name(), "avi");
    assert_eq!(
        clip.asked.load(Ordering::SeqCst),
        0,
        "converting is only planning"
    );
    let blob = out.as_blob().unwrap();
    blob.read(0..12, &cx).await.unwrap();
    assert_eq!(clip.asked.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_file_has_every_frame_as_a_padded_jpeg_and_all_the_audio() {
    let cx = cx();
    let (_, source) = clip(true);
    let out = cx
        .mediator
        .convert(source, avi(serde_json::json!({ "quality": 80 })), &cx)
        .await
        .unwrap();
    let file = read_all(out.as_blob().unwrap(), &cx).await.unwrap();
    assert!(file.windows(4).any(|w| w == b"MJPG"));

    let chunks = movi_chunks(&file);
    let video: Vec<_> = chunks.iter().filter(|(id, _)| id == b"00dc").collect();
    assert_eq!(video.len(), FRAMES as usize);
    for (_, jpeg) in &video {
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "starts with SOI");
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xFF, 0xD9], "ends with EOI");
        // The SOF0 marker holds height then width: the padded size, 21 -> 32 and 37 -> 48.
        let sof = jpeg
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("SOF0");
        let height = u16::from_be_bytes([jpeg[sof + 5], jpeg[sof + 6]]);
        let width = u16::from_be_bytes([jpeg[sof + 7], jpeg[sof + 8]]);
        assert_eq!((width, height), (48, 32));
    }
    let audio_bytes: usize = chunks
        .iter()
        .filter(|(id, _)| id == b"01wb")
        .map(|(_, p)| p.len())
        .sum();
    assert_eq!(
        audio_bytes,
        (RATE * FRAMES / FPS) as usize * 2,
        "16-bit mono, the whole track"
    );
    // The audio is interleaved with the video, not appended after it.
    let first_audio = chunks.iter().position(|(id, _)| id == b"01wb").unwrap();
    let last_video = chunks.iter().rposition(|(id, _)| id == b"00dc").unwrap();
    assert!(first_audio < last_video, "audio follows each frame");
}

#[tokio::test]
async fn a_limited_range_source_is_expanded() {
    let cx = cx();
    let (_, full) = clip(true);
    let (_, limited) = clip(false);
    let a = cx
        .mediator
        .convert(full, avi(serde_json::json!({})), &cx)
        .await
        .unwrap();
    let b = cx
        .mediator
        .convert(limited, avi(serde_json::json!({})), &cx)
        .await
        .unwrap();
    let a = read_all(a.as_blob().unwrap(), &cx).await.unwrap();
    let b = read_all(b.as_blob().unwrap(), &cx).await.unwrap();
    assert_ne!(a, b, "the samples were expanded");
}

#[tokio::test]
async fn mute_leaves_out_the_sound_and_a_track_that_does_not_exist_is_refused() {
    let cx = cx();
    let (_, source) = clip(true);
    let out = cx
        .mediator
        .convert(
            source.clone(),
            avi(serde_json::json!({ "mute": true })),
            &cx,
        )
        .await
        .unwrap();
    let file = read_all(out.as_blob().unwrap(), &cx).await.unwrap();
    assert!(movi_chunks(&file).iter().all(|(id, _)| id == b"00dc"));

    let err = cx
        .mediator
        .convert(source, avi(serde_json::json!({ "audio_track": 3 })), &cx)
        .await
        .unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::InvalidOptions { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn bad_options_fail_at_planning() {
    let cx = cx();
    let (clip, source) = clip(true);
    for options in [
        serde_json::json!({ "codec": "h264" }),
        serde_json::json!({ "quality": 0 }),
        serde_json::json!({ "quality": 101 }),
        serde_json::json!({ "bitrate": 5 }),
    ] {
        let err = cx
            .mediator
            .convert(source.clone(), avi(options.clone()), &cx)
            .await
            .unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::InvalidOptions { .. }),
            "{options}: {err}"
        );
    }
    assert_eq!(clip.asked.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_file_is_streamed_to_the_output() {
    let cx = cx();
    let (_, source) = clip(true);
    let out = cx
        .mediator
        .convert(source, avi(serde_json::json!({})), &cx)
        .await
        .unwrap();
    assert!(cx
        .write_blob("clip.avi", out.key(), out.as_blob().unwrap())
        .await
        .unwrap());
    let written = cx.output.read("clip.avi").await.unwrap().to_bytes();
    assert_eq!(&written[..4], b"RIFF");
    assert_eq!(
        movi_chunks(&written)
            .iter()
            .filter(|(id, _)| id == b"00dc")
            .count(),
        FRAMES as usize
    );
}

async fn file(options: serde_json::Value, container: &'static str) -> respak_core::Result<Bytes> {
    let cx = cx();
    let (_, source) = clip(true);
    let target = Target {
        format: FormatId::new(container),
        options,
        via: Vec::new(),
    };
    let out = cx.mediator.convert(source, target, &cx).await?;
    read_all(out.as_blob().unwrap(), &cx).await
}

#[tokio::test]
async fn avi_holds_cinepak_and_zmbv() {
    for (codec, fourcc) in [("cinepak", b"cvid"), ("zmbv", b"ZMBV")] {
        let file = file(serde_json::json!({ "codec": codec }), "avi")
            .await
            .unwrap();
        assert!(file.windows(4).any(|w| w == fourcc), "{codec}");
        let chunks = movi_chunks(&file);
        assert_eq!(
            chunks.iter().filter(|(id, _)| id == b"00dc").count(),
            FRAMES as usize,
            "{codec}"
        );
        let audio: usize = chunks
            .iter()
            .filter(|(id, _)| id == b"01wb")
            .map(|(_, p)| p.len())
            .sum();
        assert_eq!(
            audio,
            (RATE * FRAMES / FPS) as usize * 2,
            "{codec} keeps the sound"
        );
    }
}

#[tokio::test]
async fn mov_has_video_and_sound_in_both_codecs() {
    for codec in ["mjpeg", "cinepak"] {
        let file = file(serde_json::json!({ "codec": codec }), "mov")
            .await
            .unwrap();
        // The audio sample table counts 2-byte samples, all of them (NihAV leaves it empty).
        let tables: Vec<(u32, u32)> = file
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"stsz")
            .map(|(at, _)| {
                let field = |n: usize| {
                    u32::from_be_bytes(file[at + 8 + n * 4..at + 12 + n * 4].try_into().unwrap())
                };
                (field(0), field(1))
            })
            .collect();
        assert_eq!(tables.len(), 2, "{codec}");
        assert_eq!(
            tables[1],
            (2, RATE * FRAMES / FPS),
            "{codec}: audio sample size and count"
        );
        for atom in [&b"moov"[..], b"mdat", b"vide", b"soun", b"sowt"] {
            assert!(
                file.windows(4).any(|w| w == atom),
                "{codec}: no {}",
                String::from_utf8_lossy(atom)
            );
        }
    }
    let silent = file(serde_json::json!({ "mute": true }), "mov")
        .await
        .unwrap();
    assert!(!silent.windows(4).any(|w| w == b"soun"));
}

#[tokio::test]
async fn y4m_is_raw_limited_range_planes_without_sound() {
    let file = file(serde_json::json!({}), "y4m").await.unwrap();
    let header = b"YUV4MPEG2 W37 H21 F25:1 C420";
    assert!(
        file.starts_with(header),
        "{}",
        String::from_utf8_lossy(&file[..40])
    );
    let frame = 37 * 21 + 2 * 19 * 11;
    let body = &file[file.iter().position(|b| *b == b'\n').unwrap() + 1..];
    assert_eq!(
        body.len(),
        FRAMES as usize * (6 + frame),
        "FRAME\n and the planes, per frame"
    );
    assert_eq!(&body[..6], b"FRAME\n");
    // The source is full range, so the samples are compressed to 16..=235.
    assert!(body[6..6 + 37 * 21].iter().all(|y| (16..=235).contains(y)));
}

#[tokio::test]
async fn a_codec_a_container_cannot_hold_is_refused_and_y4m_has_no_track_to_pick() {
    for (container, options) in [
        ("y4m", serde_json::json!({ "codec": "mjpeg" })),
        ("mov", serde_json::json!({ "codec": "zmbv" })),
        ("avi", serde_json::json!({ "codec": "rawvideo" })),
        ("y4m", serde_json::json!({ "audio_track": 0 })),
    ] {
        let err = file(options.clone(), container).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::InvalidOptions { .. }),
            "{container} {options}: {err}"
        );
    }
}
