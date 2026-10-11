# 05 · Slice: walking skeleton, PNG ↔ DDS
**Status:** todo · **Depends on:** 04 · **See:** PROJECT.md §5.1–§5.5, §8, §10

## Goal
The thinnest path through every layer, on real files, in both directions:
```
respak --root <dir> --out <dir> convert icon.png --to '{"type":"dds"}'
respak --root <dir> --out <dir> convert icon.dds --to '{"type":"png"}'
```
A PNG → DDS → PNG round trip gives the same pixels. This slice uses one plugin and no containers. It checks whether the core shapes (plugin registration, detection, capability edges, the mediator) work in real handlers before more is built on them.

## Scope
**Core** (only what these commands need):
- `Plugin { id, register }`, and `Registry` with `format(name)`, `.from(src, handler)`, `.extensions([..])`, `.provides::<dyn Cap>()`, `.from_cap::<dyn Cap>(handler)`, `.cost(n)`. Defining a format twice is an error at build time.
- Read handler = `Detect::score` + read. `Probe` holds the source value, the extension, the length (from `stat`) and a shared header prefix. The only score helper for now is `magic`.
- Detection: extension candidates first, then all read edges. Highest score wins. A tie → `AmbiguousFormat`. Nothing matches → `UnknownFormat`.
- Path search: the cheapest path over direct and capability edges, enough for `file → png ─(Bitmap)→ dds` and `file → dds ─(Bitmap)→ png`. No path → `NoPath`.
- `Mediator::convert` / `identify` as plain calls with no middleware. `Cx` holds the mediator handle and the input/output operators.
- Output: core writes a `Blob` result to the output operator. Default path = the input path with the target's extension.
- `Engine::builder().input().output().plugin().build()` and `Engine::convert(&ConvertRequest)`, where `from` can only be a single path for now. Keep the §8 JSON shape so it doesn't change later.

**respak-formats:**
- `png`: read from `file` (magic + extension), provides `Bitmap` (RGBA8); encode from `Bitmap` (cost 2).
- `dds`: read from `file` (magic + extension), provides `Bitmap`; encode from `Bitmap` (cost 1). Uncompressed RGBA8 only for now. BC passthrough comes in slice 08, and BC decoding (a BC-compressed DDS → PNG) in slice 09.
- Pick existing crates. Ask the user if unsure.

**CLI:** `--root`, `--out`, `convert <path> --to '<json>' [--path P]`. `lib.rs` `run(cli, engine, out)` can be tested in-process with `services::Memory`. The exit code is non-zero on failure and the error goes to stderr.

## Not in this slice
Events, middleware, cancellation, the plugin cache, containers/volumes, globs, `run`, `discover`, `formats`, `via`, concurrency limits, target options.

## Demo
The user round-trips a real PNG through DDS and back, and opens both outputs in a viewer.
