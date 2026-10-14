//! The container formats NihAV writes: `avi`, `mov` and `y4m`, one [`NihavContainer`] each.
//!
//! They differ in the muxer's name, the codecs they hold and whether they carry sound; the
//! streams and the packet flow are the same. Audio is 16-bit little-endian PCM.

use std::fs::File;
use std::io::BufWriter;

use nihav_core::codecs::NAStreamRef;
use nihav_core::demuxers::StreamManager;
use nihav_core::formats::SND_S16_FORMAT;
use nihav_core::frame::{
    NAAudioInfo, NACodecInfo, NACodecTypeInfo, NAPacket, NAStream, NATimeInfo, NAVideoInfo, StreamType,
};
use nihav_core::io::byteio::FileWriter;
use nihav_core::muxers::{create_muxer, Muxer, RegisteredMuxers};
use respak_core::{Error, ErrorKind, Result};
use respak_formats::{Container, MuxSetup, Packet, PacketSink};

use crate::enc::stream_codec;
use crate::{backend, guarded};

/// A container written by one of NihAV's muxers.
pub struct NihavContainer {
    name: &'static str,
    /// The muxer's name in NihAV.
    muxer: &'static str,
    codecs: &'static [&'static str],
    audio: bool,
}

impl NihavContainer {
    /// AVI: MJPEG, Cinepak or ZMBV, with PCM sound.
    pub const fn avi() -> Self {
        Self { name: "avi", muxer: "avi", codecs: &["mjpeg", "cinepak", "zmbv"], audio: true }
    }

    /// QuickTime MOV: MJPEG or Cinepak, with PCM sound.
    pub const fn mov() -> Self {
        Self { name: "mov", muxer: "mov", codecs: &["mjpeg", "cinepak"], audio: true }
    }

    /// YUV4MPEG2: raw planar video, no sound.
    pub const fn y4m() -> Self {
        Self { name: "y4m", muxer: "yuv4mpeg", codecs: &["rawvideo"], audio: false }
    }
}

impl Container for NihavContainer {
    fn name(&self) -> &'static str {
        self.name
    }

    fn codecs(&self) -> &[&'static str] {
        self.codecs
    }

    fn has_audio(&self) -> bool {
        self.audio
    }

    fn mux(
        &self,
        setup: &MuxSetup<'_>,
        file: &mut File,
        feed: &mut dyn FnMut(&mut dyn PacketSink) -> Result<()>,
    ) -> Result<()> {
        let step = self.name;
        let (codec, format) = self
            .codecs
            .contains(&setup.codec)
            .then(|| stream_codec(setup.codec))
            .flatten()
            .ok_or_else(|| {
                Error::new(ErrorKind::InvalidOptions {
                    message: format!("`{}` cannot hold codec `{}`", self.name, setup.codec),
                })
            })?;
        guarded(step, || {
            let (num, den) = setup.fps;
            let vinfo = NAVideoInfo::new(setup.size.0 as usize, setup.size.1 as usize, false, format);
            let vcodec = NACodecInfo::new(codec, NACodecTypeInfo::Video(vinfo), None);
            let mut streams = StreamManager::new();
            streams.add_stream(NAStream::new(StreamType::Video, 0, vcodec, den, num, 0));
            let audio = setup.audio.filter(|_| self.audio);
            if let Some(a) = audio {
                let ainfo = NAAudioInfo::new(a.sample_rate, a.channels as u8, SND_S16_FORMAT, 0);
                let acodec = NACodecInfo::new("pcm", NACodecTypeInfo::Audio(ainfo), None);
                streams.add_stream(NAStream::new(StreamType::Audio, 1, acodec, 1, a.sample_rate, 0));
            }

            {
                // A buffer in front of the file; it flushes before every seek, so the header
                // patches at the end land in the right place (and when it is dropped).
                let mut writer = FileWriter::new_write(BufWriter::with_capacity(1 << 20, &mut *file));
                let mut registry = RegisteredMuxers::new();
                nihav_commonfmt::generic_register_all_muxers(&mut registry);
                let creator = registry.find_muxer(self.muxer).ok_or_else(|| backend(step, "not built in"))?;
                let mut muxer = create_muxer(creator, streams, &mut writer).map_err(|e| backend(step, e))?;

                let video = muxer.get_stream(0).ok_or_else(|| backend(step, "no video stream"))?;
                let audio_stream = muxer.get_stream(1);
                let mut sink = Sink {
                    step,
                    muxer: &mut muxer,
                    video,
                    audio: audio_stream,
                    frame: 0,
                    samples: 0,
                    fps: setup.fps,
                    sample_rate: audio.map_or(1, |a| a.sample_rate),
                    block: audio.map_or(2, |a| usize::from(a.channels) * 2),
                };
                feed(&mut sink)?;
                muxer.end().map_err(|e| backend(step, e))?;
            }
            Ok(())
        })
    }
}

struct Sink<'a, 'b> {
    step: &'static str,
    muxer: &'a mut Muxer<'b>,
    video: NAStreamRef,
    audio: Option<NAStreamRef>,
    /// Video packets written.
    frame: u64,
    /// Sample frames of audio written.
    samples: u64,
    fps: (u32, u32),
    sample_rate: u32,
    /// Bytes per sample frame.
    block: usize,
}

impl PacketSink for Sink<'_, '_> {
    fn video(&mut self, packet: Packet) -> Result<()> {
        let ts = NATimeInfo::new(Some(self.frame), None, Some(1), self.fps.1, self.fps.0);
        self.frame += 1;
        let packet = NAPacket::new(self.video.clone(), ts, packet.keyframe, packet.data);
        self.muxer.mux_frame(packet).map_err(|e| backend(self.step, e))
    }

    fn audio(&mut self, pcm: &[u8]) -> Result<()> {
        let Some(stream) = self.audio.clone() else { return Ok(()) };
        let frames = (pcm.len() / self.block) as u64;
        let ts = NATimeInfo::new(Some(self.samples), None, Some(frames), 1, self.sample_rate);
        self.samples += frames;
        let packet = NAPacket::new(stream, ts, true, pcm[..frames as usize * self.block].to_vec());
        self.muxer.mux_frame(packet).map_err(|e| backend(self.step, e))
    }
}
