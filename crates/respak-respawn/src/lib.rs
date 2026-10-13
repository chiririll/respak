//! respak-respawn: Respawn formats. Read now, write later.
//!
//! The format code is ported from the C++ tool rsx (AGPL-3.0); each module names its sources.

mod guid;
mod oodle;
mod pak;
mod rpak;
mod rtech;
mod starpak;
mod txtr;
#[cfg(feature = "testing")]
pub mod testing;

use respak_core::capability::{Bitmap, Container, Metadata};
use respak_core::{Plugin, Registry};

pub use guid::string_to_guid;
pub use pak::{AssetEntry, PagePtr, PakHeader, Tables};
pub use rpak::{RpakAsset, RpakData, RpakRead, MEM};
pub use starpak::{Starpak, StarpakData, StarpakEntry, StarpakRead};
pub use txtr::{pixel_format, TxtrAsset, TxtrHeader, TxtrRead};

/// Registers the Respawn formats.
#[derive(Debug, Default)]
pub struct RespawnPlugin;

impl RespawnPlugin {
    /// A plugin with default settings.
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for RespawnPlugin {
    fn id(&self) -> &str {
        "respawn"
    }

    fn register(&self, r: &mut Registry) {
        r.format("rpak").label("Respawn pak").from("file", RpakRead).extensions(["rpak"]).provides::<dyn Container>();
        r.format("starpak")
            .label("Respawn starpak")
            .from("file", StarpakRead)
            .extensions(["starpak"])
            .provides::<dyn Container>();
        // Handed out by the rpak volume, never read from a file.
        r.format("rpak.asset").label("Respawn pak asset").provides::<dyn Metadata>();
        r.format("txtr")
            .label("Respawn texture")
            .from("rpak.asset", TxtrRead)
            .extensions(["txtr"])
            .provides::<dyn Bitmap>()
            .provides::<dyn Metadata>();
    }
}
