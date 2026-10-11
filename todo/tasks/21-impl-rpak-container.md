# 21 · Implement RPak container (TDD)
**Status:** blocked · **Depends on:** 19, 20 · **See:** task 12, docs/formats/rpak.md

## Goal
Implement the `OpenInput`, `Discover`, `Select`, `Locate` and `ReadItem` handlers in `respak-rpak`.

## Requirements
- Follow the large-file rule (PROJECT.md §7). Header and tables are read with ranged reads where possible. Starpak data is never read fully; `ItemData` reads ranges on demand.
- Companion files (starpaks, patches) are found through the input `Operator`.
- `Locate` checks the pak's own items, then the plugin cache, then opens other files lazily (only when needed).
- An optional cache stores name mappings and GUID → file mappings, and all of it works without the cache.
- Unknown versions give `UnsupportedVersion`.

## Tests
_To be filled in by task 17._ Use synthetic paks generated in the tests.
