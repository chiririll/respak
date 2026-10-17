//! The mediator: detection, path search and conversion (docs/design/project.md §5.5). No middleware yet.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::error::Result;
use crate::event::{Event, Events, DEFAULT_EVENT_CAPACITY};
use crate::graph::{self, FormatGraph, Path, TargetInfo};
use crate::middleware::Middleware;
use crate::plugin::Registry;
use crate::value::FormatId;

mod cx;
mod expand;
mod open;
mod pipeline;

pub use cx::Cx;
pub use open::Mounts;

/// What to convert to: a format and its options.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Target format.
    pub format: FormatId,
    /// Target options, `Null` if none. Unused so far.
    pub options: serde_json::Value,
    /// Formats the path must pass through, in order. Empty = cheapest path.
    pub via: Vec<String>,
}

/// One format that scored for an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The format, with its detected version.
    pub format: FormatId,
    /// Its score.
    pub score: u32,
}

/// Detects formats and converts values along the cheapest path.
#[derive(Clone)]
pub struct Mediator {
    registry: Arc<Registry>,
    events: Events,
    middleware: Arc<[Arc<dyn Middleware>]>,
}

impl Mediator {
    /// A mediator over a checked registry.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry: Arc::new(registry),
            events: Events::new(DEFAULT_EVENT_CAPACITY),
            middleware: crate::middleware::builtins().into(),
        }
    }

    /// Replaces the event channel (the builder's `event_capacity`).
    pub fn with_events(mut self, events: Events) -> Self {
        self.events = events;
        self
    }

    /// Sets host middleware, run after the built-ins.
    pub fn with_middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        let mut chain = crate::middleware::builtins();
        chain.extend(middleware);
        self.middleware = chain.into();
        self
    }

    /// The `cache_version` the plugin `id` declared (0 if unknown).
    pub(crate) fn cache_version(&self, id: &str) -> u32 {
        self.registry.cache_versions.get(id).copied().unwrap_or(0)
    }

    /// Sends `event` to every subscriber.
    pub fn publish(&self, event: Event) {
        self.events.publish(event)
    }

    /// A receiver for events published from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// The cheapest path from `from` to `to` that visits `via` in order.
    ///
    /// Candidates are ordered by: total cost, then fewer capability steps (direct edges first),
    /// then fewer hops, then registration order. `via` must be a subsequence of the path's
    /// formats (both ends count). When more than one complete path existed, the chosen one is
    /// published as `Event::Warning`. `NoPath` if there is none.
    pub fn plan(&self, from: &FormatId, to: &str, via: &[String]) -> Result<Path> {
        Ok(self.plan_inner(from, to, via)?.path)
    }

    /// Formats reachable from `from` (excluding itself), found from declared capabilities
    /// without decoding anything. In registration order.
    pub fn targets_from(&self, from: &str) -> Vec<TargetInfo> {
        graph::targets_from(&self.registry, from)
    }

    /// Whether values of `format` provide the [`Container`](crate::capability::Container) capability.
    pub fn provides_container(&self, format: &str) -> bool {
        let id = crate::plugin::CapId::of::<dyn crate::capability::Container>();
        self.registry
            .formats
            .iter()
            .any(|f| f.name == format && f.provides.contains(&id))
    }

    /// The whole graph as data.
    pub fn formats(&self) -> FormatGraph {
        graph::format_graph(&self.registry)
    }

    /// The extension output files of `format` get (its first registered extension).
    pub fn extension_of(&self, format: &str) -> Option<&str> {
        self.registry
            .formats
            .iter()
            .find(|f| f.name == format)
            .and_then(|f| f.extensions.first().copied())
    }
}
