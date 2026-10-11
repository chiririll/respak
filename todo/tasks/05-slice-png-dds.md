# 05 · Slice: walking skeleton, PNG ↔ DDS
**Status:** done · **Depends on:** 04 · **See:** PROJECT.md §5.1–§5.5, §8, §10

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

## Decisions (gate)
- Crates: `png` 0.18, `ddsfile` 0.6.
- `Cap` only names a capability (no `Cap::of`); handlers call `value.as_bitmap()` directly.
- DDS is written with a legacy header and RGBA masks; read accepts RGBA8 and BGRA8.
- A duplicate format is `ErrorKind::Custom`. No `.to()` edges. No `dependencies` in the report yet.
- PNG decode reads the whole file; DDS parses the header and reads mips with ranged reads.

## Tests
Core (synthetic plugin, `services::Memory`):
1. Registering the same format twice makes `Engine::build` fail.
2. `magic` matches at an offset and is false on a short header.
3. Detection scores extension candidates first, then all read edges if none match.
4. Highest score wins and carries its version.
5. Equal top score → `AmbiguousFormat`; no match → `UnknownFormat`.
6. The header prefix is read once, sized to the largest `header_len`.
7. Path search finds `file → A ─(Bitmap)→ B` and picks the cheaper of two capability edges.
8. No capability edge to the target → `NoPath`.
9. A source already in the target format is returned as is.
10. Default output path = input path with the target's extension; `path` overrides it.
11. A non-`Blob` result gives an error, not a write.
12. Unknown request `version` → `InvalidRequest`.
13. One failing output doesn't stop the others; one report entry each.
14. Request JSON round-trips (`type` + flattened options).
15. Missing input file → `NotFound`.

Formats:
16. PNG → DDS → PNG gives identical pixels (generated gradient with alpha).
17. DDS → PNG → DDS gives identical bytes.
18. Both decoders reject truncated or garbage input with `Corrupt`.
19. A BC-format DDS gives a clear unsupported error.
20. A `.png` file with DDS content is detected as DDS (magic beats extension).
21. A BGRA8 DDS gives an RGBA8 PNG with the right channel order.
22. Odd sizes (1×1, 3×5) round-trip.

CLI (in-process, `services::Memory`):
23. `convert` writes the output and prints the report JSON to `out`.
24. A failing convert exits non-zero and writes the error to stderr.
25. `--path` overrides the output path.
26. Malformed `--to` JSON → `InvalidRequest`.
