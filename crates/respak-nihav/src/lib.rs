//! respak-nihav: everything respak takes from NihAV.
//!
//! The only crate that knows NihAV's muxers and encoders. It fills in the traits of `respak_formats::videofile`: [`Mjpeg`], [`Cinepak`], [`RawVideo`] and [`Zmbv`]
//! are [`Codec`]s, [`NihavContainer`] writes `avi`, `mov` and `y4m`. Everything NihAV returns is
//! converted to respak's own types here, its errors become `Custom` errors that name the step, and
//! a panic inside it is caught.

mod enc;
mod mjpeg;
mod mux;
mod simple;

use std::fmt::Debug;
use std::sync::Arc;

use respak_core::capability::Video;
use respak_core::{Error, ErrorKind, Plugin, Registry};
use respak_formats::{Codec, VideoFileEncode};

pub use mjpeg::Mjpeg;
pub use mux::NihavContainer;
pub use simple::{Cinepak, RawVideo, Zmbv};

/// Registers `avi`, `mov` and `y4m` (written from any `Video`).
pub struct NihavPlugin;

impl Plugin for NihavPlugin {
    fn id(&self) -> &str {
        "nihav"
    }

    fn register(&self, r: &mut Registry) {
        let codecs = || -> Vec<Arc<dyn Codec>> {
            vec![
                Arc::new(Mjpeg),
                Arc::new(Cinepak),
                Arc::new(Zmbv),
                Arc::new(RawVideo),
            ]
        };
        let mut video_file =
            |name: &'static str, label: &'static str, container: NihavContainer| {
                r.format(name)
                    .label(label)
                    .extensions([name])
                    .from_cap::<dyn Video>(VideoFileEncode::new(Arc::new(container), codecs()))
                    .options::<respak_formats::VideoFileOptions>();
            };
        video_file("avi", "AVI video", NihavContainer::avi());
        video_file("mov", "QuickTime video", NihavContainer::mov());
        video_file("y4m", "YUV4MPEG2 video", NihavContainer::y4m());
    }
}

/// A NihAV failure as an error that says which step failed.
fn backend(step: &str, e: impl Debug) -> Error {
    Error::new(ErrorKind::Custom {
        source: None,
        message: format!("{step}: {e:?}"),
    })
}

/// Runs `f`, turning a panic inside NihAV into an error.
fn guarded<T>(step: &str, f: impl FnOnce() -> respak_core::Result<T>) -> respak_core::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(backend(step, "NihAV panicked")))
}
