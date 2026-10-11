# 13 · Plugin skeleton: respak-formats
**Status:** todo · **Depends on:** 11 · **See:** PROJECT.md §5.2

## Scope
- `FormatsPlugin`; formats `png` and `dds`: read from `file` (provide `Bitmap`), encode from `Bitmap` (png cost 2; dds cost 1, copying BC blocks as-is).
- Option structs via `TypedTarget` (e.g. `mips: "all" | "first" | n`, dds pixel format), with `Default`.
- Pick existing crates for PNG/DDS encoding and BC decoding; ask the user if unsure.
- **Depends only on `respak-core`.**

## Acceptance
- It compiles and registers.
