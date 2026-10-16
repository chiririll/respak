//! respak-miles: Miles Sound System banks (Titanfall 2, Apex Legends).
//!
//! `mbnk` opens as a volume of sources (`asrc`), `mstr` is the stream file that holds their
//! bytes, and an `asrc` is an [`Audio`](respak_core::capability::Audio). Format spec:
//! `todo/formats/miles.md` and `todo/formats/miles-codecs.md`.

mod asrc;
mod bcf1;
#[cfg(test)]
mod e2e_tests;
mod mbnk;
mod mstr;
mod rada;

use respak_core::capability::{Audio, Container, Metadata};
use respak_core::{Error, ErrorKind, Plugin, Registry};

pub use asrc::AsrcData;
pub use bcf1::{Bcf1Frame, Bcf1Header};
pub use mbnk::{BankLayout, MbnkData, MbnkRead, SourceEntry};
pub use mstr::{MstrData, MstrHeader};
pub use rada::RadaHeader;

/// Registers `mbnk` (read from a file, a container of `asrc`), `mstr` (header check) and `asrc`
/// (handed out by the `mbnk` volume, an `Audio`).
pub struct MilesPlugin;

impl Plugin for MilesPlugin {
    fn id(&self) -> &str {
        "miles"
    }

    fn register(&self, r: &mut Registry) {
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

/// A damaged file.
fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}
