//! `mjpeg`: NihAV's JPEG encoder, one baseline JPEG per frame.
//!
//! The encoder takes only sizes that are multiples of 16 and full-range YUV, so each frame is
//! copied into a padded buffer (the last column and row are repeated) and, if the source is
//! limited range, expanded.

use nihav_core::codecs::{EncodeParameters, NAEncoder, RegisteredEncoders};
use nihav_core::formats::YUV420_FORMAT;
use nihav_core::frame::{
    alloc_video_buffer, FrameType, NABufferType, NACodecInfo, NACodecTypeInfo, NAFrame, NATimeInfo, NAVideoInfo,
};
use respak_core::capability::{FrameFormat, VideoFrame};
use respak_core::{Error, ErrorKind, Result};
use respak_formats::{Codec, CodecSetup, FrameEncoder, Packet};

use crate::{backend, guarded};

/// The `mjpeg` codec.
pub struct Mjpeg;

impl Codec for Mjpeg {
    fn name(&self) -> &'static str {
        "mjpeg"
    }

    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>> {
        guarded("jpeg encoder", || {
            let (w, h) = (setup.width as usize, setup.height as usize);
            let coded = (w.div_ceil(16) * 16, h.div_ceil(16) * 16);
            let info = NAVideoInfo::new(coded.0, coded.1, false, YUV420_FORMAT);
            let mut registry = RegisteredEncoders::new();
            nihav_commonfmt::generic_register_all_encoders(&mut registry);
            let create = registry.find_encoder("jpeg").ok_or_else(|| {
                Error::new(ErrorKind::Custom { source: None, message: "the jpeg encoder is not built in".into() })
            })?;
            let mut encoder = create();
            let wanted = EncodeParameters {
                format: NACodecTypeInfo::Video(info),
                quality: setup.quality,
                bitrate: 0,
                tb_num: setup.fps.1,
                tb_den: setup.fps.0,
                flags: 0,
            };
            let params = encoder.negotiate_format(&wanted).map_err(|e| backend("jpeg encoder", e))?;
            encoder.init(0, params).map_err(|e| backend("jpeg encoder", e))?;
            Ok(Box::new(JpegEncoder { encoder, info, source: (w, h), fps: setup.fps, full_range: setup.full_range, next: 0 })
                as Box<dyn FrameEncoder>)
        })
    }
}

struct JpegEncoder {
    encoder: Box<dyn NAEncoder + Send>,
    info: NAVideoInfo,
    /// Size of the source frames.
    source: (usize, usize),
    fps: (u32, u32),
    full_range: bool,
    next: u64,
}

impl FrameEncoder for JpegEncoder {
    fn wants(&self) -> FrameFormat {
        FrameFormat::Yuv420
    }

    fn coded_size(&self) -> (u32, u32) {
        (self.info.get_width() as u32, self.info.get_height() as u32)
    }

    fn encode(&mut self, frame: &VideoFrame, out: &mut Vec<Packet>) -> Result<()> {
        guarded("jpeg encoder", || {
            let buffer = self.copy(frame)?;
            let ts = NATimeInfo::new(Some(self.next), None, Some(1), self.fps.1, self.fps.0);
            self.next += 1;
            let nframe = NAFrame::new(ts, FrameType::I, true, NACodecInfo::new_dummy(), buffer);
            self.encoder.encode(&nframe).map_err(|e| backend("jpeg encoder", e))?;
            self.drain(out)
        })
    }

    fn finish(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        guarded("jpeg encoder", || {
            self.encoder.flush().map_err(|e| backend("jpeg encoder", e))?;
            self.drain(out)
        })
    }
}

impl JpegEncoder {
    fn drain(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        while let Some(packet) = self.encoder.get_packet().map_err(|e| backend("jpeg encoder", e))? {
            out.push(Packet { data: packet.get_buffer().to_vec(), keyframe: packet.is_keyframe() });
        }
        Ok(())
    }

    /// The frame's planes in a padded encoder buffer.
    fn copy(&self, frame: &VideoFrame) -> Result<NABufferType> {
        let bad = |what: &str| Error::new(ErrorKind::Corrupt { context: format!("frame {}: {what}", frame.index) });
        if frame.format != FrameFormat::Yuv420 || frame.planes.len() != 3 || frame.strides.len() != 3 {
            return Err(bad("expected three YUV 4:2:0 planes"));
        }
        let mut buffer = alloc_video_buffer(self.info, 1).map_err(|e| backend("jpeg encoder", e))?;
        let NABufferType::Video(vbuf) = &mut buffer else { return Err(backend("jpeg encoder", "no video buffer")) };
        let offsets = [vbuf.get_offset(0), vbuf.get_offset(1), vbuf.get_offset(2)];
        let strides = [vbuf.get_stride(0), vbuf.get_stride(1), vbuf.get_stride(2)];
        let dims = [vbuf.get_dimensions(0), vbuf.get_dimensions(1), vbuf.get_dimensions(2)];
        let data = vbuf
            .as_mut()
            .and_then(|b| b.get_data_mut())
            .ok_or_else(|| backend("jpeg encoder", "the frame buffer is shared"))?;
        for plane in 0..3 {
            let (src_w, src_h) = if plane == 0 { self.source } else { (self.source.0.div_ceil(2), self.source.1.div_ceil(2)) };
            let (dst_w, dst_h) = dims[plane];
            let (src, src_stride) = (&frame.planes[plane], frame.strides[plane]);
            if src_w == 0 || src_h == 0 || src.len() < (src_h - 1) * src_stride + src_w {
                return Err(bad("a plane is too short"));
            }
            for row in 0..dst_h {
                let from = &src[row.min(src_h - 1) * src_stride..][..src_w];
                let to = &mut data[offsets[plane] + row * strides[plane]..][..dst_w];
                for (col, byte) in to.iter_mut().enumerate() {
                    let sample = from[col.min(src_w - 1)];
                    *byte = if self.full_range { sample } else { expand(sample, plane == 0) };
                }
            }
        }
        Ok(buffer)
    }
}

/// A limited-range sample (Y 16..=235, chroma 16..=240) as full range.
fn expand(sample: u8, luma: bool) -> u8 {
    let s = f32::from(sample);
    let v = if luma { (s - 16.0) * 255.0 / 219.0 } else { (s - 128.0) * 255.0 / 224.0 + 128.0 };
    v.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn limited_range_is_expanded_to_full_range() {
        assert_eq!((expand(16, true), expand(235, true)), (0, 255));
        assert_eq!(expand(128, false), 128);
        // Chroma 16..=240 is symmetric around 128, so its ends land on 0.5 and 255.5.
        assert!(expand(16, false) <= 1 && expand(240, false) == 255);
        assert_eq!((expand(0, true), expand(255, true)), (0, 255), "out of range values clamp");
    }
}
