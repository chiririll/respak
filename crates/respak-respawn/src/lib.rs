//! respak-respawn: Respawn formats. Read now, write later.
//!
//! The format code is ported from the C++ tool rsx (AGPL-3.0); each module names its sources.

mod anim_codec;
mod arig;
mod aseq;
mod asqd;
mod dxbc;
mod guid;
mod locate;
mod matl;
mod matl_json;
mod oodle;
mod pak;
mod patch;
mod rmdl;
mod rmdl_json;
mod rpak;
mod rtech;
mod shader;
mod snowflake;
mod starpak;
mod txtr;
mod uiia;
mod uiia_json;
#[cfg(feature = "testing")]
pub mod testing;

use std::sync::Arc;

use respak_core::capability::{Animation, Bitmap, Container, Mesh, Metadata, Skeleton};
use respak_core::{Plugin, Registry};

pub use arig::{studio_variant, ArigAsset, ArigHeader, ArigRead, StudioSelect};
pub use aseq::{
    variant as aseq_variant, AnimDesc, AseqAsset, AseqHeader, AseqRead, FrameMovement, RigSelect, SectionData, SeqDesc,
    Weights,
};
pub use asqd::{AsqdAsset, AsqdHeader, AsqdRead};
pub use dxbc::texture_slots;
pub use guid::string_to_guid;
pub use locate::{GuidRef, RpakLocate};
pub use matl::{MatlAsset, MatlHeader, MatlRead};
pub use matl_json::{MatlJsonEncode, MatlJsonOptions, TextureTarget};
pub use rmdl::{variant as rmdl_variant, RmdlAsset, RmdlHeader, RmdlRead, Studio, StudioLayout};
pub use rmdl_json::{RmdlJsonEncode, RmdlJsonOptions};
pub use pak::{AssetEntry, PagePtr, PakHeader, PatchBlock, Tables};
pub use rpak::{RpakAsset, RpakData, RpakRead, MEM};
pub use starpak::{Starpak, StarpakData, StarpakEntry, StarpakRead};
pub use txtr::{pixel_format, TxtrAsset, TxtrHeader, TxtrRead};
pub use uiia::{Quality, Resolution, UiiaAsset, UiiaHeader, UiiaRead};
pub use uiia_json::{UiiaJsonEncode, UiiaJsonOptions};

/// Registers the Respawn formats.
#[derive(Debug, Default)]
pub struct RespawnPlugin {
    search: Arc<Vec<String>>,
    ignore_patches: bool,
    rig: RigSelect,
    studio: StudioSelect,
}

impl RespawnPlugin {
    /// A plugin with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Paks (globs on the input, e.g. `paks/Win64/*.rpak`) to search for assets that a pak
    /// refers to but does not contain, after the pak itself and its directory's `common.rpak`.
    /// Never scanned unless a lookup needs it.
    pub fn search<I, S>(mut self, globs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.search = Arc::new(globs.into_iter().map(Into::into).collect());
        self
    }

    /// Ignores rpak patches: `patch_master.rpak` is not consulted, and a patch file (`common(01).rpak`)
    /// is refused as `UnsupportedVersion` instead of being completed from the files below it. The
    /// default is to apply patches.
    pub fn ignore_patches(mut self, ignore: bool) -> Self {
        self.ignore_patches = ignore;
        self
    }

    /// Which skeleton animation sequences are bound to: `"auto"` (the default: the `arig` or
    /// `mdl_` that lists the sequence) or the guid (`0x...`) or path of one.
    pub fn rig(mut self, rig: impl Into<RigSelect>) -> Self {
        self.rig = rig.into();
        self
    }

    /// How the studiohdr layout of an `arig` is found: `"auto"` (a `mdl_` beside it, else a
    /// heuristic) or a layout name (`"v16"`, `"v17"`, `"v19_2"`).
    pub fn studio(mut self, studio: impl Into<StudioSelect>) -> Self {
        self.studio = studio.into();
        self
    }
}

impl Plugin for RespawnPlugin {
    fn id(&self) -> &str {
        "respawn"
    }

    /// Bump when what the plugin keeps in the cache (the guid -> pak index) changes shape.
    fn cache_version(&self) -> u32 {
        1
    }

    fn register(&self, r: &mut Registry) {
        r.format("rpak").label("Respawn pak").from("file", RpakRead::new(!self.ignore_patches)).extensions(["rpak"]).provides::<dyn Container>();
        r.format("starpak")
            .label("Respawn starpak")
            .from("file", StarpakRead)
            .extensions(["starpak"])
            .provides::<dyn Container>();
        // Handed out by the rpak volume, never read from a file.
        r.format("rpak.asset")
            .label("Respawn pak asset")
            .from("rpak.guid", RpakLocate::new(self.search.clone()))
            .provides::<dyn Metadata>();
        // A reference to an asset by guid, made by an asset of a pak; located by `RpakLocate`.
        r.format("rpak.guid").label("Respawn asset reference");
        r.format("matl")
            .label("Respawn material")
            .from("rpak.asset", MatlRead)
            .extensions(["matl"])
            .provides::<dyn Metadata>()
            .to("matl.json", MatlJsonEncode);
        r.format("matl.json").label("Material (JSON)").extensions(["json"]);
        r.format("txtr")
            .label("Respawn texture")
            .from("rpak.asset", TxtrRead)
            .extensions(["txtr"])
            .provides::<dyn Bitmap>()
            .provides::<dyn Metadata>();
        r.format("rmdl")
            .label("Respawn model")
            .from("rpak.asset", RmdlRead)
            .extensions(["mdl_"])
            .provides::<dyn Mesh>()
            .provides::<dyn Skeleton>()
            .provides::<dyn Metadata>()
            .to("rmdl.json", RmdlJsonEncode);
        r.format("rmdl.json").label("Model (JSON)").extensions(["json"]);
        r.format("uiia")
            .label("Respawn UI image")
            .from("rpak.asset", UiiaRead)
            .extensions(["uiia"])
            .provides::<dyn Bitmap>()
            .provides::<dyn Metadata>()
            .to("uiia.json", UiiaJsonEncode);
        r.format("uiia.json").label("UI image (JSON)").extensions(["json"]);
        r.format("aseq")
            .label("Respawn animation")
            .from("rpak.asset", AseqRead::new(self.rig.clone()))
            .extensions(["aseq"])
            .provides::<dyn Animation>()
            .provides::<dyn Metadata>();
        r.format("asqd")
            .label("Respawn animation data")
            .from("rpak.asset", AsqdRead)
            .extensions(["asqd"])
            .provides::<dyn Metadata>();
        r.format("arig")
            .label("Respawn animation rig")
            .from("rpak.asset", ArigRead::new(self.studio.clone()))
            .extensions(["arig"])
            .provides::<dyn Skeleton>()
            .provides::<dyn Metadata>();
    }
}
