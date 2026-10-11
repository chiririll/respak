# 08 · Slice: rpak texture → DDS (permanent mips)
**Status:** done · **Depends on:** 07 · **See:** PROJECT.md §4, §5.4, §6, §12

## Goal
```
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"dds"}'
```
writes `texture/foo.dds`, which the user opens in a viewer. It contains only the mips stored inside the pak; no starpak is read yet. This is the first real Respawn asset through the full chain from §4.

## rsx review
(Subagent sweep of rsx `assets/texture.{h,cpp}`, `cpakfile.{h,cpp}`, `dx.cpp`; the gate was delegated by the user.)

**What it does.** The texture version is the per-asset version in the pak's asset entry (8, 9, 10; any other is `UnsupportedVersion`). Every header is 56 bytes. v8: guid@0, name ptr@8, width u16@0x10, height@0x12, depth@0x14, format u16@0x16, optStreamedMips u8@0x1D, arraySize u8@0x1E, layerCount(flags)@0x1F, usage@0x20, permanentMips@0x21, streamedMips@0x22, swizzle=unk_1C@0x1C. v9: name ptr@0, format u16@8, width@0xA, height@0xC, depth@0xE, arraySize@0x10, layerCount@0x11, swizzle@0x12, usage@0x13, dataSize u32@0x14, permanent@0x18, streamed@0x19, opt@0x1A, type@0x1B, compTypePacked u16@0x1C, compressedBytes[7] u16@0x1E. v10: as v9 to 0x1A, then unk@0x1B, unkMipLevels@0x1C (must be 0), type@0x1D, compTypePacked@0x1E, compressedBytes[7]@0x20. Pixel format enum 0..=91: 0-13 BC1,BC1s,BC2,BC2s,BC3,BC3s,BC4u,BC4s,BC5u,BC5s,BC6Hu,BC6Hs,BC7,BC7s (8-byte blocks for BC1/BC4, else 16), 14-61 uncompressed, 62-89 ASTC (no DXGI), 90 D24S8, 91 unknown. Mips: `total = opt + streamed + permanent`; shift i = 0 is the largest; the opt mips are the largest shifts (`i < opt`), then streamed (`i < opt + streamed`), the smallest are permanent. Each mip's tight size is `blocks_w * blocks_h * bytes`, stored as `align16(size)` per array slice. Permanent mips sit in the asset's CPU data (`data` page pointer) smallest first. Streamed/opt: v8 smallest first, v9/v10 largest first (mixed sort). Compression only on streamed/opt mips: codec = `(compTypePacked >> 2*i) & 3` (0 none, 1 RTech pak, 2 Snowflake, 3 Oodle), on-disk size `(compressedBytes[i] + 1) << 12`, decoded size is the tight size. Name: header name pointer, else the cache db, else `texture/0x<GUID>`.

**Problems (not ported).** Format index only asserted (`>= 91` crashes); `swizzle` indexes a table unchecked (console tiling is not ported; non-zero is rejected); `compressedBytes` has 7 entries but mips up to 8 are possible; streamed array textures asserted away; the Oodle path passes the wrong buffer lengths and falls back to "uncompressed" on error; sub-4 BC mips need one block; `unkMipLevels` ignored in release; ASTC order is unconfirmed.

**Fit.** `txtr` is read from `rpak.asset` (score from kind `txtr` and a supported version); native `TxtrAsset` with a `Bitmap` over the mips; permanent mips via ranged reads of `.mem`; mip order in the `Bitmap` is largest first (DDS order). Streamed mips are slice 10. A 4096-padded compressed mip needs a codec edge (RTech yes; Snowflake and Oodle not supplied).

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

## Decisions (the user delegated the gate)
- Slice 08 serves permanent mips only; a texture with streamed mips writes just the permanent chain with a `tracing::warn!` (the DDS is then a small, valid mip chain). Streamed mips arrive in slice 10.
- Only the PC layout (`swizzle == 0`) is supported; others are `UnsupportedVersion`. Depth must be 1.
- The volume serves `.mem` (the flat decompressed pages) and the header relocated; relocation rewrites each pointer slot to `.mem` offsets (the pointer table is sorted; one pass).
- Output path default: the path inside the innermost container with the target's extension.

## Result
Checked locally on `ui.rpak` (v8, RTech): all 11 `txtr` assets (v8 headers, BC7 sRGB and R8, up to 8192x8192, one permanent mip each) convert to DDS; the files are exactly header + the mip chain sizes the header implies (e.g. 8192x8192 BC7 = 67,108,864 data bytes). Not checked: that the pixels look right in a viewer (no viewer here; the demo is yours), v9/v10 headers and multi-mip textures (synthetic tests only), the GUID hash (the cached UI textures have null names; an ignored test checks it on any pak that does have names).

Changes against the plan: the volume does not list `.mem` (it resolves); `rpak.asset` carries the pak volume and the header path instead of `FileData` values; the CLI binary is now called `respak`; `respak-respawn` has a `testing` feature with synthetic pak builders (used by its own tests and the CLI tests, which are the only place both plugins meet); the default output path is the path inside the innermost container (`0x...dds`, not the texture name, until names are readable); errors raised by a volume now pass through OpenDAL unchanged.

## Tests
Synthetic paks with a txtr asset (v8, v9, v10 headers, BC1/BC3/RGBA8 data) generated in tests; DDS round trip equals the data; relocation; unsupported swizzle/format; the warning for streamed mips.
