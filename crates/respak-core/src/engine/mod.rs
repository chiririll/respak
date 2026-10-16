//! The public entry point (PROJECT.md §8). Slice 05: single-path `convert` only.

mod convert;
mod inspect;
mod request;

use std::sync::Arc;

use opendal::Operator;

use crate::error::{Error, ErrorKind, Result};
use crate::event::{Event, Events, DEFAULT_EVENT_CAPACITY};
use crate::graph::FormatGraph;
use crate::mediator::{Cx, Mediator};
use crate::middleware::Middleware;
use crate::plugin::{Plugin, Registry};
use crate::value::Value;
use crate::CancellationToken;

pub use request::{
    ConvertReport, ConvertRequest, FromSpec, LinkedValue, OutputReport, OutputRequest,
    OutputResult, SourceSpec, TargetSpec,
};

pub(crate) use inspect::list_volume;

/// Entries read at once by `list` with names.
const NAME_CONCURRENCY: usize = 32;

/// Entries read at once by each thread of `inspect`.
const WORKER_CONCURRENCY: usize = 8;

/// `inspect` reports progress every this many entries.
const INSPECT_PROGRESS: usize = 500;

/// Builds an [`Engine`].
#[derive(Default)]
pub struct EngineBuilder {
    input: Option<Operator>,
    output: Option<Operator>,
    cache: Option<Operator>,
    plugins: Vec<Box<dyn Plugin>>,
    max_concurrency: Option<usize>,
    middleware: Vec<Arc<dyn Middleware>>,
    blocking_limit: Option<usize>,
    io_limit: Option<usize>,
    event_capacity: Option<usize>,
}

impl EngineBuilder {
    /// How many outputs run at once (default 4, at least 1).
    pub fn max_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = Some(n.max(1));
        self
    }

    /// Adds host middleware, run after the built-ins in the order added.
    pub fn middleware(mut self, m: impl Middleware + 'static) -> Self {
        self.middleware.push(Arc::new(m));
        self
    }

    /// How many `Cx::blocking` tasks run at once (default: unlimited).
    pub fn blocking_limit(mut self, n: usize) -> Self {
        self.blocking_limit = Some(n);
        self
    }

    /// How many `Cx::io` futures run at once (default: unlimited).
    pub fn io_limit(mut self, n: usize) -> Self {
        self.io_limit = Some(n);
        self
    }

    /// Unread events a subscriber may fall behind by before it misses some (default 1024).
    pub fn event_capacity(mut self, n: usize) -> Self {
        self.event_capacity = Some(n);
        self
    }

    /// Where inputs are read from (required).
    pub fn input(mut self, op: Operator) -> Self {
        self.input = Some(op);
        self
    }
    /// Where outputs are written (required).
    pub fn output(mut self, op: Operator) -> Self {
        self.output = Some(op);
        self
    }
    /// Where plugins keep cached data (optional).
    pub fn cache(mut self, op: Operator) -> Self {
        self.cache = Some(op);
        self
    }
    /// Adds a plugin.
    pub fn plugin(mut self, p: impl Plugin + 'static) -> Self {
        self.plugins.push(Box::new(p));
        self
    }
    /// Fails on a missing operator or a format defined twice.
    pub fn build(self) -> Result<Engine> {
        let missing = |what: &str| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: format!("engine has no {what} operator"),
            })
        };
        let input = self.input.ok_or_else(|| missing("input"))?;
        let output = self.output.ok_or_else(|| missing("output"))?;
        let mut registry = Registry::new();
        for p in &self.plugins {
            registry.add(p.as_ref());
        }
        registry.check()?;
        Ok(Engine {
            mediator: Mediator::new(registry)
                .with_events(Events::new(
                    self.event_capacity.unwrap_or(DEFAULT_EVENT_CAPACITY),
                ))
                .with_middleware(self.middleware),
            input,
            output,
            cache: self.cache,
            max_concurrency: self.max_concurrency.unwrap_or(4),
            limits: (self.blocking_limit, self.io_limit),
        })
    }
}

/// Converts requests using the registered plugins.
pub struct Engine {
    mediator: Mediator,
    input: Operator,
    output: Operator,
    cache: Option<Operator>,
    max_concurrency: usize,
    limits: (Option<usize>, Option<usize>),
}

impl Engine {
    /// Starts a builder.
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    /// The format graph, for `respak formats`.
    pub fn formats(&self) -> FormatGraph {
        self.mediator.formats()
    }

    /// A receiver for the events of every call from now on.
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.mediator.subscribe()
    }

    /// The paths `path_or_glob` stands for on the input, as `open` expands them.
    pub async fn expand(
        &self,
        path_or_glob: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>> {
        self.mediator.expand(path_or_glob, &self.cx(cancel)).await
    }

    /// Passes the bytes of the file at `path` to `sink` in chunks, in order. `NotFound` if there is
    /// no such file; a value with no bytes is an `InvalidRequest`.
    pub async fn read_file(
        &self,
        path: &str,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(&[u8]),
    ) -> Result<()> {
        let cx = self.cx(cancel);
        let value = self.mediator.open(path, &cx).await?;
        let blob = value.as_blob().ok_or_else(|| {
            Error::new(ErrorKind::InvalidRequest {
                message: format!("{path} has no bytes to read ({})", value.format()),
            })
        })?;
        let mut pos = 0u64;
        loop {
            cx.check_cancelled()?;
            let chunk = blob.read(pos..pos + crate::capability::CHUNK, &cx).await?;
            if chunk.is_empty() {
                return Ok(());
            }
            pos += chunk.len() as u64;
            sink(&chunk);
        }
    }

    fn cx(&self, cancel: &CancellationToken) -> Cx {
        let cx = Cx::new(
            self.mediator.clone(),
            self.input.clone(),
            self.output.clone(),
        )
        .with_cancel(cancel.clone());
        let cx = match &self.cache {
            Some(op) => cx.with_cache(op.clone()),
            None => cx,
        };
        match self.limits {
            (None, None) => cx,
            (blocking, io) => {
                cx.with_limits(blocking.unwrap_or(usize::MAX), io.unwrap_or(usize::MAX))
            }
        }
    }

    /// Opens a path or glob on the input: one value per match (`NoMatch` if a glob matches
    /// nothing, `NotFound` for a path that is not there).
    pub async fn open(&self, path_or_glob: &str, cancel: &CancellationToken) -> Result<Vec<Value>> {
        let cx = self.cx(cancel);
        let mut values = Vec::new();
        for path in self.mediator.expand(path_or_glob, &cx).await? {
            values.push(self.mediator.open(&path, &cx).await?);
        }
        Ok(values)
    }
}
