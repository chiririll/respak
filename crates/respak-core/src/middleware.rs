//! Middleware around every conversion request (PROJECT.md §5.6).

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
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
    chain: &'a [Arc<dyn Middleware>],
    mediator: &'a Mediator,
}

impl<'a> Next<'a> {
    pub(crate) fn new(chain: &'a [Arc<dyn Middleware>], mediator: &'a Mediator) -> Self {
        Self { chain, mediator }
    }
}

impl Next<'_> {
    /// Runs the remainder of the chain.
    pub async fn run(self, req: &Convert, cx: &Cx) -> Result<Value> {
        match self.chain.split_first() {
            Some((first, rest)) => first.around(req, cx, Next { chain: rest, mediator: self.mediator }).await,
            None => self.mediator.convert_unwrapped(req.source.clone(), req.to.clone(), cx).await,
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
        cx.mediator.publish(Event::RequestStarted { what: what.clone() });
        let result = next.run(req, cx).await;
        cx.mediator.publish(match &result {
            Ok(_) => Event::RequestFinished { what },
            Err(error) => Event::RequestFailed { what, error: error.to_string() },
        });
        result
    }
}

/// The built-ins in their fixed order (PROJECT.md §5.6). The cycle guard and dedup are not
/// built yet.
pub(crate) fn builtins() -> Vec<Arc<dyn Middleware>> {
    vec![Arc::new(CancellationMiddleware), Arc::new(TracingMiddleware), Arc::new(EventsMiddleware)]
}
