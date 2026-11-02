//! Formats written by an external `ffmpeg`: `mp4`, `mkv`, `webm`, `gif` from any `Video` (with the
//! sound of any `Audio` it also is) and `mp3`, `flac`, `ogg` from any `Audio`.
//!
//! Nothing here links a codec library. [`FfmpegPlugin`] looks for `ffmpeg` on `PATH` when it
//! registers and adds the formats only if it finds one, so without ffmpeg they do not exist.
//!
//! The source is decoded by respak and piped to ffmpeg as raw RGBA frames (video) or 16-bit PCM
//! (sound). Video with sound gets its PCM from a temporary file, since a child process has one
//! stdin. The result is a [`LazyBlob`] served from a temporary file, like the NihAV containers.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use respak_core::capability::{AudioChunk, AudioTrackInfo, FrameFormat, VideoFrame, VideoInfo};
use respak_core::value::{LazyBlob, LazySource};
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, Event, FormatId, Plugin, Registry, Result, Target,
    TypedTarget, Value, ValueKey,
};
use tempfile::TempPath;
use tokio::runtime::Handle;

use crate::wav::pcm16;
use crate::{audio_of, video_of, VideoFileOptions, WavOptions};

/// How much of ffmpeg's stderr an error quotes.
const STDERR_TAIL: usize = 2000;

/// A video format ffmpeg writes.
struct VideoSpec {
    name: &'static str,
    label: &'static str,
    /// The codec used when the `codec` option is absent.
    codec: &'static str,
    /// The sound codec; `None` for a format without sound.
    audio_codec: Option<&'static str>,
    /// Arguments for the output only.
    extra: &'static [&'static str],
}

const VIDEO: &[VideoSpec] = &[
    VideoSpec {
        name: "mp4",
        label: "MP4 video",
        codec: "libx264",
        audio_codec: Some("aac"),
        extra: &["-movflags", "+faststart"],
    },
    VideoSpec {
        name: "mkv",
        label: "Matroska video",
        codec: "libx264",
        audio_codec: Some("aac"),
        extra: &[],
    },
    VideoSpec {
        name: "webm",
        label: "WebM video",
        codec: "libvpx-vp9",
        audio_codec: Some("libopus"),
        extra: &["-b:v", "0"],
    },
    VideoSpec {
        name: "gif",
        label: "GIF animation",
        codec: "gif",
        audio_codec: None,
        extra: &[],
    },
];

/// An audio format ffmpeg writes.
struct AudioSpec {
    name: &'static str,
    label: &'static str,
    codec: &'static str,
}

const AUDIO: &[AudioSpec] = &[
    AudioSpec {
        name: "mp3",
        label: "MP3 audio",
        codec: "libmp3lame",
    },
    AudioSpec {
        name: "flac",
        label: "FLAC audio",
        codec: "flac",
    },
    AudioSpec {
        name: "ogg",
        label: "Ogg Vorbis audio",
        codec: "libvorbis",
    },
];

/// Registers the formats ffmpeg writes, if there is an ffmpeg.
pub struct FfmpegPlugin {
    exe: Option<PathBuf>,
}

impl FfmpegPlugin {
    /// Uses the first `ffmpeg` on `PATH`; with none, the plugin registers nothing.
    pub fn new() -> Self {
        Self::at(find_in_path())
    }

    /// Uses the given ffmpeg executable (`None`: none).
    pub fn at(exe: Option<PathBuf>) -> Self {
        Self { exe }
    }

    /// The ffmpeg that will be run, if any.
    pub fn exe(&self) -> Option<&Path> {
        self.exe.as_deref()
    }
}

impl Default for FfmpegPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for FfmpegPlugin {
    fn id(&self) -> &str {
        "ffmpeg"
    }

    fn register(&self, r: &mut Registry) {
        let Some(exe) = &self.exe else {
            return;
        };
        let exe: Arc<Path> = exe.as_path().into();
        for spec in VIDEO {
            r.format(spec.name)
                .label(spec.label)
                .extensions([spec.name])
                .from_cap::<dyn respak_core::capability::Video>(VideoEncode {
                    exe: exe.clone(),
                    spec,
                })
                .options::<VideoFileOptions>();
        }
        for spec in AUDIO {
            r.format(spec.name)
                .label(spec.label)
                .extensions([spec.name])
                .from_cap::<dyn respak_core::capability::Audio>(AudioEncode {
                    exe: exe.clone(),
                    spec,
                })
                .options::<WavOptions>();
        }
    }
}

