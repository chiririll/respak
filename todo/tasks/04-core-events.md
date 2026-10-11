# 04 · Core: events
**Status:** todo · **Depends on:** 03 · **See:** PROJECT.md §5 (Events)

## Goal
Define the pub/sub event types used for logging, progress and host UIs.

## Scope (`src/events.rs`)
- `#[non_exhaustive] enum Event`, which is `Clone + Serialize`, with these variants:
  - `RequestStarted { key, name }`, `RequestFinished { key }`, `RequestFailed { key, error }`
  - `InputOpened { path, files: Vec<String> }`, `OutputWritten { path }`
  - `Progress { task: String, done: u64, total: Option<u64> }`
  - `Warning { message }`
  - `Custom { source: String, name: String, data: Value }`
- `EventBus` is a thin wrapper over `tokio::sync::broadcast` with a configurable capacity. Methods: `publish` and `subscribe`.
- `LogSubscriber` forwards events to `tracing`, at a matching level.
- Document that **progress is only an event** (there is no separate progress interface), and that a lagging subscriber may miss events. Results that matter must always be in returned values, never only in events.
- Logic bodies are `todo!()`.

## Acceptance
- It compiles and is documented.
