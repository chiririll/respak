//! Plugins and the registry they fill (docs/design/project.md §5.2).

use std::sync::Arc;

use crate::capability::Cap;
use crate::detect::{ConvertHandler, ReadHandler};
use crate::error::{Error, ErrorKind, Result};

/// A set of formats and edges, contributed by one crate.
pub trait Plugin: Send + Sync {
    /// Unique plugin id, e.g. `formats`.
    fn id(&self) -> &str;
    /// Version of the data this plugin keeps in the plugin cache; bump it when the cached schema
    /// changes (the cache is namespaced by plugin id and this number).
    fn cache_version(&self) -> u32 {
        0
    }
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
    /// Registration order across all edges, for tie-breaks.
    pub order: usize,
}

pub(crate) struct DirectEdge {
    pub from: &'static str,
    pub to: &'static str,
    pub handler: Arc<dyn ConvertHandler>,
    pub cost: u32,
    pub order: usize,
}

pub(crate) struct FormatDef {
    pub name: &'static str,
    pub label: Option<&'static str>,
    pub plugin: String,
    pub extensions: Vec<&'static str>,
    pub provides: Vec<CapId>,
    pub read_edges: Vec<ReadEdge>,
    pub cap_edges: Vec<CapEdge>,
}

impl FormatDef {
    pub fn label(&self) -> &str {
        self.label.unwrap_or(self.name)
    }
}

/// The edge whose cost `cost()` sets.
enum LastEdge {
    Cap { format: usize },
    Direct { index: usize },
}

/// Collects what plugins register. Registration is grouped per format.
pub struct Registry {
    pub(crate) formats: Vec<FormatDef>,
    pub(crate) direct_edges: Vec<DirectEdge>,
    next_order: usize,
    last_edge: Option<LastEdge>,
    current_plugin: String,
    pub(crate) cache_versions: std::collections::HashMap<String, u32>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self {
            formats: Vec::new(),
            direct_edges: Vec::new(),
            next_order: 0,
            last_edge: None,
            current_plugin: String::new(),
            cache_versions: Default::default(),
        }
    }

    /// Runs `plugin.register`, tagging everything it adds with the plugin's id.
    pub fn add(&mut self, plugin: &dyn Plugin) {
        self.current_plugin = plugin.id().to_owned();
        self.cache_versions
            .insert(self.current_plugin.clone(), plugin.cache_version());
        plugin.register(self);
    }

    /// Defines format `name`; the builder chains its edges.
    /// Defining one name twice is reported by [`Registry::check`].
    pub fn format(&mut self, name: &'static str) -> FormatBuilder<'_> {
        self.formats.push(FormatDef {
            name,
            label: None,
            plugin: self.current_plugin.clone(),
            extensions: Vec::new(),
            provides: Vec::new(),
            read_edges: Vec::new(),
            cap_edges: Vec::new(),
        });
        self.last_edge = None;
        FormatBuilder {
            index: self.formats.len() - 1,
            registry: self,
        }
    }

    /// Fails if a format name was defined twice, or a direct edge leads to an undefined format.
    pub fn check(&self) -> Result<()> {
        let custom = |message: String| {
            Error::new(ErrorKind::Custom {
                source: None,
                message,
            })
        };
        for (i, f) in self.formats.iter().enumerate() {
            if let Some(first) = self.formats[..i].iter().find(|o| o.name == f.name) {
                return Err(custom(format!(
                    "format `{}` is defined twice (plugins `{}` and `{}`)",
                    f.name, first.plugin, f.plugin
                )));
            }
        }
        for d in &self.direct_edges {
            if !self.formats.iter().any(|f| f.name == d.to) {
                return Err(custom(format!(
                    "edge `{}` -> `{}` leads to an undefined format",
                    d.from, d.to
                )));
            }
        }
        Ok(())
    }

    fn take_order(&mut self) -> usize {
        self.next_order += 1;
        self.next_order - 1
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
        self.def().read_edges.push(ReadEdge {
            from: src,
            handler: Arc::new(handler),
        });
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
        let order = self.registry.take_order();
        self.def().cap_edges.push(CapEdge {
            cap: CapId::of::<C>(),
            handler: Arc::new(handler),
            cost: 1,
            order,
        });
        self.registry.last_edge = Some(LastEdge::Cap { format: self.index });
        self
    }

    /// Direct convert edge: this format's values → format `dst`, converted by `handler`.
    /// `dst` must be defined by the time the registry is checked.
    pub fn to(mut self, dst: &'static str, handler: impl ConvertHandler + 'static) -> Self {
        let order = self.registry.take_order();
        let from = self.def().name;
        self.registry.direct_edges.push(DirectEdge {
            from,
            to: dst,
            handler: Arc::new(handler),
            cost: 1,
            order,
        });
        self.registry.last_edge = Some(LastEdge::Direct {
            index: self.registry.direct_edges.len() - 1,
        });
        self
    }

    /// Human-readable label shown by `formats` and `discover` (default: the name).
    pub fn label(mut self, label: &'static str) -> Self {
        self.def().label = Some(label);
        self
    }

    /// Sets the cost (default 1) of the edge added last (direct or capability).
    ///
    /// # Panics
    /// If no edge has been added to this format yet (a programming error in the plugin).
    pub fn cost(self, cost: u32) -> Self {
        match self
            .registry
            .last_edge
            .as_ref()
            .expect("cost() must follow from_cap() or to()")
        {
            LastEdge::Cap { format } => {
                self.registry.formats[*format]
                    .cap_edges
                    .last_mut()
                    .expect("edge exists")
                    .cost = cost
            }
            LastEdge::Direct { index } => self.registry.direct_edges[*index].cost = cost,
        }
        self
    }
}
