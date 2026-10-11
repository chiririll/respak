//! respak-formats: common formats (png, dds). One plugin, no containers.

mod dds;
mod png;

use respak_core::value::FileData;
use respak_core::{capability::Bitmap, Error, ErrorKind, Plugin, Registry, Result, Value};

pub use dds::{DdsEncode, DdsImage, DdsRead};
pub use png::{PngEncode, PngImage, PngRead};

/// Registers `png` and `dds`.
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

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

fn unsupported(format: &str, what: &str) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: format.to_owned(), version: what.to_owned() })
}

/// Swaps the red and blue channels of RGBA/BGRA pixels in place.
fn swap_rb(pixels: &mut [u8]) {
    for p in pixels.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
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
