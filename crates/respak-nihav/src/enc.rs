//! What the NihAV video encoders have in common: opening one, feeding it a buffer, collecting its
//! packets, and building the buffers the planar and packed codecs take.

use nihav_core::codecs::{EncodeParameters, NAEncoder, RegisteredEncoders};
use nihav_core::formats::{
    ColourModel, NAPixelChromaton, NAPixelFormaton, RGBSubmodel, YUV420_FORMAT,
};
use nihav_core::frame::{
    alloc_video_buffer, FrameType, NABufferType, NACodecInfo, NACodecTypeInfo, NAFrame, NATimeInfo,
    NAVideoInfo,
};
use respak_core::capability::{FrameFormat, VideoFrame};
use respak_core::{Error, ErrorKind, Result};
use respak_formats::{CodecSetup, Packet};

use crate::{backend, guarded};

/// What a codec is called in NihAV and the pixel format its stream declares.
pub(crate) fn stream_codec(codec: &str) -> Option<(&'static str, NAPixelFormaton)> {
    match codec {
        "mjpeg" => Some(("jpeg", YUV420_FORMAT)),
        "cinepak" => Some(("cinepak", YUV420_FORMAT)),
        "rawvideo" => Some(("rawvideo", YUV420_FORMAT)),
        "zmbv" => Some(("zmbv", bgr0())),
        _ => None,
    }
}

/// 8-bit RGB stored as B, G, R and a padding byte (what the ZMBV encoder takes).
pub(crate) fn bgr0() -> NAPixelFormaton {
    let c = |comp_offs| {
        Some(NAPixelChromaton {
            h_ss: 0,
            v_ss: 0,
            packed: true,
            depth: 8,
            shift: 0,
            comp_offs,
            next_elem: 4,
        })
    };
    NAPixelFormaton {
        model: ColourModel::RGB(RGBSubmodel::RGB),
        components: 3,
        comp_info: [c(2), c(1), c(0), None, None],
        elem_size: 4,
        be: false,
        alpha: false,
        palette: false,
    }
}

/// A NihAV encoder with the bookkeeping to push frames into it.
pub(crate) struct Driver {
    step: &'static str,
    encoder: Box<dyn NAEncoder + Send>,
    /// The frame format the encoder is opened with (and the buffers have).
    pub info: NAVideoInfo,
    fps: (u32, u32),
    next: u64,
}

impl Driver {
    pub fn open(
        step: &'static str,
        nihav: &str,
        info: NAVideoInfo,
        setup: &CodecSetup,
    ) -> Result<Self> {
        let mut registry = RegisteredEncoders::new();
        nihav_commonfmt::generic_register_all_encoders(&mut registry);
        let create = registry
            .find_encoder(nihav)
            .ok_or_else(|| backend(step, "not built in"))?;
        let mut encoder = create();
        let wanted = EncodeParameters {
            format: NACodecTypeInfo::Video(info),
            quality: setup.quality,
            bitrate: 0,
            tb_num: setup.fps.1,
            tb_den: setup.fps.0,
            flags: 0,
        };
        let params = encoder
            .negotiate_format(&wanted)
            .map_err(|e| backend(step, e))?;
        encoder.init(0, params).map_err(|e| backend(step, e))?;
        Ok(Self {
            step,
            encoder,
            info,
            fps: setup.fps,
            next: 0,
        })
    }

    pub fn push(&mut self, buffer: NABufferType, out: &mut Vec<Packet>) -> Result<()> {
        let ts = NATimeInfo::new(Some(self.next), None, Some(1), self.fps.1, self.fps.0);
        self.next += 1;
        let frame = NAFrame::new(ts, FrameType::I, true, NACodecInfo::new_dummy(), buffer);
        self.encoder
            .encode(&frame)
            .map_err(|e| backend(self.step, e))?;
        self.drain(out)
    }

    pub fn finish(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        self.encoder.flush().map_err(|e| backend(self.step, e))?;
        self.drain(out)
    }

    fn drain(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        while let Some(packet) = self
            .encoder
            .get_packet()
            .map_err(|e| backend(self.step, e))?
        {
            out.push(Packet {
                data: packet.get_buffer().to_vec(),
                keyframe: packet.is_keyframe(),
            });
        }
        Ok(())
    }

    /// Runs `f` with a panic inside NihAV turned into an error.
    pub fn guard<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let step = self.step;
        guarded(step, || f(self))
    }
}

fn bad_frame(frame: &VideoFrame, what: &str) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: format!("frame {}: {what}", frame.index),
    })
}

/// How the samples of a YUV frame are changed on the way into a buffer.
#[derive(Clone, Copy)]
pub(crate) enum Range {
    Keep,
    /// Limited range (Y 16..=235, chroma 16..=240) to full range.
    Expand,
    /// Full range to limited.
    Compress,
}

impl Range {
    /// Expands a limited-range source, for the codecs that only take full range.
    pub fn to_full(full: bool) -> Self {
        if full {
            Self::Keep
        } else {
            Self::Expand
        }
    }

    /// Compresses a full-range source, for the formats whose readers assume limited range.
    pub fn to_limited(full: bool) -> Self {
        if full {
            Self::Compress
        } else {
            Self::Keep
        }
    }

