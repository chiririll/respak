# 06 · Slice: `formats` and `discover`
**Status:** done · **Depends on:** 05 · **See:** PROJECT.md §5.2, §5.3, §8, §10

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

## Decisions (the user delegated the gate: "implement all remaining tasks without asking")
- Direct edges `.to(dst, handler)` are added to core now (needed for the tie-break); png/dds don't use them.
- The planner is a general search over direct and capability edges. Order: cost, then fewer capability steps, then fewer hops, then registration order. `via` must be a subsequence of the path's formats (ends included).
- The chosen path is logged with `tracing::warn!` only when more than one complete path existed.
- `discover` on an unrecognised file gives an entry with `format: null`. Ties are not an error: the first hit wins, runners-up go in `alternatives`. `defaults` is `{}`, `name` is empty at depth 0. `depth > 0` is `InvalidRequest` until slice 07.
- Scores: PNG = magic 10 + `IHDR` at 12 (+2) + extension (+1); DDS = magic 10 + header size 124 (+2) + extension (+1). Both 13 for a well-named file.
- `formats` / `discover` print JSON.

## Tests
Core:
1. `u16le`/`u32le` are correct, including at the end of the header, and `None` on a short header; `within` is false for `None`.
2. `detect` returns all hits, winner first, ties in registration order, and applies no read handler; empty when unrecognised.
3. `plan` picks the cheapest path; at equal cost it prefers direct edges, then fewer hops, then earlier registration.
4. `plan` with `via` forces the longer path; an unsatisfiable `via` is `NoPath`; an already-there format is an empty path.
5. `plan` reports alternatives only when there was a real choice (unit test).
6. `targets_from` follows multi-hop paths and direct edges, excludes the source format.
7. `formats()` lists extensions, provides, read-from and produced-by edges with costs and labels.
8. `Engine::discover`: winner, version, score, alternatives, targets; unrecognised entry; `NotFound`; `depth > 0` is `InvalidRequest`.
9. `convert` honours `via` and runs multi-hop paths; `via` round-trips in the request JSON.
10. A direct edge to an undefined format fails `build`.

Formats:
11. `discover` on a PNG: `png`, score 13, targets include `dds`; on a DDS: `dds`, 13, targets include `png`.
12. A DDS named `.png` is discovered as `dds`.
13. Every listed target converts.

CLI:
14. `formats` prints the graph. `discover a b` prints one entry per path. A missing path exits non-zero with the error on stderr.
15. `convert` with `via` in `--to` works; an unsatisfiable `via` exits non-zero.
