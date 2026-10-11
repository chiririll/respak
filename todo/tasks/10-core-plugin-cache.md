# 10 · Core: plugin cache
**Status:** todo · **Depends on:** 07 · **See:** PROJECT.md §7

## Scope (`src/cache.rs`)
- `PluginCache` over an optional `Operator`, namespaced by plugin id + `Plugin::cache_version()`: `get`, `put`, `remove`, `get_json`, `put_json`.
- `cx.cache() -> Option<PluginCache>`.
- Docs: plugins must work without it; cache errors become `Warning` events, never failures.

## Acceptance
- It compiles.
