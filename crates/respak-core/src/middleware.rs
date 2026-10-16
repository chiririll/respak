//! Middleware around every conversion request (PROJECT.md §5.6).

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::future::{BoxFuture, FutureExt, Shared};

use crate::error::{Error, ErrorKind, Result};
use crate::event::Event;
use crate::mediator::{Cx, Mediator, Target};
use crate::value::Value;

/// One conversion request as middleware sees it.
pub struct Convert {
    /// The value to convert.
    pub source: Value,
    /// What to convert it to.
    pub to: Target,
}

/// The rest of the chain; calling it runs the next middleware or, last, the conversion.
pub struct Next<'a> {
    chain: Arc<[Arc<dyn Middleware>]>,
    index: usize,
    mediator: Mediator,
    _borrow: PhantomData<&'a ()>,
}

impl<'a> Next<'a> {
    pub(crate) fn new(chain: &'a Arc<[Arc<dyn Middleware>]>, mediator: &'a Mediator) -> Self {
        Self {
            chain: chain.clone(),
            index: 0,
            mediator: mediator.clone(),
            _borrow: PhantomData,
        }
    }

    /// The same continuation without a borrow, for middleware that runs it on a future that
    /// outlives the call (dedup shares one run between requests).
    pub fn into_owned(self) -> Next<'static> {
        Next {
            chain: self.chain,
            index: self.index,
            mediator: self.mediator,
            _borrow: PhantomData,
        }
    }
}

impl Next<'_> {
    /// Runs the remainder of the chain.
    pub async fn run(self, req: &Convert, cx: &Cx) -> Result<Value> {
        match self.chain.get(self.index).cloned() {
            Some(middleware) => {
                let next = Next {
                    chain: self.chain,
                    index: self.index + 1,
                    mediator: self.mediator,
                    _borrow: PhantomData,
                };
                middleware.around(req, cx, next).await
            }
            None => {
                self.mediator
                    .convert_unwrapped(req.source.clone(), req.to.clone(), cx)
                    .await
            }
        }
    }
}

/// Wraps a request. Hosts add their own through `EngineBuilder::middleware`.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Handles `req`; call `next.run(req, cx)` to continue.
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value>;
}

/// `Cancelled` if the token is already triggered when the request starts.
pub struct CancellationMiddleware;

/// A request already in the call chain is `DependencyCycle`; otherwise it extends the chain
/// for everything the handler asks for in turn.
pub struct CycleGuardMiddleware;

/// Merges concurrent identical requests into one run and rejects waits that would deadlock.
///
/// A request is identified by its source key, target format and options. While one is in
/// flight, others for the same key join its `Shared` future; the entry is dropped when the
/// run ends, so later requests run again. Each request records "parent waits for key" in a
/// wait-for graph; one that would close a loop (concurrent chains A -> B and B -> A) is a
/// `DependencyCycle` instead of a deadlock.
#[derive(Default)]
pub struct DedupMiddleware {
    state: Arc<Mutex<DedupState>>,
}

type Run = Shared<BoxFuture<'static, Result<Value>>>;

#[derive(Default)]
struct DedupState {
    in_flight: HashMap<String, Run>,
    /// waiter key -> keys it waits for (one entry per outstanding wait).
    waits: HashMap<String, Vec<String>>,
}

impl DedupState {
    fn reaches(&self, from: &str, to: &str) -> bool {
        let mut seen = vec![from];
        let mut todo = vec![from];
        while let Some(k) = todo.pop() {
            for next in self.waits.get(k).into_iter().flatten() {
                if next == to {
                    return true;
                }
                if !seen.contains(&next.as_str()) {
                    seen.push(next);
                    todo.push(next);
                }
            }
        }
        false
    }
}

/// Removes a recorded wait when the waiter finishes or is dropped.
struct WaitEdge {
    state: Arc<Mutex<DedupState>>,
    from: String,
    to: String,
}

impl Drop for WaitEdge {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        if let Some(edges) = state.waits.get_mut(&self.from) {
            if let Some(i) = edges.iter().position(|k| *k == self.to) {
                edges.swap_remove(i);
            }
            if edges.is_empty() {
                state.waits.remove(&self.from);
            }
        }
    }
}

