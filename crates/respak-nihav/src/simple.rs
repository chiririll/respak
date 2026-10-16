//! The codecs that are a small step from `Driver`: `cinepak`, `rawvideo` and `zmbv`.

use nihav_core::formats::YUV420_FORMAT;
use nihav_core::frame::NAVideoInfo;
use respak_core::capability::{FrameFormat, VideoFrame};
use respak_core::Result;
use respak_formats::{Codec, CodecSetup, FrameEncoder, Packet};

use crate::enc::{bgr0, bgr0_buffer, Driver, Range};
use crate::guarded;
use crate::mjpeg::Planar;

/// `cinepak`: lossy and small, plays on everything old; sizes are multiples of 4 (padded).
pub struct Cinepak;

impl Codec for Cinepak {
    fn name(&self) -> &'static str {
        "cinepak"
    }

    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>> {
        const STEP: &str = "cinepak encoder";
        guarded(STEP, || {
            let (w, h) = (setup.width as usize, setup.height as usize);
            let info = NAVideoInfo::new(w.div_ceil(4) * 4, h.div_ceil(4) * 4, false, YUV420_FORMAT);
            let driver = Driver::open(STEP, "cinepak", info, setup)?;
            Ok(Box::new(Planar {
                driver,
                source: (w, h),
                range: Range::Keep,
            }) as Box<dyn FrameEncoder>)
        })
    }
}

/// `rawvideo`: uncompressed planar YUV 4:2:0 at the source size. A full-range source is brought
/// to limited range, which is what readers of raw YUV assume.
pub struct RawVideo;

impl Codec for RawVideo {
    fn name(&self) -> &'static str {
        "rawvideo"
    }

    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>> {
        const STEP: &str = "rawvideo encoder";
        guarded(STEP, || {
            let (w, h) = (setup.width as usize, setup.height as usize);
            let info = NAVideoInfo::new(w, h, false, YUV420_FORMAT);
            let driver = Driver::open(STEP, "rawvideo", info, setup)?;
            Ok(Box::new(Planar {
                driver,
                source: (w, h),
                range: Range::to_limited(setup.full_range),
            }) as Box<dyn FrameEncoder>)
        })
    }
}

/// `zmbv`: lossless, from RGB; `quality` does not apply.
pub struct Zmbv;

impl Codec for Zmbv {
    fn name(&self) -> &'static str {
        "zmbv"
    }

    fn open(&self, setup: &CodecSetup) -> Result<Box<dyn FrameEncoder>> {
        const STEP: &str = "zmbv encoder";
        guarded(STEP, || {
            let info = NAVideoInfo::new(setup.width as usize, setup.height as usize, false, bgr0());
            Ok(Box::new(Packed {
                driver: Driver::open(STEP, "zmbv", info, setup)?,
            }) as Box<dyn FrameEncoder>)
        })
    }
}

/// An encoder that takes packed B, G, R, 0 pixels.
struct Packed {
    driver: Driver,
}

impl FrameEncoder for Packed {
    fn wants(&self) -> FrameFormat {
        FrameFormat::Rgba8
    }

    fn coded_size(&self) -> (u32, u32) {
        (
            self.driver.info.get_width() as u32,
            self.driver.info.get_height() as u32,
        )
    }

    fn encode(&mut self, frame: &VideoFrame, out: &mut Vec<Packet>) -> Result<()> {
        let info = self.driver.info;
        self.driver.guard(|d| {
            let buffer = bgr0_buffer(frame, info, "zmbv encoder")?;
            d.push(buffer, out)
        })
    }

    fn finish(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        self.driver.guard(|d| d.finish(out))
    }
}
