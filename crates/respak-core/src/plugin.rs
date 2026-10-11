//! Plugins and the registry they fill (PROJECT.md §5.2).

use std::sync::Arc;

use crate::capability::Cap;
use crate::detect::{ConvertHandler, ReadHandler};
use crate::error::Result;

/// A set of formats and edges, contributed by one crate.
pub trait Plugin: Send + Sync {
    /// Unique plugin id, e.g. `formats`.
    fn id(&self) -> &str;
    /// Declares the plugin's formats and edges.
    fn register(&self, r: &mut Registry);
}

/// Identity of a capability, by its [`Cap::NAME`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapId {
    pub(crate) name: &'static str,
}

impl CapId {
    /// The id of capability `C`.
    pub fn of<C: Cap + ?Sized>() -> Self {
        todo!()
    }
}

pub(crate) struct ReadEdge {
    pub from: &'static str,
    pub handler: Arc<dyn ReadHandler>,
}

pub(crate) struct CapEdge {
    pub cap: CapId,
    pub handler: Arc<dyn ConvertHandler>,
    pub cost: u32,
}

pub(crate) struct FormatDef {
    pub name: &'static str,
    pub plugin: String,
    pub extensions: Vec<&'static str>,
    pub provides: Vec<CapId>,
    pub read_edges: Vec<ReadEdge>,
    pub cap_edges: Vec<CapEdge>,
}

/// Collects what plugins register. Registration is grouped per format.
pub struct Registry {
    pub(crate) formats: Vec<FormatDef>,
    current_plugin: String,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        todo!()
    }

    /// Runs `plugin.register`, tagging everything it adds with the plugin's id.
    pub fn add(&mut self, plugin: &dyn Plugin) {
        todo!()
    }

    /// Defines format `name` (or adds edges to it); the builder chains its edges.
    /// Defining one name twice is reported by [`Registry::check`].
    pub fn format(&mut self, name: &'static str) -> FormatBuilder<'_> {
        todo!()
    }

    /// Fails if a format name was defined twice.
    pub fn check(&self) -> Result<()> {
        todo!()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Chained declarations of one format.
pub struct FormatBuilder<'r> {
    registry: &'r mut Registry,
    index: usize,
}

impl FormatBuilder<'_> {
    /// Read edge: `src` (e.g. `"file"`) → this format, scored and read by `handler`.
    pub fn from(self, src: &'static str, handler: impl ReadHandler + 'static) -> Self {
        todo!()
    }

    /// Extensions (lower case, no dot) that make this format's read edges candidates first.
    /// The first one is used for output paths.
    pub fn extensions<const N: usize>(self, exts: [&'static str; N]) -> Self {
        todo!()
    }

    /// Declares that values of this format implement capability `C`.
    pub fn provides<C: Cap + ?Sized>(self) -> Self {
        todo!()
    }

    /// Capability edge: any value providing `C` → this format.
    pub fn from_cap<C: Cap + ?Sized>(self, handler: impl ConvertHandler + 'static) -> Self {
        todo!()
    }

    /// Sets the cost (default 1) of the edge added last.
    pub fn cost(self, cost: u32) -> Self {
        todo!()
    }
}