/// The identity of a request: source key, target format, options and `via`.
fn request_key(req: &Convert) -> String {
    let key = crate::value::ValueKey::converted(
        Arc::new(req.source.key().clone()),
        req.to.format.clone(),
        &req.to.options,
    );
    if req.to.via.is_empty() {
        key.to_string()
    } else {
        format!("{key} via {}", req.to.via.join(","))
    }
}

/// `DependencyCycle` for the chain so far plus the request that closes the loop.
fn cycle(cx: &Cx, closing: String) -> Error {
    let mut chain = cx.chain().to_vec();
    chain.push(closing);
    Error::new(ErrorKind::DependencyCycle { chain })
}

/// One `tracing` span per request.
pub struct TracingMiddleware;

/// Publishes `RequestStarted` / `RequestFinished` / `RequestFailed`.
pub struct EventsMiddleware;

#[async_trait]
impl Middleware for CancellationMiddleware {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        cx.check_cancelled()?;
        next.run(req, cx).await
    }
}

#[async_trait]
impl Middleware for CycleGuardMiddleware {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        let key = request_key(req);
        if cx.chain().contains(&key) {
            return Err(cycle(cx, key));
        }
        // Only what a top-level request's converter asked for directly is a dependency.
        if cx.chain().len() == 1 {
            cx.link(req.source.key().to_string(), req.to.format.to_string());
        }
        next.run(req, &cx.pushed(key)).await
    }
}

#[async_trait]
impl Middleware for DedupMiddleware {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        let key = request_key(req);
        // The guard pushed `key`; the request that asked for it is the entry before.
        let parent = cx
            .chain()
            .iter()
            .rev()
            .nth(1)
            .filter(|_| cx.chain().last() == Some(&key))
            .cloned();

        let (run, _edge) = {
            let mut state = self.state.lock().unwrap();
            if let Some(parent) = &parent {
                if state.reaches(&key, parent) {
                    return Err(cycle(cx, parent.clone()));
                }
            }
            let edge = parent.map(|from| {
                state
                    .waits
                    .entry(from.clone())
                    .or_default()
                    .push(key.clone());
                WaitEdge {
                    state: self.state.clone(),
                    from,
                    to: key.clone(),
                }
            });
            // An entry only the map still holds lost every waiter (e.g. cancelled): start over.
            let live = state
                .in_flight
                .get(&key)
                .filter(|r| r.strong_count() != Some(1))
                .cloned();
            let run = match live {
                Some(run) => run,
                None => {
                    let owned = Convert {
                        source: req.source.clone(),
                        to: req.to.clone(),
                    };
                    let (cx, next, id, shared) = (
                        cx.clone(),
                        next.into_owned(),
                        key.clone(),
                        self.state.clone(),
                    );
                    let run = async move {
                        let result = next.run(&owned, &cx).await;
                        shared.lock().unwrap().in_flight.remove(&id);
                        result
                    }
                    .boxed()
                    .shared();
                    state.in_flight.insert(key, run.clone());
                    run
                }
            };
            (run, edge)
        };
        run.await
    }
}

#[async_trait]
impl Middleware for TracingMiddleware {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        use tracing::Instrument;
        let span = tracing::debug_span!("convert", from = %req.source.key(), to = %req.to.format);
        next.run(req, cx).instrument(span).await
    }
}

#[async_trait]
impl Middleware for EventsMiddleware {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        let what = format!("{} -> {}", req.source.key(), req.to.format);
        cx.mediator
            .publish(Event::RequestStarted { what: what.clone() });
        let result = next.run(req, cx).await;
        cx.mediator.publish(match &result {
            Ok(_) => Event::RequestFinished { what },
            Err(error) => Event::RequestFailed {
                what,
                error: error.to_string(),
            },
        });
        result
    }
}

/// The built-ins in their fixed order (PROJECT.md §5.6).
pub(crate) fn builtins() -> Vec<Arc<dyn Middleware>> {
    vec![
        Arc::new(CancellationMiddleware),
        Arc::new(CycleGuardMiddleware),
        Arc::new(DedupMiddleware::default()),
        Arc::new(TracingMiddleware),
        Arc::new(EventsMiddleware),
    ]
}