    pub fn apply(self, sample: u8, luma: bool) -> u8 {
        let s = f32::from(sample);
        let v = match (self, luma) {
            (Self::Keep, _) => return sample,
            (Self::Expand, true) => (s - 16.0) * 255.0 / 219.0,
            (Self::Expand, false) => (s - 128.0) * 255.0 / 224.0 + 128.0,
            (Self::Compress, true) => s * 219.0 / 255.0 + 16.0,
            (Self::Compress, false) => (s - 128.0) * 224.0 / 255.0 + 128.0,
        };
        v.round().clamp(0.0, 255.0) as u8
    }
}

/// The planes of a `Yuv420` frame of `source` size in a buffer of the (larger or equal) size
/// `info`, the last column and row repeated into the padding.
pub(crate) fn yuv420_buffer(
    frame: &VideoFrame,
    info: NAVideoInfo,
    source: (usize, usize),
    range: Range,
    step: &str,
) -> Result<NABufferType> {
    if frame.format != FrameFormat::Yuv420 || frame.planes.len() != 3 || frame.strides.len() != 3 {
        return Err(bad_frame(frame, "expected three YUV 4:2:0 planes"));
    }
    let mut buffer = alloc_video_buffer(info, 1).map_err(|e| backend(step, e))?;
    let NABufferType::Video(vbuf) = &mut buffer else {
        return Err(backend(step, "no video buffer"));
    };
    let offsets = [vbuf.get_offset(0), vbuf.get_offset(1), vbuf.get_offset(2)];
    let strides = [vbuf.get_stride(0), vbuf.get_stride(1), vbuf.get_stride(2)];
    let dims = [
        vbuf.get_dimensions(0),
        vbuf.get_dimensions(1),
        vbuf.get_dimensions(2),
    ];
    let data = vbuf
        .as_mut()
        .and_then(|b| b.get_data_mut())
        .ok_or_else(|| backend(step, "the frame buffer is shared"))?;
    for plane in 0..3 {
        let (src_w, src_h) = if plane == 0 {
            source
        } else {
            (source.0.div_ceil(2), source.1.div_ceil(2))
        };
        let (dst_w, dst_h) = dims[plane];
        let (src, src_stride) = (&frame.planes[plane], frame.strides[plane]);
        if src_w == 0 || src_h == 0 || src.len() < (src_h - 1) * src_stride + src_w {
            return Err(bad_frame(frame, "a plane is too short"));
        }
        for row in 0..dst_h {
            let from = &src[row.min(src_h - 1) * src_stride..][..src_w];
            let to = &mut data[offsets[plane] + row * strides[plane]..][..dst_w];
            for (col, byte) in to.iter_mut().enumerate() {
                *byte = range.apply(from[col.min(src_w - 1)], plane == 0);
            }
        }
    }
    Ok(buffer)
}

/// An `Rgba8` frame as a packed buffer of B, G, R and a zero byte per pixel.
pub(crate) fn bgr0_buffer(
    frame: &VideoFrame,
    info: NAVideoInfo,
    step: &str,
) -> Result<NABufferType> {
    if frame.format != FrameFormat::Rgba8 || frame.planes.len() != 1 || frame.strides.len() != 1 {
        return Err(bad_frame(frame, "expected one RGBA plane"));
    }
    let mut buffer = alloc_video_buffer(info, 1).map_err(|e| backend(step, e))?;
    let (w, h) = (info.get_width(), info.get_height());
    let (src, src_stride) = (&frame.planes[0], frame.strides[0]);
    if w == 0 || h == 0 || src.len() < (h - 1) * src_stride + w * 4 {
        return Err(bad_frame(frame, "the plane is too short"));
    }
    let vbuf = match &mut buffer {
        NABufferType::Video(v) | NABufferType::VideoPacked(v) => v,
        _ => return Err(backend(step, "no video buffer")),
    };
    let (offset, stride) = (vbuf.get_offset(0), vbuf.get_stride(0));
    let data = vbuf
        .as_mut()
        .and_then(|b| b.get_data_mut())
        .ok_or_else(|| backend(step, "the frame buffer is shared"))?;
    for row in 0..h {
        let from = &src[row * src_stride..][..w * 4];
        let to = &mut data[offset + row * stride..][..w * 4];
        for (px, out) in from.chunks_exact(4).zip(to.chunks_exact_mut(4)) {
            out.copy_from_slice(&[px[2], px[1], px[0], 0]);
        }
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::Range;

    #[test]
    fn limited_range_is_expanded_to_full_range() {
        let e = |s, l| Range::Expand.apply(s, l);
        assert_eq!((e(16, true), e(235, true)), (0, 255));
        assert_eq!(e(128, false), 128);
        // Chroma 16..=240 is symmetric around 128, so its ends land on 0.5 and 255.5.
        assert!(e(16, false) <= 1 && e(240, false) == 255);
        assert_eq!(
            (e(0, true), e(255, true)),
            (0, 255),
            "out of range values clamp"
        );
    }

    #[test]
    fn full_range_is_compressed_to_limited_range() {
        let c = |s, l| Range::Compress.apply(s, l);
        assert_eq!((c(0, true), c(255, true)), (16, 235));
        assert_eq!((c(128, false), c(0, false), c(255, false)), (128, 16, 240));
        assert_eq!(Range::Keep.apply(77, true), 77);
    }
}
