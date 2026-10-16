//! The mediator: detection, path search and conversion (PROJECT.md §5.5). No middleware yet.

use std::sync::Arc;

use opendal::Operator;
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::event::{Event, Events, DEFAULT_EVENT_CAPACITY};
use crate::middleware::{Convert, Middleware, Next};

use crate::detect::{Detected, Probe};
use crate::error::{Error, ErrorKind, Result};
use crate::graph::{self, FormatGraph, Path, TargetInfo};
use crate::plugin::{FormatDef, ReadEdge, Registry};
use crate::value::{FileData, FormatId, Value, ValueKey};

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

type Candidate<'a> = (&'a FormatDef, &'a ReadEdge);
type Scored<'a> = (&'a FormatDef, &'a ReadEdge, Detected);

/// The result of scoring: `primary` is what `identify` chooses from (extension candidates if
/// any of them matched, otherwise every read edge); `rest` are the hits of the other phase.
struct Scan<'a> {
    primary: Vec<Scored<'a>>,
    rest: Vec<Scored<'a>>,
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

    fn candidates(&self, from: &str) -> Vec<Candidate<'_>> {
        self.registry
            .formats
            .iter()
            .flat_map(|f| {
                f.read_edges
                    .iter()
                    .filter(|e| e.from == from)
                    .map(move |e| (f, e))
            })
            .collect()
    }

    /// Scores read edges. With `full` false the second phase is skipped when the first
    /// matched (what `identify` needs); with `full` true both phases are scored.
    async fn scan<'a>(&'a self, source: &Value, cx: &Cx, full: bool) -> Result<Scan<'a>> {
        let candidates = self.candidates(source.format().name());
        let header_len = candidates
            .iter()
            .map(|(_, e)| e.handler.header_len())
            .max()
            .unwrap_or(0);
        let (header, len) = probe_data(source, header_len).await?;
        let extension = extension_of_key(source.key());
        let probe = Probe {
            source,
            extension: extension.as_deref(),
            len,
            header: &header,
        };

        let (by_ext, others): (Vec<_>, Vec<_>) = candidates.into_iter().partition(|(f, _)| {
            extension
                .as_deref()
                .is_some_and(|x| f.extensions.contains(&x))
        });

        let mut first = score_all(&by_ext, &probe, cx).await?;
        if first.is_empty() {
            let primary = score_all(&others, &probe, cx).await?;
            return Ok(Scan {
                primary: sorted(primary),
                rest: Vec::new(),
            });
        }
        let rest = if full {
            score_all(&others, &probe, cx).await?
        } else {
            Vec::new()
        };
        first = sorted(first);
        Ok(Scan {
            primary: first,
            rest: sorted(rest),
        })
    }

    /// Scores read edges from `source`'s format and applies the winner.
    ///
    /// Extension candidates are scored first; if none matches, all read edges are. Highest
    /// score wins; a tie is `AmbiguousFormat`, no match `UnknownFormat`.
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value> {
        let scan = self.scan(&source, cx, false).await?;
        let top = scan
            .primary
            .first()
            .map(|h| h.2.score)
            .ok_or_else(|| Error::new(ErrorKind::UnknownFormat))?;
        let mut winners: Vec<_> = scan
            .primary
            .into_iter()
            .filter(|h| h.2.score == top)
            .collect();
        if winners.len() > 1 {
            return Err(Error::new(ErrorKind::AmbiguousFormat {
                candidates: winners.iter().map(|(f, _, _)| f.name.to_owned()).collect(),
            }));
        }
        let (_, edge, detected) = winners.remove(0);
        edge.handler.read(source, &detected, cx).await
    }

    /// Reads `source` as `format` without scoring the alternatives (a request's `{path, format}`
    /// override, for assets that are not in a container). The edge's own score function still
    /// runs, to fill in the layout variant; a version in `format` overrides the detected one.
    /// `NoPath` if there is no read edge from the source's format to `format`.
    pub async fn identify_as(&self, source: Value, format: &FormatId, cx: &Cx) -> Result<Value> {
        let (_, edge) = self
            .candidates(source.format().name())
            .into_iter()
            .find(|(f, _)| f.name == format.name())
            .ok_or_else(|| {
                Error::new(ErrorKind::NoPath {
                    from: source.format().to_string(),
                    to: format.to_string(),
                })
            })?;
        let (header, len) = probe_data(&source, edge.handler.header_len()).await?;
        let extension = extension_of_key(source.key());
        let probe = Probe {
            source: &source,
            extension: extension.as_deref(),
            len,
            header: &header,
        };
        let mut detected = edge.handler.score(&probe, cx).await?.unwrap_or(Detected {
            score: 0,
            version: None,
        });
        if let Some(v) = format.version() {
            detected.version = Some(v);
        }
        edge.handler.read(source, &detected, cx).await
    }

    /// Scores read edges like [`identify`](Self::identify) but applies none. The winner comes
    /// first, then the other hits of the same phase by score (ties in registration order), then
    /// the hits of the other phase. Empty = unrecognised. Never fails on a tie.
    pub async fn detect(&self, source: &Value, cx: &Cx) -> Result<Vec<Hit>> {
        let scan = self.scan(source, cx, true).await?;
        Ok(scan
            .primary
            .into_iter()
            .chain(scan.rest)
            .map(|(f, _, d)| {
                let mut format = FormatId::new(f.name);
                if let Some(v) = d.version {
                    format = format.with_version(v);
                }
                Hit {
                    format,
                    score: d.score,
                }
            })
            .collect())
    }

    /// Identifies `source`, then follows the cheapest path to `to.format` that visits `to.via`
    /// (`NoPath` if there is none). A source already in the target format is returned as is.
    ///
    /// Runs through the middleware chain (built-ins first, then the host's).
    pub async fn convert(&self, source: Value, to: Target, cx: &Cx) -> Result<Value> {
        let req = Convert { source, to };
        Next::new(&self.middleware, self).run(&req, cx).await
    }

    pub(crate) async fn convert_unwrapped(
        &self,
        source: Value,
        to: Target,
        cx: &Cx,
    ) -> Result<Value> {
        let target = to.format.name();
        let mut value = source;
        if value.format().name() == target {
            return Ok(value);
        }
        if !self.candidates(value.format().name()).is_empty() {
            value = self.identify(value, cx).await?;
            if value.format().name() == target {
                return Ok(value);
            }
        }
        let planned = self.plan_inner(value.format(), target, &to.via)?;
        let last = planned.handlers.len().saturating_sub(1);
        for (i, (step, handler)) in planned.path.steps.iter().zip(&planned.handlers).enumerate() {
            let next = if i == last {
                to.clone()
            } else {
                Target {
                    format: FormatId::new(step.to.clone()),
                    options: serde_json::Value::Null,
                    via: Vec::new(),
                }
            };
            value = handler.convert(value, &next, cx).await?;
        }
        Ok(value)
    }

    fn plan_inner(&self, from: &FormatId, to: &str, via: &[String]) -> Result<graph::Planned> {
        let planned = graph::plan(&self.registry, from.name(), to, via).ok_or_else(|| {
            Error::new(ErrorKind::NoPath {
                from: from.to_string(),
                to: to.to_owned(),
            })
        })?;
        if planned.alternatives > 0 {
            self.publish(Event::Warning {
                message: format!(
                    "path {} -> {to}: chose `{}` over {} other path(s)",
                    from.name(),
                    planned.path,
                    planned.alternatives
                ),
            });
        }
        Ok(planned)
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

/// Best score first; equal scores keep registration order (stable sort).
fn sorted(mut hits: Vec<Scored<'_>>) -> Vec<Scored<'_>> {
    hits.sort_by_key(|h| std::cmp::Reverse(h.2.score));
    hits
}

async fn score_all<'a>(
    candidates: &[Candidate<'a>],
    probe: &Probe<'_>,
    cx: &Cx,
) -> Result<Vec<Scored<'a>>> {
    let mut hits = Vec::new();
    for &(f, e) in candidates {
        if let Some(d) = e.handler.score(probe, cx).await? {
            hits.push((f, e, d));
        }
    }
    Ok(hits)
}

/// The shared header prefix and the length, for `file` values (one `stat` plus one ranged read).
async fn probe_data(source: &Value, header_len: usize) -> Result<(Vec<u8>, Option<u64>)> {
    let Some(file) = source.downcast_ref::<FileData>() else {
        return Ok((Vec::new(), None));
    };
    let start = file.range.as_ref().map_or(0, |r| r.start);
    let len = match &file.range {
        Some(r) => r.end - r.start,
        None => file.op.stat(&file.path).await?.content_length(),
    };
    let n = (header_len as u64).min(len);
    let header = if n == 0 {
        Vec::new()
    } else {
        file.op
            .read_with(&file.path)
            .range(start..start + n)
            .await?
            .to_vec()
    };
    Ok((header, Some(len)))
}

fn extension_of_key(key: &ValueKey) -> Option<String> {
    match key {
        ValueKey::File { path, .. } => {
            let name = path.rsplit('/').next()?;
            let (stem, ext) = name.rsplit_once('.')?;
            (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// Per-call context handed to handlers.
#[derive(Clone)]
pub struct Cx {
    /// For handlers that need another conversion.
    pub mediator: Mediator,
    /// Where inputs live.
    pub input: Operator,
    /// Where outputs go.
    pub output: Operator,
    /// Containers mounted so far in this call.
    pub mounts: Arc<crate::resolve::Mounts>,
    /// Output path -> key of the value written there, for the whole call (PROJECT.md §5.5).
    written: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Triggered by the host to stop the call; long handlers check it.
    pub cancel: CancellationToken,
    /// Bounds for `blocking` and `io`.
    limits: Arc<Limits>,
    /// Values requested by the converters of the current output (see `scoped`).
    linked: Arc<std::sync::Mutex<Vec<crate::engine::LinkedValue>>>,
    /// The plugin cache operator, if the host gave one.
    cache: Option<Operator>,
    /// Requests being served on the way to this one, outermost first (PROJECT.md §5.6).
    chain: Arc<Vec<String>>,
}

/// Semaphores behind `Cx::blocking` and `Cx::io`.
struct Limits {
    blocking: Semaphore,
    io: Semaphore,
}

impl Cx {
    /// A context with nothing mounted yet. One context serves one `Engine` call.
    pub fn new(mediator: Mediator, input: Operator, output: Operator) -> Self {
        Self {
            mediator,
            input,
            output,
            mounts: Arc::default(),
            written: Arc::default(),
            linked: Arc::default(),
            cache: None,
            chain: Arc::default(),
            cancel: CancellationToken::new(),
            limits: Arc::new(Limits {
                blocking: Semaphore::new(Semaphore::MAX_PERMITS),
                io: Semaphore::new(Semaphore::MAX_PERMITS),
            }),
        }
    }

    /// This context with its own, empty list of linked values: one per top-level output.
    pub(crate) fn scoped(&self) -> Self {
        let mut next = self.clone();
        next.linked = Arc::default();
        next
    }

    /// Notes that the converter running asked for `key` converted to `format`, once.
    pub(crate) fn link(&self, key: String, format: String) {
        let value = crate::engine::LinkedValue { key, format };
        let mut linked = self.linked.lock().unwrap();
        if !linked.contains(&value) {
            linked.push(value);
        }
    }

    /// The values linked so far in this scope.
    pub(crate) fn dependencies(&self) -> Vec<crate::engine::LinkedValue> {
        self.linked.lock().unwrap().clone()
    }

    /// Uses `op` as the plugin cache.
    pub fn with_cache(mut self, op: Operator) -> Self {
        self.cache = Some(op);
        self
    }

    /// Plugin `plugin`'s part of the plugin cache (PROJECT.md §7), namespaced by its id and
    /// `cache_version`. `None` if the host configured no cache; plugins must work without it.
    pub fn cache(&self, plugin: &str) -> Option<crate::cache::PluginCache> {
        let op = self.cache.clone()?;
        Some(crate::cache::PluginCache::new(
            op,
            plugin,
            self.mediator.cache_version(plugin),
            self.mediator.clone(),
        ))
    }

    /// The requests being served on the way to this one, outermost first.
    pub fn chain(&self) -> &[String] {
        &self.chain
    }

    /// This context with `key` appended to the call chain.
    pub(crate) fn pushed(&self, key: String) -> Self {
        let mut next = self.clone();
        next.chain = Arc::new(self.chain.iter().cloned().chain([key]).collect());
        next
    }

    /// Uses `cancel` as this call's token.
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Bounds `blocking` and `io` to the given numbers of concurrent tasks.
    pub fn with_limits(mut self, blocking: usize, io: usize) -> Self {
        let clamp = |n: usize| n.clamp(1, Semaphore::MAX_PERMITS);
        self.limits = Arc::new(Limits {
            blocking: Semaphore::new(clamp(blocking)),
            io: Semaphore::new(clamp(io)),
        });
        self
    }

    /// `Err(Cancelled)` if the token is triggered. Long loops call this between units of work.
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(Error::new(ErrorKind::Cancelled));
        }
        Ok(())
    }

    /// Publishes `Event::Warning`.
    pub fn warn(&self, message: impl Into<String>) {
        self.mediator.publish(Event::Warning {
            message: message.into(),
        });
    }

    /// Runs CPU-heavy or blocking `f` on the blocking pool, bounded by `blocking_limit`.
    pub async fn blocking<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.check_cancelled()?;
        let _permit = self
            .limits
            .blocking
            .acquire()
            .await
            .expect("semaphore is never closed");
        tokio::task::spawn_blocking(f).await.map_err(|e| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: format!("blocking task failed: {e}"),
            })
        })
    }

    /// Runs an I/O-bound future, bounded by `io_limit`.
    pub async fn io<T, Fut>(&self, fut: Fut) -> Result<T>
    where
        Fut: std::future::Future<Output = T> + Send,
        T: Send,
    {
        self.check_cancelled()?;
        let _permit = self
            .limits
            .io
            .acquire()
            .await
            .expect("semaphore is never closed");
        Ok(fut.await)
    }

    /// Claims `path` for the value with key `key`. `false`: the same key already wrote it.
    fn claim(&self, path: &str, key: &crate::value::ValueKey) -> Result<bool> {
        let mut written = self.written.lock().unwrap();
        match written.get(path) {
            Some(k) if *k == key.to_string() => Ok(false),
            Some(_) => Err(Error::new(ErrorKind::OutputConflict {
                path: path.to_owned(),
            })),
            None => {
                written.insert(path.to_owned(), key.to_string());
                Ok(true)
            }
        }
    }

    /// Writes `bytes` to `path` on the output operator on behalf of the value with key `key`.
    ///
    /// The first writer of a path wins. Writing the same key to the same path again is skipped
    /// (`Ok`, nothing written); a different key is `OutputConflict`. Returns whether the file
    /// was written.
    pub async fn write(
        &self,
        path: &str,
        key: &crate::value::ValueKey,
        bytes: bytes::Bytes,
    ) -> Result<bool> {
        if !self.claim(path, key)? {
            return Ok(false);
        }
        if let Err(e) = self.output.write(path, bytes).await {
            // Leave the path free for a retry.
            self.written.lock().unwrap().remove(path);
            return Err(e.into());
        }
        self.mediator.publish(Event::OutputWritten {
            path: path.to_owned(),
        });
        Ok(true)
    }

    /// Like [`Cx::write`], for a blob: reads it in chunks and streams them to the output, so a
    /// large (or computed, see [`LazyBlob`](crate::value::LazyBlob)) blob is never held whole.
    /// A skipped path (same key) reads nothing, so a lazy blob is not even computed. A failure
    /// part-way aborts the file and frees the path.
    pub async fn write_blob(
        &self,
        path: &str,
        key: &crate::value::ValueKey,
        blob: &dyn crate::capability::Blob,
    ) -> Result<bool> {
        if !self.claim(path, key)? {
            return Ok(false);
        }
        if let Err(e) = self.copy_blob(path, blob).await {
            self.written.lock().unwrap().remove(path);
            return Err(e);
        }
        self.mediator.publish(Event::OutputWritten {
            path: path.to_owned(),
        });
        Ok(true)
    }

    async fn copy_blob(&self, path: &str, blob: &dyn crate::capability::Blob) -> Result<()> {
        let mut writer = self.output.writer(path).await?;
        let copied = async {
            let mut at = 0;
            // No cancellation check here: a finished conversion is still written, as `write` does.
            // A computed blob stops by itself when the token fires while it is being produced.
            loop {
                let chunk = blob.read(at..at + crate::capability::CHUNK, self).await?;
                if chunk.is_empty() {
                    return Ok(());
                }
                at += chunk.len() as u64;
                writer.write(chunk).await?;
            }
        }
        .await;
        match copied {
            Ok(()) => {
                writer.close().await?;
                Ok(())
            }
            Err(e) => {
                let _ = writer.abort().await;
                Err(e)
            }
        }
    }
}
