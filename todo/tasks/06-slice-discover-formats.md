# 06 · Slice: `formats` and `discover`
**Status:** todo · **Depends on:** 05 · **See:** PROJECT.md §5.2, §5.3, §8, §10

## Goal
```
respak formats
respak discover icon.png icon.dds
```
`formats` prints the format graph. `discover` prints the detected format, its score, the runners-up and the reachable targets. An entry's `path` + a target can be pasted straight into `convert`.

## Scope
**Core:**
- `FormatGraph`: targets reachable from a format and its declared capabilities, found without decoding anything. `Engine::formats()`.
- `Engine::discover(path, &DiscoverOptions { depth: 0 })` → `Discovery` exactly as in §8 (winner, score, `alternatives`, `targets` with `defaults`).
- Path choice made visible: tie-break (direct edge before capability edge, then fewer hops, then registration order) and `via` pinning. Log the chosen path with `tracing::warn!` until events exist (slice 12).
- More score helpers as needed (`u16le`, `u32le`, `within`), plus the documented score convention.

**CLI:** `formats`, `discover <path>...`; `convert` accepts `via`.

## Not in this slice
Containers (`depth` > 0 comes in slice 07) and events.

## Demo
The user runs `formats` and `discover` on a real PNG and DDS and checks that the listed targets convert.