/// The first `ffmpeg` executable in a `PATH` directory.
fn find_in_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom {
        source: None,
        message: message.into(),
    })
}

fn io(e: std::io::Error, what: &str) -> Error {
    Error::new(ErrorKind::Custom {
        source: Some(Box::new(e)),
        message: what.into(),
    })
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidOptions {
        message: message.into(),
    })
}

/// Capability edge `Video -> <ffmpeg container>`.
struct VideoEncode {
    exe: Arc<Path>,
    spec: &'static VideoSpec,
}

#[async_trait]
impl ConvertHandler for VideoEncode {
    /// Checks the options and the source now and returns a [`LazyBlob`]; ffmpeg runs when the blob
    /// is first read. A bad codec or track is `InvalidOptions`.
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<VideoFileOptions>::parse(target)?;
        let opts = options.options().clone();
        let video = video_of(&source)?;
        let info = video.info();
        if !(1..=100).contains(&opts.quality) {
            return Err(invalid("`quality` is 1 to 100"));
        }
        let codec = opts.codec.clone().unwrap_or_else(|| self.spec.codec.into());
        if codec.is_empty()
            || !codec
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || codec.starts_with('-')
        {
            return Err(invalid(format!("`{codec}` is not a codec name")));
        }
        if opts.audio_track.is_some() && self.spec.audio_codec.is_none() {
            return Err(invalid(format!(
                "`{}` has no sound, so there is no audio track to pick",
                self.spec.name
            )));
        }
        let track = match (opts.mute || self.spec.audio_codec.is_none(), source.as_audio()) {
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

        let format = FormatId::new(self.spec.name);
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            format.unversioned(),
            options.normalized(),
        );
        let (exe, spec) = (self.exe.clone(), self.spec);
        let init = Box::new(move |cx: Cx| {
            Box::pin(async move {
                produce_video(exe, spec, codec, opts, source, info, track, cx).await
            }) as futures::future::BoxFuture<'static, Result<LazySource>>
        });
        Ok(Arc::new(LazyBlob::new(format, key, init)))
    }
}

#[allow(clippy::too_many_arguments)]
async fn produce_video(
    exe: Arc<Path>,
    spec: &'static VideoSpec,
    codec: String,
    opts: VideoFileOptions,
    source: Value,
    info: VideoInfo,
    track: Option<(u32, AudioTrackInfo)>,
    cx: Cx,
) -> Result<LazySource> {
    let frames = video_of(&source)?.frames(FrameFormat::Rgba8, &cx).await?;
    // The sound goes to a temporary file first; ffmpeg reads it as a second input.
    let audio = match (track, source.as_audio()) {
        (Some((index, shape)), Some(audio)) => {
            let stream = audio.samples(index, &cx).await?;
            Some(write_pcm(shape, stream, &cx).await?)
        }
        _ => None,
    };
    let handle = Handle::current();
    let task_cx = cx.clone();
    let file = cx
        .blocking(move || {
            let _keep = source;
            run_video(
                &exe, spec, &codec, &opts, info, audio, frames, handle, &task_cx,
            )
        })
        .await??;
    Ok(LazySource::File(file))
}

/// The track as raw 16-bit PCM in a temporary file, with the shape ffmpeg has to be told.
async fn write_pcm(
    shape: AudioTrackInfo,
    mut stream: BoxStream<'static, Result<AudioChunk>>,
    cx: &Cx,
) -> Result<(TempPath, AudioTrackInfo)> {
    let file = tempfile::Builder::new()
        .suffix(".pcm")
        .tempfile()
        .map_err(|e| io(e, "creating the temporary file"))?;
    let (file, path) = file.into_parts();
    let mut out = BufWriter::new(file);
    while let Some(chunk) = stream.next().await {
        cx.check_cancelled()?;
        let pcm: Vec<u8> = chunk?
            .samples
            .iter()
            .flat_map(|&s| pcm16(s).to_le_bytes())
            .collect();
        out.write_all(&pcm)
            .map_err(|e| io(e, "writing the temporary file"))?;
    }
    out.flush()
        .map_err(|e| io(e, "writing the temporary file"))?;
    Ok((path, shape))
}

