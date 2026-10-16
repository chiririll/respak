//! respak-nihav: everything respak takes from NihAV.
//!
//! The only crate that knows NihAV. It reads Bink 2 (`bik`, decoded by `nihav_rad`) and fills in
//! the traits of `respak_formats::videofile`: [`Mjpeg`], [`Cinepak`], [`RawVideo`] and [`Zmbv`]
//! are [`Codec`]s, [`NihavContainer`] writes `avi`, `mov` and `y4m`. Everything NihAV returns is
//! converted to respak's own types here, its errors become `Custom` errors that name the step, and
//! a panic inside it is caught.

mod bik;
mod bik_decode;
mod enc;
mod mjpeg;
mod mux;
mod simple;

use std::fmt::Debug;
use std::sync::Arc;

use respak_core::capability::{Audio, Video};
use respak_core::{Error, ErrorKind, Plugin, Registry};
use respak_formats::{Codec, VideoFileEncode};

pub use bik::{BikFile, BikHeader, BikRead};
pub use mjpeg::Mjpeg;
pub use mux::NihavContainer;
pub use simple::{Cinepak, RawVideo, Zmbv};

/// Registers `bik` (read) and `avi`, `mov` and `y4m` (written from any `Video`).
pub struct NihavPlugin;

impl Plugin for NihavPlugin {
    fn id(&self) -> &str {
        "nihav"
    }

    fn register(&self, r: &mut Registry) {
        r.format("bik")
            .label("Bink 2 video")
            .from("file", BikRead)
            .extensions(["bik", "bk2"])
            .provides::<dyn Video>()
            .provides::<dyn Audio>();
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
                    .from_cap::<dyn Video>(VideoFileEncode::new(Arc::new(container), codecs()));
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
