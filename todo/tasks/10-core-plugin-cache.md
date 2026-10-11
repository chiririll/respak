# 10 · Core: plugin cache
**Status:** todo · **Depends on:** 05 · **See:** PROJECT.md §8

## Goal
Provide optional, pluggable, abstract storage that plugins use to speed up decoding: name mappings, indexes, anything else a plugin finds useful.

## Scope (`src/cache.rs`)
- The backing store is an optional `opendal::Operator` given to the Engine builder. There is no home-made storage.
- `PluginCache` is a namespaced view (prefix `<plugin-id>/v<schema>/`). Methods:
  - `async get(key) -> Result<Option<Bytes>>`
  - `async put(key, Bytes)`
  - `async remove(key)`
  - `get_json<T>` and `put_json<T>`
- Plugins get it through `cx.cache()`, which returns `None` when no cache is configured.
- Contract, documented:
  - **Core never interprets contents.**
  - **Plugins must work correctly without a cache.**
  - A cache error becomes a `Warning` event and is never fatal.
- Doc example: RPak caching GUID → original name and GUID → containing file. A GUID is a hash of the asset path, as in rsx's `cachedb.h`.

## Acceptance
- It compiles and is documented.
