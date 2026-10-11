# 06 · Core: plugins, registry, detection, path search
**Status:** todo · **Depends on:** 03, 04 · **See:** PROJECT.md §5.2, §5.3

## Goal
How plugins describe formats and edges, how inputs are detected, how a conversion path is chosen.

## Scope
- `trait Plugin { fn id(&self) -> &str; fn cache_version(&self) -> u32 { 0 } fn register(&self, r: &mut Registry); }`
- `Registry` builder grouped per format: `format(name)`, `.from(source_format, read_handler)`, `.extensions([...])`, `.provides::<dyn Cap>()`, `.to(target_format, convert_handler)`, `.from_cap::<dyn Cap>(handler)`, `.cost(n)`. Duplicate format definition → build error.
- Handler traits (object-safe, `async_trait`): read handler = `Detect` + read; convert handler. Typed adapters that downcast the input `Value`.
- `Detect::score(&Probe, &Cx) -> Result<Option<Detected { score, version }>>`. `Probe`: source value, extension, length, shared header prefix, ranged read; `header_len()` per candidate.
- Score helper functions (`magic`, `u16le/u32le/…`, `within`) and the documented score convention.
- Detection order: extension candidates first, then all read edges from the source format; tie → `AmbiguousFormat`; known format + unknown variant → `UnsupportedVersion`.
- `FormatGraph`: per-request cheapest path from a format and its declared capabilities; tie-break (direct > capability, fewer hops, registration order) with a `Warning`; `via` pinning. Also used by discovery to list reachable targets.

## Acceptance
- It compiles; `cargo doc` explains detection order, the score convention, costs and the tie-break.
