//! Plugins and the registry they fill (PROJECT.md §5.2).

use std::sync::Arc;

use crate::capability::Cap;
use crate::detect::{ConvertHandler, ReadHandler};
use crate::error::{Error, ErrorKind, Result};

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
        Self { name: C::NAME }
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
        Self { formats: Vec::new(), current_plugin: String::new() }
    }

    /// Runs `plugin.register`, tagging everything it adds with the plugin's id.
    pub fn add(&mut self, plugin: &dyn Plugin) {
        self.current_plugin = plugin.id().to_owned();
        plugin.register(self);
    }

    /// Defines format `name`; the builder chains its edges.
    /// Defining one name twice is reported by [`Registry::check`].
    pub fn format(&mut self, name: &'static str) -> FormatBuilder<'_> {
        self.formats.push(FormatDef {
            name,
            plugin: self.current_plugin.clone(),
            extensions: Vec::new(),
            provides: Vec::new(),
            read_edges: Vec::new(),
            cap_edges: Vec::new(),
        });
        FormatBuilder { index: self.formats.len() - 1, registry: self }
    }

    /// Fails if a format name was defined twice.
    pub fn check(&self) -> Result<()> {
        for (i, f) in self.formats.iter().enumerate() {
            if let Some(first) = self.formats[..i].iter().find(|o| o.name == f.name) {
                return Err(Error::new(ErrorKind::Custom {
                    source: None,
                    message: format!(
                        "format `{}` is defined twice (plugins `{}` and `{}`)",
                        f.name, first.plugin, f.plugin
                    ),
                }));
            }
        }
        Ok(())
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
    fn def(&mut self) -> &mut FormatDef {
        &mut self.registry.formats[self.index]
    }

    /// Read edge: `src` (e.g. `"file"`) → this format, scored and read by `handler`.
    pub fn from(mut self, src: &'static str, handler: impl ReadHandler + 'static) -> Self {
        self.def().read_edges.push(ReadEdge { from: src, handler: Arc::new(handler) });
        self
    }

    /// Extensions (lower case, no dot) that make this format's read edges candidates first.
    /// The first one is used for output paths.
    pub fn extensions<const N: usize>(mut self, exts: [&'static str; N]) -> Self {
        self.def().extensions.extend(exts);
        self
    }

    /// Declares that values of this format implement capability `C`.
    pub fn provides<C: Cap + ?Sized>(mut self) -> Self {
        self.def().provides.push(CapId::of::<C>());
        self
    }

    /// Capability edge: any value providing `C` → this format.
    pub fn from_cap<C: Cap + ?Sized>(mut self, handler: impl ConvertHandler + 'static) -> Self {
        self.def().cap_edges.push(CapEdge { cap: CapId::of::<C>(), handler: Arc::new(handler), cost: 1 });
        self
    }

    /// Sets the cost (default 1) of the edge added last.
    ///
    /// # Panics
    /// If no capability edge has been added yet (a programming error in the plugin).
    pub fn cost(mut self, cost: u32) -> Self {
        self.def().cap_edges.last_mut().expect("cost() must follow from_cap()").cost = cost;
        self
    }
}
