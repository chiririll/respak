# 09 · Slice: BC decoding and target options (rpak texture → PNG)
**Status:** blocked · **Depends on:** 08 · **See:** PROJECT.md §5.2, §5.5

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
