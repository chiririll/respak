# 11 · Core: Engine & JSON API
**Status:** todo · **Depends on:** 05–10 · **See:** PROJECT.md §9

## Goal
Build the public, host-facing API as a thin layer over the mediator.

## Scope (`src/engine.rs`, `src/api.rs`)
- `Engine::builder()`. Methods:
  - `.input(Operator)` and `.output(Operator)` (required)
  - `.cache(Operator)` (optional)
  - `.plugin(impl Plugin)`, `.middleware(..)`
  - `.max_concurrency(n)`, `.event_capacity(n)`
  - `.build() -> Result<Engine>`
  
  The default middleware stack is installed automatically.
- Methods:
  - `async discover(&self, path, &CancellationToken) -> Result<Discovery>`
  - `async convert(&self, &ConvertRequest, &CancellationToken) -> ConvertReport`. **One failing output doesn't stop the others.** Top-level outputs run in parallel up to `max_concurrency`.
  - `async decode(&self, path, selector: &Value, &CancellationToken) -> Result<Decoded>`
  - `events(&self) -> broadcast::Receiver<Event>`
  - `handlers(&self) -> Vec<HandlerInfo>`, used by the CLI `formats` command
- Serde types:
  - `ConvertRequest { version: u32, input: String, format: Option<String>, outputs: Vec<OutputRequest> }`
  - `OutputRequest { select: Value, target: Value, path: Option<String> }`
  - `ConvertReport { outputs: Vec<OutputResult> }`, where `OutputResult` is `Ok { files, dependencies }` or `Err { error }`
  - `Discovery { input, format, items: Vec<DiscoveredItem> }`
- Docs include the full JSON examples from PROJECT.md §9, and state that discovery returns the same shapes a request uses.
- Defaults to document:
  - the input format is auto-detected, and `format` overrides it
  - existing outputs are overwritten
  - when `path` is absent, the input plugin's `suggested_path` is used

## Acceptance
- It compiles, and `cargo doc` shows the JSON examples.
