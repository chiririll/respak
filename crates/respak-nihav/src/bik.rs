//! `bik`: RAD Bink video, Bink 2 only (`KB2` + a revision letter; `.bik` or `.bk2`).
//!
//! Not ported from rsx (rsx does not read Bink). The container and codecs come from NihAV
//! (`nihav_rad`, our fork `github.com/chiririll/nihav`); see [`crate::bik_decode`].
//!
//! The file is read **whole** and once: the demuxer wants the file and frames are decoded front
//! to back anyway (Apex videos are about 10 MB). Frames and audio are decoded on demand through
//! the `Video` and `Audio` capabilities.

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt};
use respak_core::capability::{
    Audio, AudioChunk, AudioTrackInfo, FrameFormat, Video, VideoFrame, VideoInfo,
};
use respak_core::value::FileData;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::bik_decode;

/// Decoded items a stream may run ahead of its consumer (a 1080p RGBA frame is 8 MB).
const STREAM_BUFFER: usize = 4;

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

/// The fixed part of a Bink header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BikHeader {
    /// Bink version: 1 (`BIK`) or 2 (`KB2`).
    pub major: u32,
    /// The letter after the magic (`b'i'` in current Apex).
    pub revision: u8,
    /// File size minus 8.
    pub size: u32,
    /// Number of frames.
    pub frame_count: u32,
    /// Size of the largest frame.
    pub largest_frame: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Header flags.
    pub flags: u32,
    /// Number of audio tracks.
    pub audio_tracks: u32,
}

impl BikHeader {
    /// Bytes of the fixed header, the part [`BikHeader::parse`] needs.
    pub const LEN: usize = 44;

    /// Frames and sizes above these cannot be a real video.
    const MAX_DIMENSION: u32 = 1 << 16;

    /// Parses a Bink header (`KB2x` or `BIKx`, `x` a lower-case letter); `None` if it is not one
    /// or its fields are not plausible (no frames, a zero or huge size, a zero frame rate).
    pub fn parse(header: &[u8]) -> Option<Self> {
        let header = header.get(..Self::LEN)?;
        let major = match &header[..3] {
            b"KB2" => 2,
            b"BIK" => 1,
            _ => return None,
        };
        let revision = header[3];
        if !revision.is_ascii_lowercase() {
            return None;
        }
        let field = |at: usize| {
            u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
        };
        let parsed = Self {
            major,
            revision,
            size: field(4),
            frame_count: field(8),
            largest_frame: field(12),
            width: field(20),
            height: field(24),
            fps_num: field(28),
            fps_den: field(32),
            flags: field(36),
            audio_tracks: field(40),
        };
        let size_ok = |n: u32| (1..=Self::MAX_DIMENSION).contains(&n);
        let plausible = parsed.frame_count > 0
            && size_ok(parsed.width)
            && size_ok(parsed.height)
            && parsed.fps_num > 0
            && parsed.fps_den > 0;
        plausible.then_some(parsed)
    }

    /// The version as detection reports it: the Bink version, and the revision letter as the
    /// layout variant (`a` = 0).
    pub fn version(&self) -> Version {
        Version::with_minor(self.major, u32::from(self.revision - b'a'))
    }
}

/// Read edge `file -> bik`.
pub struct BikRead;

#[async_trait]
impl Detect for BikRead {
    /// Magic and a revision letter = 10, plausible fields = +2, extension `bik` or `bk2` = +1.
    /// Bink 1 (`BIK` + a letter) is scored like Bink 2 so that reading it can say
    /// `UnsupportedVersion` instead of `UnknownFormat`. `Version { major: 2, minor: letter - 'a' }`
    /// (`major: 1` for Bink 1).
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(header) = BikHeader::parse(input.header) else {
            return Ok(None);
        };
        let ext = u32::from(matches!(input.extension, Some("bik" | "bk2")));
        Ok(Some(Detected {
            score: 12 + ext,
            version: Some(header.version()),
        }))
    }

    fn header_len(&self) -> usize {
        BikHeader::LEN
    }
}