/// ffmpeg's `-crf` for a quality of 1..=100: 18 at the best, 51 (x264/x265) or 63 (vp9) at the worst.
fn crf(codec: &str, quality: u8) -> Option<u32> {
    let worst = match codec {
        "libx264" | "libx265" => 51,
        "libvpx-vp9" => 63,
        _ => return None,
    };
    let q = u32::from(quality);
    Some(worst - (q - 1) * (worst - 18) / 99)
}

#[allow(clippy::too_many_arguments)]
fn run_video(
    exe: &Path,
    spec: &VideoSpec,
    codec: &str,
    opts: &VideoFileOptions,
    info: VideoInfo,
    audio: Option<(TempPath, AudioTrackInfo)>,
    mut frames: BoxStream<'static, Result<VideoFrame>>,
    handle: Handle,
    cx: &Cx,
) -> Result<File> {
    let out = tempfile::Builder::new()
        .suffix(&format!(".{}", spec.name))
        .tempfile()
        .map_err(|e| io(e, "creating the temporary file"))?
        .into_temp_path();

    let mut cmd = Command::new(exe);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
        .arg("-s")
        .arg(format!("{}x{}", info.width, info.height))
        .arg("-framerate")
        .arg(format!("{}/{}", info.fps.0, info.fps.1.max(1)))
        .args(["-i", "pipe:0"]);
    if let Some((pcm, shape)) = &audio {
        cmd.args(["-f", "s16le"])
            .arg("-ar")
            .arg(shape.sample_rate.to_string())
            .arg("-ac")
            .arg(shape.channels.to_string())
            .arg("-i")
            .arg(pcm.as_os_str());
    }
    cmd.args(["-map", "0:v"]);
    if audio.is_some() {
        cmd.args(["-map", "1:a"]);
    }
    cmd.args(["-c:v", codec]);
    if spec.name != "gif" {
        // yuv420p wants even sizes, and is what players expect.
        cmd.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p"]);
    }
    if let Some(crf) = crf(codec, opts.quality) {
        cmd.arg("-crf").arg(crf.to_string());
    }
    if let (Some(audio_codec), true) = (spec.audio_codec, audio.is_some()) {
        cmd.args(["-c:a", audio_codec]);
    }
    cmd.args(spec.extra).arg(out.as_os_str());

    let mut run = Run::start(cmd)?;
    let mut stdin = run.stdin()?;
    let mut seen = 0u32;
    let mut write_failed = false;
    while let Some(frame) = handle.block_on(frames.next()) {
        if let Err(e) = cx.check_cancelled() {
            run.kill();
            return Err(e);
        }
        let frame = frame.inspect_err(|_| run.kill())?;
        cx.mediator.publish(Event::Progress {
            task: "video encode".into(),
            done: u64::from(frame.index),
            total: u64::from(info.frame_count),
        });
        if write_frame(&mut stdin, &frame, info).is_err() {
            // ffmpeg stopped reading; its stderr says why.
            write_failed = true;
            break;
        }
        seen += 1;
    }
    drop(stdin);
    if seen == 0 && !write_failed {
        run.kill();
        return Err(Error::corrupt("the video has no frames"));
    }
    run.finish(spec.name)?;
    open_result(out)
}

/// One frame's RGBA rows, without any padding at the end of a row.
fn write_frame(out: &mut impl Write, frame: &VideoFrame, info: VideoInfo) -> std::io::Result<()> {
    let row = info.width as usize * 4;
    let (Some(plane), Some(&stride)) = (frame.planes.first(), frame.strides.first()) else {
        return Err(std::io::ErrorKind::InvalidData.into());
    };
    if stride == row {
        return out.write_all(plane);
    }
    for line in plane.chunks(stride).take(info.height as usize) {
        out.write_all(&line[..row.min(line.len())])?;
    }
    Ok(())
}

/// Capability edge `Audio -> <ffmpeg audio format>`.
struct AudioEncode {
    exe: Arc<Path>,
    spec: &'static AudioSpec,
}

