//! respak-respawn: Respawn formats. Read now, write later.
//!
//! The format code is ported from the C++ tool rsx (AGPL-3.0); each module names its sources.

mod guid;
mod pak;
mod rpak;
mod rtech;

use respak_core::capability::{Container, Metadata};
use respak_core::{Plugin, Registry};

pub use guid::string_to_guid;
pub use pak::{AssetEntry, PagePtr, PakHeader, Tables};
pub use rpak::{RpakAsset, RpakData, RpakRead};

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
        // Handed out by the rpak volume, never read from a file.
        r.format("rpak.asset").label("Respawn pak asset").provides::<dyn Metadata>();
    }
}
