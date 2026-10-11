# 05 · Core: mediator, handlers, plugins, Cx
**Status:** todo · **Depends on:** 02–04 · **See:** PROJECT.md §5

## Goal
Write the interfaces of the hand-written in-process mediator. It is the heart of the architecture.

## Scope (`src/mediator/`)
- `trait Request: Send + Sync + 'static { type Response: Clone + Send + Sync + 'static; fn key(&self) -> RequestKey; }`
- `#[async_trait] trait Handler<R: Request>: Send + Sync`, with:
  - `fn info(&self) -> HandlerInfo`
  - `fn can_handle(&self, &R) -> bool`
  - `async fn handle(&self, R, &Cx) -> Result<R::Response>`
- `HandlerInfo` holds **static** metadata: `id`, `label`, `plugin`, and `role`, where `role` is one of:
  - `Input { formats }`
  - `Decoder { kind, versions: VersionRange, produces: &'static str }`
  - `Encoder { target_type, accepts: Vec<&'static str>, defaults: Value }`
  - `Other`
  
  Discovery computes available targets from this metadata alone, without decoding.
- `trait Plugin { fn id(&self) -> &str; fn register(&self, m: &mut Mediator); }`. Document that a plugin registers *all* of its handlers and may take options to enable a subset.
- `Mediator` with `register<R>`, `add_middleware`, `async send<R>(&self, R, &Cx)`, `publish` and `subscribe`.
  - Internally: handlers are stored per `TypeId` of `R`. Registration order matters, because the **first** handler whose `can_handle` returns true wins; document this.
  - Type erasure: `ErasedRequest` exposes key, type name and `Any`. `ErasedResponse` is `Arc<dyn Any + Send + Sync>`.
- `Cx` is the per-call context. It holds:
  - a mediator handle (`Arc`)
  - the call chain (`Vec<RequestKey>`)
  - the `CancellationToken`
  - the input and output `opendal::Operator`s
  - `cache() -> Option<PluginCache>`, which is namespaced per plugin (see task 10)
  - `blocking(f)` (`spawn_blocking` under the concurrency limit) and `io(f)`
- Document that plugins may define their **own request types** (e.g. a private `ReadStarpakRange`).
- Logic bodies are `todo!()`.

## Acceptance
- It compiles. The traits are object-safe where they need to be (`Arc<dyn Handler<R>>`).
- `cargo doc` explains routing order, how `NoHandler` versus `UnsupportedVersion` is chosen, and the role of `Cx`.
