//! The per-call context handed to handlers.

use std::sync::Arc;

use opendal::Operator;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, ErrorKind, Result};
use crate::event::Event;

use super::Mediator;

mod output;

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
    pub mounts: Arc<super::open::Mounts>,
    /// Output path -> key of the value written there, for the whole call (docs/design/project.md §5.5).
    written: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Triggered by the host to stop the call; long handlers check it.
    pub cancel: CancellationToken,
    /// Bounds for `blocking` and `io`.
    limits: Arc<Limits>,
    /// Values requested by the converters of the current output (see `scoped`).
    linked: Arc<std::sync::Mutex<Vec<crate::engine::LinkedValue>>>,
    /// The plugin cache operator, if the host gave one.
    cache: Option<Operator>,
    /// Requests being served on the way to this one, outermost first (docs/design/project.md §5.6).
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

    /// Plugin `plugin`'s part of the plugin cache (docs/design/project.md §7), namespaced by its id and
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
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| Error::msg(format!("blocking task failed: {e}")))
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
}
