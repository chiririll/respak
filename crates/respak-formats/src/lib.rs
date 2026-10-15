//! respak-formats: common formats (png, dds). One plugin, no containers.

mod bc;
mod cast;
mod dds;
mod frames;
mod png;
mod videofile;
mod wav;

use respak_core::value::FileData;
use respak_core::{
    capability::{Animation, Audio, Bitmap, Mesh, Video},
    Error, ErrorKind, Plugin, Registry, Result, Value,
};

pub use bc::decode_rgba8;
pub use cast::{
    AllBlends, AllLods, BlendSelect, CastAnimationEncode, CastEncode, CastOptions, CastWriter, LodSelect, RootFixup,
};
pub use dds::{DdsEncode, DdsImage, DdsOptions, DdsRead};
pub use frames::{FramesEncode, FramesOptions};
pub use png::{Mips, PngEncode, PngImage, PngOptions, PngRead};
pub use videofile::{
    Codec, CodecSetup, Container, FrameEncoder, MuxSetup, Packet, PacketSink, VideoFileEncode, VideoFileOptions,
};
pub use wav::{WavEncode, WavOptions};

/// Registers `png`, `dds`, `png.frames` and `wav`.
pub struct FormatsPlugin;

impl Plugin for FormatsPlugin {
    fn id(&self) -> &str {
        "formats"
    }

    fn register(&self, r: &mut Registry) {
        r.format("png")
            .from("file", PngRead)
            .extensions(["png"])
            .provides::<dyn Bitmap>()
            .from_cap::<dyn Bitmap>(PngEncode)
            .cost(2);
        r.format("dds")
            .from("file", DdsRead)
            .extensions(["dds"])
            .provides::<dyn Bitmap>()
            .from_cap::<dyn Bitmap>(DdsEncode);
        // Written only: a folder of `frame_00001.png ...` from any video.
        r.format("png.frames").label("PNG frames").from_cap::<dyn Video>(FramesEncode);
        // Written only: a Cast model (skeleton, materials, meshes) from any mesh, or Cast animations
        // (skeleton, curves) from any animation.
        r.format("cast")
            .label("Cast model")
            .extensions(["cast"])
            .from_cap::<dyn Mesh>(CastEncode)
            .from_cap::<dyn Animation>(CastAnimationEncode);
        // Written only: 16-bit PCM from any audio.
        r.format("wav").label("WAV audio").extensions(["wav"]).from_cap::<dyn Audio>(WavEncode);
    }
}

fn file_of(source: &Value) -> Result<&FileData> {
    source.downcast_ref::<FileData>().ok_or_else(|| {
        Error::new(ErrorKind::Custom { source: None, message: format!("expected a file, got `{}`", source.format()) })
    })
}

fn bitmap_of(source: &Value) -> Result<&dyn Bitmap> {
    source.as_bitmap().ok_or_else(|| {
        Error::new(ErrorKind::Custom {
            source: None,
            message: format!("`{}` has no Bitmap capability", source.format()),
        })
    })
}

fn video_of(source: &Value) -> Result<&dyn Video> {
    source.as_video().ok_or_else(|| {
        Error::new(ErrorKind::Custom {
            source: None,
            message: format!("`{}` has no Video capability", source.format()),
        })
    })
}

fn audio_of(source: &Value) -> Result<&dyn Audio> {
    source.as_audio().ok_or_else(|| {
        Error::new(ErrorKind::Custom {
            source: None,
            message: format!("`{}` has no Audio capability", source.format()),
        })
    })
}

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

fn unsupported(format: &str, what: &str) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: format.to_owned(), version: what.to_owned() })
}

/// The whole file (PNG is a single deflate stream and cannot be read in parts).
async fn read_all(file: &FileData) -> Result<bytes::Bytes> {
    let reader = file.op.read_with(&file.path);
    let buf = match &file.range {
        Some(r) => reader.range(r.clone()).await?,
        None => reader.await?,
    };
    Ok(buf.to_bytes())
}
