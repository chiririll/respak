//! Building a [`Cx`] without an [`Engine`](crate::Engine), for tests of plugins and handlers.

use std::sync::Arc;

use opendal::Operator;

use crate::event::Events;
use crate::mediator::{Cx, Mediator};
use crate::middleware::Middleware;
use crate::plugin::Registry;
use crate::CancellationToken;

/// A context over `registry` reading from `input` and writing to `output`, with defaults for the rest.
pub fn cx(registry: Registry, input: Operator, output: Operator) -> Cx {
    CxBuilder::new(registry, input, output).build()
}

/// A mediator over `registry` alone, for planning and detection tests.
pub fn mediator(registry: Registry) -> Mediator {
    Mediator::new(registry)
}

/// Builds a [`Cx`] with the settings an `Engine` would give it.
pub struct CxBuilder {
    mediator: Mediator,
    input: Operator,
    output: Operator,
    cache: Option<Operator>,
    cancel: Option<CancellationToken>,
    limits: Option<(usize, usize)>,
}

impl CxBuilder {
    /// A builder over a checked registry.
    pub fn new(registry: Registry, input: Operator, output: Operator) -> Self {
        Self {
            mediator: Mediator::new(registry),
            input,
            output,
            cache: None,
            cancel: None,
            limits: None,
        }
    }

    /// Uses `events` as the event channel.
    pub fn events(mut self, events: Events) -> Self {
        self.mediator = self.mediator.with_events(events);
        self
    }

    /// Adds host middleware, run after the built-ins.
    pub fn middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        self.mediator = self.mediator.with_middleware(middleware);
        self
    }

    /// Uses `op` as the plugin cache.
    pub fn cache(mut self, op: Operator) -> Self {
        self.cache = Some(op);
        self
    }

    /// Uses `cancel` as the call's token.
    pub fn cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Bounds `Cx::blocking` and `Cx::io` to the given numbers of concurrent tasks.
    pub fn limits(mut self, blocking: usize, io: usize) -> Self {
        self.limits = Some((blocking, io));
        self
    }

    /// The context.
    pub fn build(self) -> Cx {
        let mut cx = Cx::new(self.mediator, self.input, self.output);
        if let Some(op) = self.cache {
            cx = cx.with_cache(op);
        }
        if let Some(cancel) = self.cancel {
            cx = cx.with_cancel(cancel);
        }
        if let Some((blocking, io)) = self.limits {
            cx = cx.with_limits(blocking, io);
        }
        cx
    }
}
