//! `rpak`: the Respawn pak container, and `rpak.asset`, the typed entry its volume hands out.
//!
//! # The volume
//! ```text
//! common.rpak/
//!   0x8A3F12C4D5E6F701.txtr          the asset header (a name hashed to the same GUID resolves too)
//!   0x8A3F12C4D5E6F701.txtr.stream   its entry in a mandatory starpak (resolves, not listed)
//!   0x8A3F12C4D5E6F701.txtr.opt      its entry in an optional starpak (resolves, not listed)
//! ```
//! The starpaks are looked up next to the pak by file name and opened on first use (their table
//! of contents only). A missing one is `MissingDependency`.
//! Each asset is a file named by its GUID and kind. The file's bytes are the asset header as
//! stored in the pak (pointers are not relocated until the slice that reads assets). Names are
//! not known yet, so listing shows GUIDs; a path that gives a *name* (`texture/foo.txtr`) is
//! hashed to a GUID and resolves.
//!
//! # Patches
//! A patched pak (`common(03).rpak`) stores the final tables and only part of the page data. Its
//! `.mem` is a map built from the pak's edit stream (see `patch.rs`): a read follows the map into
//! the pak itself and the lower files of the chain, which are opened, and the map built, on the
//! first read of page data. Listing never touches them. A lower file that is not there is
//! `MissingDependency`. Opening `common.rpak` next to a `patch_master.rpak` opens the newest patch
//! it names instead; opening a patch file directly opens that file.
//!
//! # Cost
//! An uncompressed pak is read with ranged reads: the tables, then only the header bytes asked
//! for. An RTech-compressed pak is a stream of output chunks decoded front to back: a read at
//! offset `X` reads and decodes the stream up to the chunk containing `X` (never past it), and the
//! decoded prefix is kept for the life of the container. Listing a pak decodes only its tables, so
//! it costs the start of the file, not all of it; reading a page near the end costs the file.

mod body;
mod chain;
mod data;
mod handler;
mod master;
mod names;
mod oodle_read;
mod rtech_read;
#[cfg(test)]
mod tests;
mod volume;

pub use data::{RpakAsset, RpakData};
pub use handler::RpakRead;

/// The flat file of all decompressed pages (not listed, but it resolves). Pointers in served
/// headers are offsets into it.
pub const MEM: &str = ".mem";
