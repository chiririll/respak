//! respak-rad: RAD Game Tools formats (Titanfall 2, Apex Legends): Miles Sound System banks
//! and Bink 2 video.
//!
//! Miles: `mbnk` opens as a volume of sources (`asrc`), `mstr` is the stream file that holds their
//! bytes, and an `asrc` is an [`Audio`](respak_core::capability::Audio). Format spec:
//! `docs/formats/miles.md` and `docs/formats/miles-codecs.md`.

mod asrc;
mod bcf1;
mod bik;
mod bik_decode;
#[cfg(test)]
mod e2e_tests;
mod mbnk;
mod mstr;
mod rada;

use respak_core::capability::{Audio, Container, Metadata, Video};
use respak_core::{Plugin, Registry};

pub use asrc::AsrcData;
pub use bcf1::{Bcf1Frame, Bcf1Header};
pub use bik::{BikFile, BikHeader, BikRead};
pub use mbnk::{BankLayout, MbnkData, MbnkRead, SourceEntry};
pub use mstr::{MstrData, MstrHeader};
pub use rada::RadaHeader;

/// Registers `bik` (read from a file, a `Video` with `Audio`), `mbnk` (read from a file, a container of `asrc`), `mstr` (header check) and `asrc`
/// (handed out by the `mbnk` volume, an `Audio`).
pub struct RadPlugin;

impl Plugin for RadPlugin {
    fn id(&self) -> &str {
        "rad"
    }

    fn register(&self, r: &mut Registry) {
        r.format("bik")
            .label("Bink 2 video")
            .from("file", bik::BikRead)
            .extensions(["bik", "bk2"])
            .provides::<dyn Video>()
            .provides::<dyn Audio>();
        r.format("mbnk")
            .label("Miles sound bank")
            .from("file", mbnk::MbnkRead)
            .extensions(["mbnk"])
            .provides::<dyn Container>();
        r.format("mstr")
            .label("Miles stream file")
            .from("file", mstr::MstrRead)
            .extensions(["mstr"]);
        r.format("asrc")
            .label("Miles audio source")
            .extensions(["asrc"])
            .provides::<dyn Audio>()
            .provides::<dyn Metadata>();
    }
}
