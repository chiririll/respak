//! `mjpeg`: NihAV's JPEG encoder, one baseline JPEG per frame.
//!
//! The encoder takes only sizes that are multiples of 16 and full-range YUV, so each frame is
//! copied into a padded buffer (the last column and row are repeated) and, if the source is
//! limited range, expanded.

use nihav_core::formats::YUV420_FORMAT;
use nihav_core::frame::NAVideoInfo;
use respak_core::capability::{FrameFormat, VideoFrame};
use respak_core::Result;
use respak_formats::{Codec, CodecSetup, FrameEncoder, Packet};

use crate::enc::{yuv420_buffer, Driver, Range};
use crate::guarded;

const STEP: &str = "jpeg encoder";

/// The `mjpeg` codec.
pub struct Mjpeg;

impl Codec for Mjpeg {
    fn name(&self) -> &'static str {
        "mjpeg"
    }

    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>> {
        guarded(STEP, || {
            let (w, h) = (setup.width as usize, setup.height as usize);
            let info = NAVideoInfo::new(
                w.div_ceil(16) * 16,
                h.div_ceil(16) * 16,
                false,
                YUV420_FORMAT,
            );
            let driver = Driver::open(STEP, "jpeg", info, setup)?;
            Ok(Box::new(Planar {
                driver,
                source: (w, h),
                range: Range::to_full(setup.full_range),
            }) as Box<dyn FrameEncoder>)
        })
    }
}

/// A YUV 4:2:0 encoder whose buffer may be larger than the source (padded) and whose samples may
/// need a range change.
pub(crate) struct Planar {
    pub driver: Driver,
    pub source: (usize, usize),
    pub range: Range,
}

impl FrameEncoder for Planar {
    fn wants(&self) -> FrameFormat {
        FrameFormat::Yuv420
    }

    fn coded_size(&self) -> (u32, u32) {
        (
            self.driver.info.get_width() as u32,
            self.driver.info.get_height() as u32,
        )
    }

    fn encode(&mut self, frame: &VideoFrame, out: &mut Vec<Packet>) -> Result<()> {
        let (info, source, range) = (self.driver.info, self.source, self.range);
        self.driver.guard(|d| {
            let buffer = yuv420_buffer(frame, info, source, range, STEP)?;
            d.push(buffer, out)
        })
    }

    fn finish(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        self.driver.guard(|d| d.finish(out))
    }
}
