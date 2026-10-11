# 11 · Core: Engine and JSON API
**Status:** todo · **Depends on:** 05–10 · **See:** PROJECT.md §8

## Scope (`src/engine/`)
- `Engine::builder()`: `input`, `output`, `cache`, `plugin`, `middleware`, `max_concurrency`, `blocking_limit`, `io_limit`, `event_capacity`, `build()` (fails on duplicate formats).
- `discover(path, &DiscoverOptions{depth}, &cancel) -> Result<Discovery>`; `convert(&ConvertRequest, &cancel) -> Result<ConvertReport>`; `open(path_or_glob, &cancel)`; `events()`; `formats()`.
- Serde types exactly as in §8: `ConvertRequest { version, outputs: [{ from, to, path? }] }` (`from` = path | glob | list of path / `{path, format}`), `ConvertReport { outputs: [{ from, result }] }`, `Discovery { path, entries }`.
- Bundle rule (target has an edge from `bundle`), default output path rule, one report entry per conversion.

## Acceptance
- It compiles; the example request in §8 deserializes.
