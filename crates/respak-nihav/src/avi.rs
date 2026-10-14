//! `avi`: NihAV's AVI muxer, with MJPEG video and 16-bit PCM audio.

use std::fs::File;
use std::io::BufWriter;

use nihav_core::codecs::NAStreamRef;
use nihav_core::formats::{SND_S16_FORMAT, YUV420_FORMAT};
use nihav_core::frame::{
    NAAudioInfo, NACodecInfo, NACodecTypeInfo, NAPacket, NAStream, NATimeInfo, NAVideoInfo, StreamType,
};
use nihav_core::io::byteio::FileWriter;
use nihav_core::muxers::{create_muxer, Muxer, RegisteredMuxers};
use nihav_core::demuxers::StreamManager;
use respak_core::{Error, ErrorKind, Result};
use respak_formats::{Container, MuxSetup, Packet, PacketSink};

use crate::{backend, guarded};

/// The `avi` container.
pub struct Avi;

impl Container for Avi {
    fn name(&self) -> &'static str {
        "avi"
    }

    fn codecs(&self) -> &[&'static str] {
        &["mjpeg"]
    }

    fn mux(
        &self,
        setup: &MuxSetup<'_>,
        file: &mut File,
        feed: &mut dyn FnMut(&mut dyn PacketSink) -> Result<()>,
    ) -> Result<()> {
        if setup.codec != "mjpeg" {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!("`avi` cannot hold codec `{}`", setup.codec),
            }));
        }
        guarded("avi muxer", || {
            let (num, den) = setup.fps;
            let vinfo = NAVideoInfo::new(setup.size.0 as usize, setup.size.1 as usize, false, YUV420_FORMAT);
            let vcodec = NACodecInfo::new("jpeg", NACodecTypeInfo::Video(vinfo), None);
            let mut streams = StreamManager::new();
            streams.add_stream(NAStream::new(StreamType::Video, 0, vcodec, den, num, 0));
            if let Some(a) = setup.audio {
                let ainfo = NAAudioInfo::new(a.sample_rate, a.channels as u8, SND_S16_FORMAT, 0);
                let acodec = NACodecInfo::new("pcm", NACodecTypeInfo::Audio(ainfo), None);
                streams.add_stream(NAStream::new(StreamType::Audio, 1, acodec, 1, a.sample_rate, 0));
            }

            // A buffer in front of the file; it flushes before every seek, so the header patches
            // at the end land in the right place.
            let mut writer = FileWriter::new_write(BufWriter::with_capacity(1 << 20, &mut *file));
            let mut registry = RegisteredMuxers::new();
            nihav_commonfmt::generic_register_all_muxers(&mut registry);
            let creator = registry.find_muxer("avi").ok_or_else(|| backend("avi muxer", "not built in"))?;
            let mut muxer = create_muxer(creator, streams, &mut writer).map_err(|e| backend("avi muxer", e))?;

            let video = muxer.get_stream(0).ok_or_else(|| backend("avi muxer", "no video stream"))?;
            let audio = muxer.get_stream(1);
            let mut sink = AviSink {
                muxer: &mut muxer,
                video,
                audio,
                frame: 0,
                samples: 0,
                fps: setup.fps,
                sample_rate: setup.audio.map_or(1, |a| a.sample_rate),
                block: setup.audio.map_or(2, |a| usize::from(a.channels) * 2),
            };
            feed(&mut sink)?;
            muxer.end().map_err(|e| backend("avi muxer", e))
        })
    }
}

struct AviSink<'a, 'b> {
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

impl PacketSink for AviSink<'_, '_> {
    fn video(&mut self, packet: Packet) -> Result<()> {
        let ts = NATimeInfo::new(Some(self.frame), None, Some(1), self.fps.1, self.fps.0);
        self.frame += 1;
        let packet = NAPacket::new(self.video.clone(), ts, packet.keyframe, packet.data);
        self.muxer.mux_frame(packet).map_err(|e| backend("avi muxer", e))
    }

    fn audio(&mut self, pcm: &[u8]) -> Result<()> {
        let Some(stream) = self.audio.clone() else { return Ok(()) };
        let frames = (pcm.len() / self.block) as u64;
        let ts = NATimeInfo::new(Some(self.samples), None, Some(frames), 1, self.sample_rate);
        self.samples += frames;
        let packet = NAPacket::new(stream, ts, true, pcm[..frames as usize * self.block].to_vec());
        self.muxer.mux_frame(packet).map_err(|e| backend("avi muxer", e))
    }
}
