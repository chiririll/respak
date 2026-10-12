# 09 · Slice: BC decoding and target options (rpak texture → PNG)
**Status:** done · **Depends on:** 08 · **See:** PROJECT.md §5.2, §5.5

## Goal
```
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png","mips":2}'
respak convert foo_bc7.dds --to '{"type":"png"}'
```
The PNG encoder from slice 05 learns to decode BC formats, and targets get their first options.

## Scope
**respak-formats:**
- `png` encode decodes BC to RGBA. Pick a BC decoding crate; ask if unsure. The `dds` reader accepts BC-compressed files.
- Options through `TypedTarget<T>`: `mips: "first" | n`. Missing fields come from `Default`; bad options → `InvalidOptions`. `"all"` waits for composite folders (slice 11).

**Core:** `TypedTarget<T>`. Option normalization in value keys (defaults filled in, fields sorted).

## Demo
The user converts real textures of several pixel formats and checks the PNGs.

## Done
- `respak-formats/src/bc.rs`: BC1–BC7 → RGBA8 via `bcdec_rs`; `png` encode decodes any block format. The `dds` reader accepts BC files, arrays and cube maps (layers are face-major).
- `respak-core/src/target.rs`: `TypedTarget<T>` (defaults from `T::default()`, `InvalidOptions` on bad input, `normalized()` for value keys). `png` takes `mips: "first" | n`; `dds` rejects unknown options. `"all"` is `InvalidOptions` until composite outputs (slice 11).
- Tests: `core/tests/options.rs`, `formats/tests/bc_png.rs`, CLI `an_rpak_bc1_texture_converts_to_png_and_honours_mips`.