#[async_trait]
impl ConvertHandler for AudioEncode {
    /// Like `wav`: no audio at all is an error that says so, a missing track is `InvalidOptions`.
    /// ffmpeg runs when the blob is first read.
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<WavOptions>::parse(target)?;
        let audio = audio_of(&source)?;
        let tracks = audio.tracks();
        if tracks.is_empty() {
            return Err(custom(format!("`{}` has no audio", source.format())));
        }
        let index = options.options().track;
        let shape = *tracks.get(index as usize).ok_or_else(|| {
            invalid(format!(
                "track {index}: the source has {} audio track(s)",
                tracks.len()
            ))
        })?;
        if shape.channels == 0 || shape.sample_rate == 0 {
            return Err(Error::corrupt(format!(
                "audio track {index} has {} channels at {} Hz",
                shape.channels, shape.sample_rate
            )));
        }
        let format = FormatId::new(self.spec.name);
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            format.unversioned(),
            options.normalized(),
        );
        let (exe, spec) = (self.exe.clone(), self.spec);
        let init = Box::new(move |cx: Cx| {
            Box::pin(async move {
                let stream = audio_of(&source)?.samples(index, &cx).await?;
                let handle = Handle::current();
                let task_cx = cx.clone();
                let file = cx
                    .blocking(move || {
                        let _keep = source;
                        run_audio(&exe, spec, shape, stream, handle, &task_cx)
                    })
                    .await??;
                Ok(LazySource::File(file))
            }) as futures::future::BoxFuture<'static, Result<LazySource>>
        });
        Ok(Arc::new(LazyBlob::new(format, key, init)))
    }
}

fn run_audio(
    exe: &Path,
    spec: &AudioSpec,
    shape: AudioTrackInfo,
    mut stream: BoxStream<'static, Result<AudioChunk>>,
    handle: Handle,
    cx: &Cx,
) -> Result<File> {
    let out = tempfile::Builder::new()
        .suffix(&format!(".{}", spec.name))
        .tempfile()
        .map_err(|e| io(e, "creating the temporary file"))?
        .into_temp_path();

    let mut cmd = Command::new(exe);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .args(["-f", "s16le"])
        .arg("-ar")
        .arg(shape.sample_rate.to_string())
        .arg("-ac")
        .arg(shape.channels.to_string())
        .args(["-i", "pipe:0", "-vn", "-c:a", spec.codec])
        .arg(out.as_os_str());

    let mut run = Run::start(cmd)?;
    let mut stdin = run.stdin()?;
    while let Some(chunk) = handle.block_on(stream.next()) {
        if let Err(e) = cx.check_cancelled() {
            run.kill();
            return Err(e);
        }
        let chunk = chunk.inspect_err(|_| run.kill())?;
        let pcm: Vec<u8> = chunk
            .samples
            .iter()
            .flat_map(|&s| pcm16(s).to_le_bytes())
            .collect();
        if stdin.write_all(&pcm).is_err() {
            break;
        }
    }
    drop(stdin);
    run.finish(spec.name)?;
    open_result(out)
}

/// The finished file, opened; the path is deleted, so the open file is all that is left.
fn open_result(path: TempPath) -> Result<File> {
    File::open(&path).map_err(|e| io(e, "opening the ffmpeg output"))
}

/// A running ffmpeg, with its stderr collected on a thread so it never fills up.
struct Run {
    child: Child,
    stderr: Option<std::thread::JoinHandle<Vec<u8>>>,
}

impl Run {
    fn start(mut cmd: Command) -> Result<Self> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io(e, "starting ffmpeg"))?;
        let mut pipe = child.stderr.take();
        let stderr = std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(pipe) = pipe.as_mut() {
                let _ = pipe.read_to_end(&mut text);
            }
            text
        });
        Ok(Self {
            child,
            stderr: Some(stderr),
        })
    }

    fn stdin(&mut self) -> Result<BufWriter<std::process::ChildStdin>> {
        let stdin = self
            .child
            .stdin
            .take()
            .ok_or_else(|| custom("ffmpeg has no stdin"))?;
        Ok(BufWriter::with_capacity(1 << 20, stdin))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// Waits for ffmpeg to finish; a failure is an error that quotes what it printed.
    fn finish(mut self, step: &str) -> Result<()> {
        let status = self
            .child
            .wait()
            .map_err(|e| io(e, "waiting for ffmpeg"))?;
        let text = self
            .stderr
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default();
        if status.success() {
            return Ok(());
        }
        let tail = String::from_utf8_lossy(&text[text.len().saturating_sub(STDERR_TAIL)..]);
        Err(custom(format!(
            "ffmpeg ({step}) failed with {status}: {}",
            tail.trim()
        )))
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // A run that was not finished (an error on the way) must not leave ffmpeg behind.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