#[async_trait]
impl ReadHandler for BikRead {
    /// Reads the whole file (see the module docs) and opens its container once, so a broken index
    /// fails here. Bink 1 gives `UnsupportedVersion` before anything is read.
    async fn read(&self, source: Value, detected: &Detected, cx: &Cx) -> Result<Value> {
        if let Some(v) = detected.version.filter(|v| v.major == 1) {
            let letter = char::from(b'a' + v.minor as u8);
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "bik".into(),
                version: format!("Bink 1 (BIK{letter})"),
            }));
        }
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "bik must be read from a file".into(),
            })
        })?;
        let reader = file.op.read_with(&file.path);
        let bytes = match &file.range {
            Some(r) => reader.range(r.clone()).await?,
            None => reader.await?,
        }
        .to_bytes();
        let header = BikHeader::parse(&bytes)
            .ok_or_else(|| corrupt("bik: the header is not a Bink 2 header"))?;
        // The demuxer sizes its frame index from the header without checking the file, so a
        // damaged frame count would ask for gigabytes (and abort, which cannot be caught).
        // The index (4 bytes per frame, plus one) is part of the file.
        let index_len = (u64::from(header.frame_count) + 1) * 4 + BikHeader::LEN as u64;
        if index_len > bytes.len() as u64 {
            return Err(corrupt(format!(
                "bik: {} frames need an index of {index_len} bytes, the file has {}",
                header.frame_count,
                bytes.len()
            )));
        }

        let probed = {
            let bytes = bytes.clone();
            cx.blocking(move || bik_decode::probe(&bytes)).await??
        };
        let video = VideoInfo {
            width: probed.size.0,
            height: probed.size.1,
            fps: (header.fps_num, header.fps_den),
            frame_count: header.frame_count,
            // Bink 2 decodes to full-range YUV (NihAV's `YUVJ`).
            full_range: true,
        };
        Ok(std::sync::Arc::new(BikFile {
            bytes,
            header,
            video,
            tracks: probed.audio,
            format: FormatId::new("bik").with_version(header.version()),
            key: source.key().clone(),
        }))
    }
}

/// A Bink 2 file in memory: `Data` + `Video` + `Audio`.
#[derive(Debug)]
pub struct BikFile {
    bytes: Bytes,
    header: BikHeader,
    video: VideoInfo,
    tracks: Vec<AudioTrackInfo>,
    format: FormatId,
    key: ValueKey,
}

impl BikFile {
    /// The parsed header.
    pub fn header(&self) -> &BikHeader {
        &self.header
    }
}

impl Data for BikFile {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn approx_size(&self) -> usize {
        self.bytes.len()
    }
    fn as_video(&self) -> Option<&dyn Video> {
        Some(self)
    }
    fn as_audio(&self) -> Option<&dyn Audio> {
        Some(self)
    }
}

/// Runs `decode` on the blocking pool and streams what it hands to its sink. The decoder stops
/// when the stream is dropped or the request is cancelled (the last item is then `Cancelled`).
fn spawn_stream<T: Send + 'static>(
    cx: &Cx,
    decode: impl FnOnce(&mut dyn FnMut(Result<T>) -> bool) + Send + 'static,
) -> BoxStream<'static, Result<T>> {
    let (tx, rx) = tokio::sync::mpsc::channel(STREAM_BUFFER);
    let cx = cx.clone();
    tokio::spawn(async move {
        let sender = tx.clone();
        let cancel = cx.cancel.clone();
        let ran = cx
            .blocking(move || {
                decode(&mut |item| {
                    if cancel.is_cancelled() {
                        let _ = sender.blocking_send(Err(Error::new(ErrorKind::Cancelled)));
                        return false;
                    }
                    sender.blocking_send(item).is_ok()
                })
            })
            .await;
        if let Err(e) = ran {
            let _ = tx.send(Err(e)).await;
        }
    });
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
    .boxed()
}

#[async_trait]
impl Video for BikFile {
    fn info(&self) -> VideoInfo {
        self.video
    }

    /// Decodes with NihAV on the blocking pool; `Err(Cancelled)` ends the stream if the token
    /// fires; a truncated file yields its frames, then `Corrupt`.
    async fn frames(
        &self,
        format: FrameFormat,
        cx: &Cx,
    ) -> Result<BoxStream<'static, Result<VideoFrame>>> {
        let bytes = self.bytes.clone();
        Ok(spawn_stream(cx, move |sink| {
            bik_decode::decode_video(&bytes, format, sink)
        }))
    }
}

