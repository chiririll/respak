# 05 · Core: events
**Status:** todo · **Depends on:** 03 · **See:** PROJECT.md §5.7

## Scope (`src/event.rs`)
- `#[non_exhaustive] enum Event`: `RequestStarted`, `RequestFinished`, `RequestFailed`, `InputOpened{path, entries}`, `OutputWritten{path}`, `Progress{task, done, total}`, `Warning{message}`, `Custom{source, name, data}`. Serializable.
- Broadcast channel wrapper with configurable capacity.
- `LogSubscriber` forwarding events to `tracing`.
- Docs: lagging subscribers may miss events; the report holds what matters.

## Acceptance
- It compiles.
