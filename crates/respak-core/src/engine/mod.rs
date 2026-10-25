//! The public entry point (docs/design/project.md §8). Slice 05: single-path `convert` only.

mod convert;
mod inspect;
mod parallel;
mod report;
mod request;

use std::sync::Arc;

use opendal::Operator;

use crate::discover::{DiscoverOptions, EntryInfo};
use crate::error::{Error, ErrorKind, Result};
use crate::event::{Event, Events, DEFAULT_EVENT_CAPACITY};
use crate::graph::FormatGraph;
use crate::mediator::{Cx, Mediator};
use crate::middleware::Middleware;
use crate::plugin::{Plugin, Registry};
use crate::value::{Data, Value};
use crate::CancellationToken;

pub use report::{ConvertReport, OutputReport, OutputResult};
pub use request::{ConvertRequest, FromSpec, OutputRequest, SourceSpec, TargetSpec};

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
    memory_cache: Option<u64>,
}

impl EngineBuilder {
    /// Keeps finished conversions in memory, up to `bytes` by `Data::approx_size` (default: off).
    ///
    /// The cache belongs to the engine, so a later call reuses what an earlier one converted. A
    /// cached result is not refreshed when the input changes on disk.
    pub fn memory_cache(mut self, bytes: u64) -> Self {
        self.memory_cache = Some(bytes);
        self
    }

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
        let missing = |what: &str| Error::msg(format!("engine has no {what} operator"));
        let input = self.input.ok_or_else(|| missing("input"))?;
        let output = self.output.ok_or_else(|| missing("output"))?;
        let mut registry = Registry::new();
        for p in &self.plugins {
            registry.add(p.as_ref());
        }
        registry.check()?;
        let mut mediator = Mediator::new(registry)
            .with_events(Events::new(
                self.event_capacity.unwrap_or(DEFAULT_EVENT_CAPACITY),
            ))
            .with_middleware(self.middleware);
        if let Some(bytes) = self.memory_cache {
            mediator = mediator.with_memory_cache(bytes);
        }
        Ok(Engine {
            mediator,
            input,
            output,
            cache: self.cache,
            max_concurrency: self.max_concurrency.unwrap_or(4),
            limits: (self.blocking_limit, self.io_limit),
        })
    }
}

/// What the file at a path holds, as far as scanning it for names goes. See [`Engine::content`].
#[derive(Debug)]
pub enum Content {
    /// A container, with what each entry is made of.
    Container(Vec<EntryInfo>),
    /// A file of no known format: it may hold paths as text, see [`Engine::read_file`].
    Unrecognized,
    /// A file of a known format that is not a container, or one that cannot be described: no names.
    Opaque,
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

    /// Reads what the file at `path` holds: the entries of a container (as [`Engine::inspect`]), else
    /// whether it is a file of no known format. Only a failure other than "not a container" is an `Err`.
    pub async fn content(&self, path: &str, cancel: &CancellationToken) -> Result<Content> {
        match self.inspect(path, cancel).await {
            Ok(entries) => Ok(Content::Container(entries)),
            Err(error) if matches!(error.kind(), ErrorKind::NotFound { .. }) => {
                let found = self
                    .discover(path, &DiscoverOptions::default(), cancel)
                    .await;
                let unknown = found
                    .is_ok_and(|found| found.entries.first().is_some_and(|e| e.format.is_none()));
                Ok(if unknown {
                    Content::Unrecognized
                } else {
                    Content::Opaque
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Writes the bytes of `data` to `dest` on the output, in chunks, like any other output: a
    /// failure part-way leaves no file, and `OutputWritten` is published. Each call is its own
    /// request, so paths written by earlier calls are not tracked. `data` must have bytes
    /// (`InvalidRequest` otherwise). Returns whether the file was written.
    pub async fn write(
        &self,
        dest: &str,
        data: &dyn Data,
        cancel: &CancellationToken,
    ) -> Result<bool> {
        let cx = self.cx(cancel);
        cx.check_cancelled()?;
        let blob = data.as_blob().ok_or_else(|| {
            Error::new(ErrorKind::InvalidRequest {
                message: format!("{dest} has no bytes to write ({})", data.format()),
            })
        })?;
        cx.write_blob(dest, data.key(), blob).await
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

    /// Opens `path` and reads it as the format it is detected to be, so the value has the
    /// capabilities of that format (an asset a container hands out is read as its kind). `path`
    /// is a single path, not a glob. An unrecognised file is `UnknownFormat`.
    pub async fn open_read(&self, path: &str, cancel: &CancellationToken) -> Result<Value> {
        let cx = self.cx(cancel);
        let value = self.mediator.open(path, &cx).await?;
        self.mediator.identify(value, &cx).await
    }

    /// Opens a path or glob on the input: one value per match (`NoMatch` if a glob matches
    /// nothing, `NotFound` for a path that is not there).
    pub async fn open_matches(
        &self,
        path_or_glob: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<Value>> {
        let cx = self.cx(cancel);
        let mut values = Vec::new();
        for path in self.mediator.expand(path_or_glob, &cx).await? {
            values.push(self.mediator.open(&path, &cx).await?);
        }
        Ok(values)
    }
}