#[async_trait]
impl Audio for BikFile {
    fn tracks(&self) -> Vec<AudioTrackInfo> {
        self.tracks.clone()
    }

    /// Decodes the track with NihAV's Bink audio decoder, on the blocking pool.
    async fn samples(&self, track: u32, cx: &Cx) -> Result<BoxStream<'static, Result<AudioChunk>>> {
        if track as usize >= self.tracks.len() {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!("no audio track {track}: the file has {}", self.tracks.len()),
            }));
        }
        let bytes = self.bytes.clone();
        Ok(spawn_stream(cx, move |sink| {
            bik_decode::decode_audio(&bytes, track, sink)
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use opendal::{services::Memory, Operator};
    use respak_core::{Mediator, Registry};

    use super::*;

    fn cx() -> Cx {
        let mem = || Operator::new(Memory::default()).unwrap();
        Cx::new(Mediator::new(Registry::new()), mem(), mem())
    }

    #[tokio::test]
    async fn a_stream_yields_what_the_decoder_hands_over_in_order_then_ends() {
        let cx = cx();
        // More items than the buffer holds: the decoder has to wait for the consumer.
        let items: Vec<u32> = (0..50).collect();
        let sent = items.clone();
        let stream = spawn_stream(&cx, move |sink| {
            for i in sent {
                if !sink(Ok(i)) {
                    return;
                }
            }
        });
        let got: Vec<u32> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(got, items);
    }

    #[tokio::test]
    async fn a_failure_is_the_last_item() {
        let cx = cx();
        let mut stream = spawn_stream(&cx, |sink| {
            sink(Ok(1u32));
            sink(Err(corrupt("cut here")));
        });
        assert_eq!(stream.next().await.unwrap().unwrap(), 1);
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn dropping_the_stream_stops_the_decoder() {
        let cx = cx();
        let produced = Arc::new(AtomicUsize::new(0));
        let counter = produced.clone();
        let mut stream = spawn_stream(&cx, move |sink| {
            while sink(Ok(0u32)) {
                counter.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        stream.next().await.unwrap().unwrap();
        drop(stream);
        // The decoder notices at its next send, so it stops soon, nowhere near "forever".
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let stopped_at = produced.load(Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            produced.load(Ordering::SeqCst),
            stopped_at,
            "still decoding after the drop"
        );
        assert!(stopped_at < 50, "{stopped_at}");
    }

    #[tokio::test]
    async fn a_triggered_token_ends_the_stream_with_cancelled() {
        let cx = cx();
        let cancel = cx.cancel.clone();
        let mut stream = spawn_stream(&cx, |sink| {
            let mut i = 0u32;
            while sink(Ok(i)) {
                i += 1;
            }
        });
        stream.next().await.unwrap().unwrap();
        cancel.cancel();
        let mut last = None;
        while let Some(item) = stream.next().await {
            last = Some(item);
        }
        let err = last.expect("items after the cancel").unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    }

    #[tokio::test]
    async fn a_token_triggered_before_the_start_gives_cancelled_and_no_decoding() {
        let cx = cx();
        cx.cancel.cancel();
        let ran = Arc::new(AtomicUsize::new(0));
        let counter = ran.clone();
        let mut stream = spawn_stream(&cx, move |_sink: &mut dyn FnMut(Result<u32>) -> bool| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_header_fields_are_read_from_their_offsets() {
        let mut h = b"KB2i".to_vec();
        for v in [
            1000u32, 210, 100, 210, 1920, 1080, 10_000_000, 333_333, 0x20, 2,
        ] {
            h.extend(v.to_le_bytes());
        }
        let parsed = BikHeader::parse(&h).unwrap();
        assert_eq!(
            (
                parsed.major,
                parsed.revision,
                parsed.size,
                parsed.frame_count,
                parsed.largest_frame
            ),
            (2, b'i', 1000, 210, 100)
        );
        assert_eq!(
            (parsed.width, parsed.height, parsed.fps_num, parsed.fps_den),
            (1920, 1080, 10_000_000, 333_333)
        );
        assert_eq!((parsed.flags, parsed.audio_tracks), (0x20, 2));
        assert_eq!(parsed.version(), Version::with_minor(2, 8));
    }
}
