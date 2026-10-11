# 08 · Slice: rpak texture → DDS (permanent mips)
**Status:** blocked · **Depends on:** 07 · **See:** PROJECT.md §4, §5.4, §6, §12

## Goal
```
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"dds"}'
```
writes `texture/foo.dds`, which the user opens in a viewer. It contains only the mips stored inside the pak; no starpak is read yet. This is the first real Respawn asset through the full chain from §4.

## rsx review
(filled in during the sketch, approved at the gate)

## Scope
Texture parsing and pointer relocation are ported from rsx (`assets/texture.{h,cpp}`, `cpakfile.cpp`). Review it first, then port it (PROJECT.md §12).

**respak-respawn:**
- The rpak volume serves the asset header at `texture/foo.txtr` and the decompressed pages at `.mem`, with pointers relocated into `.mem` offsets (§6). It has no knowledge of asset kinds.
- `rpak.asset` gains the `header` / `mem` / `stream` / `opt` files.
- `txtr`: read from `rpak.asset`. The score comes from the entry's kind and version, without detecting them again. Native `TxtrAsset`, all versions and variants rsx supports. `Bitmap` with `read_mip` for permanent mips. `txtr` reads the `rpak.asset` data type but never parses rpak structures.
- Per-mip compression, if a version uses it: the `compressed → blob` codec edge.
- Streamed mips are missing: write only the permanent mips and log a warning.

**respak-formats:** the `dds` encoder copies BC blocks as they are. Pixel formats come from `Bitmap`.

**Core:** default output path = the path inside the innermost container with the target's extension.

## Demo
The user converts real textures and checks them in a DDS viewer.
