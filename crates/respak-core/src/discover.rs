//! Discovery: what is this, and what can it become (docs/design/project.md §8). Slice 06: files only.

use serde::Serialize;

use crate::capability::Dependency;
use crate::graph::TargetInfo;

/// One line of [`Engine::list`](crate::Engine::list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedEntry {
    /// Path inside the container (for an rpak asset: `0x<GUID>.<kind>`).
    pub path: String,
    /// Readable path of the same entry (`texture/foo.txtr`), when the container knows one.
    pub name: Option<String>,
}

/// One entry of a container, read as far as its format is known: [`Engine::inspect`](crate::Engine::inspect).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryInfo {
    /// Path inside the container (for an rpak asset: `0x<GUID>.<kind>`).
    pub path: String,
    /// The name the asset stores, as stored (without the extension `list` adds).
    pub name: Option<String>,
    /// The asset's kind (e.g. `txtr`), if known.
    pub kind: Option<String>,
    /// The asset's guid, if it has one.
    pub guid: Option<u64>,
    /// Assets it refers to.
    pub dependencies: Vec<Dependency>,
    /// Strings in its data that may be names of other assets (see `NameSource`).
    pub name_candidates: Vec<String>,
}

/// Options for [`Engine::discover`](crate::Engine::discover).
#[derive(Debug, Clone, Default)]
pub struct DiscoverOptions {
    /// How many container levels to open: 0 describes the item itself, 1 lists a container's items.
    pub depth: u32,
    /// Read every entry to find its name where its metadata does not carry one without
    /// decoding (an rpak asset's name is in its header). Slow; an unreadable entry keeps no name.
    pub names: bool,
}

/// The result of discovering one path.
#[derive(Debug, Clone, Serialize)]
pub struct Discovery {
    /// The path that was discovered.
    pub path: String,
    /// One entry per recognised item. For a plain file, exactly one.
    pub entries: Vec<DiscoveryEntry>,
}

/// One item: its detected format and where it can go.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryEntry {
    /// Goes straight into a request's `from`.
    pub path: String,
    /// The winning format, `None` if nothing recognised the item.
    pub format: Option<String>,
    /// Detected version including the layout variant, e.g. `10` or `23.2`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The winner's score, 0 if unrecognised.
    pub score: u32,
    /// The other formats that scored, best first. An equal score here means the winner was
    /// not clear-cut (convert would say `AmbiguousFormat`).
    pub alternatives: Vec<Alternative>,
    /// Name from the item's metadata, if it has one without decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Kind from the item's metadata (e.g. an asset kind), if known without decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Formats it can be converted to, found without decoding.
    pub targets: Vec<TargetInfo>,
}

/// A runner-up in detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alternative {
    /// Format name.
    pub format: String,
    /// Detected version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Its score.
    pub score: u32,
}
