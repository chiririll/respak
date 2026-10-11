# 06 · Core: middleware
**Status:** todo · **Depends on:** 05 · **See:** PROJECT.md §5 (Middleware)

## Goal
Define the middleware trait and the built-in middleware types. Only signatures and docs this phase; bodies are `todo!()`.

## Scope (`src/mediator/middleware.rs`)
- `#[async_trait] trait Middleware: Send + Sync { async fn around(&self, req: &dyn ErasedRequest, cx: &Cx, next: Next<'_>) -> Result<ErasedResponse>; }`
- Built-ins, in this default order (each type documents its exact contract):
  1. `CancellationMiddleware`: fails with `Cancelled` if the token has been triggered.
  2. `CycleGuard`: a key already in `cx` call chain gives `DependencyCycle`. **It also keeps a wait-for graph over in-flight keys** and fails the request that would close a cycle across concurrent chains. Without this, chain A→B and chain B→A would deadlock on each other's shared futures.
  3. `Dedup`: an in-flight map of `RequestKey` to `futures::future::Shared`. **The entry is removed when the request completes** (memory policy: nothing is memoized after completion). The exception is opened inputs, which stay open for the duration of one `Engine::convert` call. Document how this is scoped.
  4. `TracingMiddleware`: a span per request.
  5. `EventsMiddleware`: publishes `RequestStarted`, `RequestFinished` and `RequestFailed`.
- Document explicitly: **no per-request concurrency limit.** A parent awaiting its children while holding a permit deadlocks. Limits live only in `Engine::convert` (top-level outputs) and in `Cx::blocking` / `Cx::io`.
- Hosts can add custom middleware through the Engine builder.

## Acceptance
- It compiles, and every built-in documents its behavior and its place in the order.
