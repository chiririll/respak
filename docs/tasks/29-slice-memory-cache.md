# 29 · Slice: in-memory cache (core)
**Status:** in-progress (built; demo pending) · **Depends on:** 13 · **See:** [project.md](../design/project.md) §5.6, §7

## Goal
Two requests for the same thing that do not overlap in time convert once. Today the dedup shares only concurrent requests, so a texture used by two materials that finish at different times is decoded twice.

Demo: `respak convert … --memory-cache 256MiB` on two materials that share a texture logs one `RequestStarted` for that texture (before: two).

## Changes
1. `DedupState` gets a completed-results store next to `in_flight`: key = the existing `request_key`, value = `Value`, weight = `approx_size()`. `moka::sync::Cache` with a weigher and a byte `max_capacity` does the LRU.
2. On a successful run the value is inserted; a later request for the same key returns it without running `next`. `Err` values are never stored (cancellation, `DependencyCycle`, failures).
3. A value with `approx_size() == 0` is not stored (it would be free to keep and never evicted).
4. `EngineBuilder::memory_cache(bytes)`; default off. When off, the middleware behaves exactly as today.
5. The cache belongs to the `Engine`, so it outlives one `convert` call (the web UI needs that; the CLI is unaffected).
6. Mounted volumes stay outside the cache (they live until the `Engine` call ends, §5.6).
7. CLI: `--memory-cache <MiB>`. `moka` is added to `respak-core` (project.md §3 lists it as the candidate).
8. project.md §7 and §11 updated with what was learned.

## Leaves out
- Removing the web UI's own preview cache (12 entries, 64 MiB; slice 24). It becomes redundant; a backlog follow-up.
- Raw-byte / chunk caching of the input operator (separate open question).
- Cache invalidation when files change on disk (see Open).
- `approx_size` for formats that return 0 today beyond a check of which ones matter.

## Open
- **Stale results.** `ValueKey::File` is a path plus range, with no mtime or size, so an `Engine` that lives long (web UI) can serve a result for a file that changed on disk. Options: (a) accept it and document it; (b) add a "revision" (size + mtime from `Operator::stat`) to the file key when the cache is on; (c) expose `Engine::clear_memory_cache()` for hosts. Recommendation: (a) + (c) now, (b) if it bites.
- Which formats report `approx_size() == 0` and should not (check `txtr`, `matl`, `rmdl`, Miles audio, bitmaps), or whether to give such values a minimum weight instead of skipping them.

## Tests (agreed at the gate)
- Cache on: two sequential requests for one source and target run the handler once (counting handler); the second returns the same `Arc`.
- Cache off (default): the same two requests run the handler twice (behaviour unchanged).
- Different options give different keys: both run.
- A failed run is not cached: the next request runs again.
- A cancelled run is not cached.
- Eviction: with capacity for one value, requesting A, B, A runs A twice; with capacity for two it runs A once.
- A value of size 0 is not cached.
- Concurrent requests still share one run (the existing dedup test passes unchanged), and the result then serves a later request.
- The cycle guard and wait-for graph tests pass unchanged with the cache on.
- A texture shared by two materials that finish at different times converts once (synthetic fixture, like the linking tests).
- CLI: `--memory-cache 0` is rejected or means off (decide at the gate).

## Built
`DedupMiddleware::with_memory_cache`, `EngineBuilder::memory_cache`, `--memory-cache SIZE` (`parse_size`, `0` is off) and `Settings::memory_cache`. Tests: [`memory_cache.rs`](../../crates/respak-core/tests/memory_cache.rs) (10, all but the CLI one) and `resolve_tests.rs` (the flag).
- Stale results: accepted and documented (project.md §7); `Engine::clear_memory_cache()` was **not** built (no host needs it yet).
- `eviction_policy(lru())` plus `run_pending_tasks()` after each insert, because moka's default TinyLFU may refuse a new entry and evicts lazily; both made the LRU test deterministic.
- Also `memory_cache = "256MiB"` in the config file (a size string like the Steam `cache_size`); the command line wins, `--memory-cache 0` turns a file setting off.
- Not checked: which real formats report `approx_size() == 0` (they are simply not cached). `bik` and `png` report a size; `txtr`, `matl`, `rmdl` and Miles audio were not looked at.

