# txtr · Respawn texture
**rsx sources:** `game/rtech/assets/texture.h` (`eTextureFormat` :9, `TextureAssetHeader_v8_t/_v9_t/_v10_t` :892-1000, `eTextureMipSorting` :1002, `TextureAsset` :1009), `texture.cpp` (`LoadTextureAsset` :24, mip sizing/placement ~:140-310, `ExportTextureAsset` :1003) · **Games / versions:** Apex Legends, asset versions 8, 9, 10 (Titanfall 2 `txtr` is a different, non-rpak path, not covered) · **Priority hint:** high, **built** (this file documents what respak implements, not a plan; written from the respak code and tests, rsx cited for the origin only).

## Summary
A `txtr` is a 2D texture (optionally an array) with a block-compressed or plain pixel format and a mip chain. The smallest mips sit in the pak's CPU data; larger ones are streamed from a mandatory starpak and an optional starpak. respak exposes it as a `Bitmap` plus `Metadata`, and converts it to `dds` (raw blocks kept as is) or `png` (decoded to RGBA8) through the shared bitmap converters.

## Where it lives
- Container: rpak. FourCC stored and matched as `txtr` (file order bytes `b"txtr"`). respak extension `.txtr`; rpak volume entry `0x<GUID 16 hex upper>.txtr` (e.g. `c.rpak/0x000000000000ABCD.txtr`), plus a name entry when named (see below).
- Name: **stored**, header name page pointer, NUL-terminated string read from `.mem` (max 512 bytes, empty/no NUL = no name). respak prepends `texture/` unless the name already starts with it (case-insensitive). Unnamed (null or `0` pointer) -> `Metadata::name() == None`; guid always comes from the rpak asset entry (`asset.entry.guid`), never the v8 header's guid field.
- Streamed data: mandatory starpak entry (`asset.stream`) and optional starpak entry (`asset.opt`), both resolved by the rpak asset (starpak reference + offset); no separate `.stream`/`.opt` naming in respak, the volume hands over the paths.
- Registration (`respak-respawn/src/lib.rs:139`): format `txtr`, label "Respawn texture", `from("rpak.asset", TxtrRead)`, extension `txtr`, provides `Bitmap` and `Metadata`.

## Versions and detection
- `TxtrRead::score` = `detect_kind(source, b"txtr")` -> **score 10** when the `rpak.asset` kind is `txtr`, with `Version::new(asset.entry.version)` (major = asset version, no minor). Nothing else is inspected at scoring time.
- `TxtrHeader::parse`: versions 8, 9, 10 only; any other version -> `UnsupportedVersion("txtr", "<n>")`. Header must be exactly 56 bytes, else `Corrupt`.
- Value format id after read: `txtr@<version>`.
- Further `UnsupportedVersion`s (header parses, content not): `swizzle != 0` (console layout, message "swizzle N, console layout"); v10 with `unkMipLevels != 0`; pixel format index outside the table below (wide integer, ASTC, depth, etc.); Snowflake-compressed streamed mips (at `read_mip` time); compressed streamed mips of an array texture.

## Layout
Little-endian, 56 bytes (`HEADER_LEN`), page pointers relocated by the rpak volume to `.mem` offsets (null = `u64::MAX` or 0). Offsets as respak reads them (`TxtrHeader::parse`); rsx field names in brackets.

| field | v8 | v9 | v10 |
|---|---|---|---|
| (guid, u64; ignored, entry guid is used) | 0x00 | - | - |
| name (page ptr -> C string) | 0x08 | 0x00 | 0x00 |
| width, u16 | 0x10 | 0x0A | 0x0A |
| height, u16 | 0x12 | 0x0C | 0x0C |
| (depth, u16; not read) | 0x14 | 0x0E | 0x0E |
| format (`eTextureFormat`), u16 | 0x16 | 0x08 | 0x08 |
| (dataSize, u32; not read) | 0x18 | 0x14 | 0x14 |
| swizzle, u8 (must be 0) [v8 `unk_1C`, v9/10 `unk_12`] | 0x1C | 0x12 | 0x12 |
| array size, u8 (0 treated as 1) | 0x1E | 0x10 | 0x10 |
| optional (opt-starpak) mips, u8 | 0x1D | 0x1A | 0x1A |
| permanent mips (in `.mem`), u8 | 0x21 | 0x18 | 0x18 |
| streamed (mandatory starpak) mips, u8 | 0x22 | 0x19 | 0x19 |
| unknown mips, u8 (must be 0) | - | - | 0x1C |
| codec bits, u16 | - | 0x1C | 0x1E |
| compressed sizes, `u16[7]` | - | 0x1E | 0x20 |

- Not read (rsx has them): `layerCount`/resource flags (v8 0x1F, v9/10 0x11; bit 1 = cubemap in the game), `usageFlags` (v8 0x20, v9/10 0x13), `eTextureType` (v9/10 0x1B), `depth`.
- Codec bits: 2 bits per **streamed** mip, bit pair `2*shift` (shift 0 = largest mip of the whole texture): 0 none, 1 RTech, 2 Snowflake, 3 Oodle. v8 = 0.
- Compressed size `compressed_blocks[shift]` = stored size in 4096-byte units minus one (index = shift, 7 entries only).
- Format index -> `PixelFormat` (`pixel_format`, from rsx `s_PakToDxgiFormat`/`eTextureFormat` order):

| idx | format | idx | format |
|---|---|---|---|
| 0 / 1 | Bc1 / Bc1Srgb | 12 / 13 | Bc7 / Bc7Srgb |
| 2 / 3 | Bc2 / Bc2Srgb | 14 | Rgba32F |
| 4 / 5 | Bc3 / Bc3Srgb | 20 | Rgba16F |
| 6 / 7 | Bc4 / Bc4Snorm | 31 / 32 | Rgba8 / Rgba8Srgb |
| 8 / 9 | Bc5 / Bc5Snorm | 41 | R32F |
| 10 / 11 | Bc6H / Bc6HSigned | 44 | Rg8 |
| | | 48 / 49 / 53 | R16F / R16 / R8 |

Everything else (e.g. 15-19 R32/RGB32 int, 21-25 RGBA16 int/norm, 28-30 10-bit/11-bit, 33-40, 42-43, 45-47, 50-52 int/snorm, 54+ A8/shared-exp/depth/ASTC 62+) is unmapped -> `UnsupportedVersion("<ver> (pixel format #N)")`.
Sizes: BC = `ceil(w/4)*ceil(h/4)*{8 (BC1, BC4) | 16}`; plain = `w*h*bytes_per_pixel` (`PixelFormat::mip_len`).

## Data outside the header
Total mips `T = permanent + streamed + optional` (must be 1..=16 and `permanent >= 1`, else `Corrupt`). Mip shift `s` (0 = largest) has size `max(1, w >> s) x max(1, h >> s)`.
- **Permanent** (`.mem`, at `asset.data_mem_offset`): shifts `streamed+optional .. T`, stored **smallest first**; each mip is `align16(len) * array` bytes (`max(1, ...)*16`), array slices back to back, only `len` bytes of each slice are pixels. Must fit inside `.mem` else `Corrupt("txtr mips run past the end of the pak's pages")`.
- **Mandatory starpak entry**: shifts `optional .. optional+streamed`. v8: smallest first; v9/v10: **largest first**. Uncompressed stride `align16(len)` per slice, compressed stride `(compressed_blocks[shift] + 1) << 12`; each mip occupies `stride * array`, consecutive.
- **Optional starpak entry**: shifts `0 .. optional`, same ordering rules.
- Compression (streamed only, per mip via codec bits): **RTech** (`rtech::decompress(block, 0, Some(len))`), **Oodle** (`oodle::decompress(block, len)`; the whole padded block is passed, trailing zeros ignored), **Snowflake** not implemented. Compressed array textures unsupported (rsx asserts arrays are not streamed).
- Entry size check: if the laid-out mips exceed the entry's length -> `Corrupt("... do not fit the starpak entry (N bytes)")`.

## Dependencies
Only the starpak entries above. No guid links to other assets: `Metadata::dependencies()` is empty. Missing data:
- `streamed > 0` and no mandatory starpak reference, or the file/entry is missing -> `MissingDependency { item: <name or "(unnamed)">, file }` (read fails).
- `optional > 0` and the optional starpak is absent/unavailable (`NotFound` or `MissingDependency` from stat) -> `Cx::warn("texture <name>: N optional mip(s) are skipped: <why>")`; the bitmap starts at shift `optional` (width/height `>> optional`, `mip_count = T - optional`). Other errors propagate.
- Materials (`matl`) reference textures by guid; that link lives in `matl`.

## Decoding
```
h      = parse(entry.version, header 56 bytes)          // checks above
fmt    = pixel_format(h.format) or Unsupported
mem    = permanent mips, shifts [streamed+optional, T) in reverse (smallest first), stride align16(len)
if streamed>0:  mandatory entry must exist; stat length; layout_entry(shifts optional..optional+streamed)
if optional>0:  optional entry usable? layout_entry(shifts 0..optional) : warn + first = optional
sort all by shift ascending -> level 0 = largest available
info   = { w>>first, h>>first, depth 1, array_len, faces 1, mip_count T-first, fmt }
read_mip(level, layer): level < mip_count and layer < array_size else NotFound;
   offset = mip.offset + stride*layer; uncompressed -> ranged read of `len` bytes (padding skipped);
   compressed -> read `stride` bytes at offset, decode to `len` bytes.
```
Reads are lazy and ranged (`volume.read_with(path).range(..)`), only the requested mip is touched.

## Output
- Capabilities: `Bitmap` (`BitmapInfo { width, height, depth: 1, array_len, faces: 1, mip_count, format }`, `read_mip(level, layer)` returns raw pixels/blocks in `info.format`) and `Metadata { name, kind "txtr", guid, dependencies [] }`.
- `dds` (`respak-formats`, `DdsEncode`, from any `Bitmap`): all mips of every layer, layers outer / mips inner, BC copied verbatim. Single-layer Rgba8/Bgra8 gets a legacy A8B8G8R8/A8R8G8B8 header, everything else a DX10 header (Texture2D); no options (`DdsOptions {}`). Cube maps and volumes unsupported (not produced by `txtr` anyway). Every `PixelFormat` that `txtr` can produce has a DXGI mapping.
- `png` (`PngEncode`): one mip decoded to RGBA8 (option `mips`: `"first"` default, a level number, or `"all"` -> folder of `mip<N>.png` / `layer<L>_mip<N>.png`); layer 0 only otherwise (warning for arrays). Decode via `bc::decode_rgba8` (`bcdec_rs`): BC4/R8/R16 grey `(v,v,v,255)`, BC5/RG8 `(r,g,0,255)`, floats clamped 0..1 (HDR lost), signed shifted, sRGB and linear identical, no colour-space tag. Formats `decode_rgba8` cannot convert -> `UnsupportedVersion("png", "pixel format ...")`.
- rsx exports DDS / PNG / TGA etc. plus a metadata JSON (`ExportTextureMetaData` :850, includes `resourceFlags`); respak has no metadata JSON for `txtr`.

## rsx quirks
- rsx supports console swizzles (PS4, Switch `#ifdef`) via `s_MipAligment`; respak rejects `swizzle != 0`.
- rsx keeps `compressedBytes` etc. in a single `TextureAsset`; respak splits `layout_entry` per store. v8's own "legacy" sort (`SORT_BOTTOM_TOP`) vs v9/10 `SORT_MIXED` is reproduced by "v8 smallest first / v9+ largest first in starpak, smallest first in `.mem`".
- Header doc in `txtr/mod.rs` still says streamed mips are "slice 10" and the `Bitmap` exposes permanent mips only; the code now handles streamed/optional/RTech/Oodle (stale comment, not behaviour).
- The `array_size` 0 -> 1 rule and `permanent == 0` -> `Corrupt` are respak choices.

## Reuse in respak
`rpak.asset` (`read_header`, `data_mem_offset`, `stream`, `opt`, `volume`), `detect_kind`, `rtech`/`oodle` codecs, `Reader`, shared `PixelFormat`/`Bitmap`/`BitmapBuf`. `uiia`/`uimg` also provide `Bitmap`; `matl` links textures by guid. Source: `crates/respak-respawn/src/assets/txtr/{mod,read}.rs`.

## Tests
Synthetic (`crates/respak-respawn/tests/txtr.rs`, `tests/starpak.rs`, helpers `build_tex_pak`/`TexSpec` in `respak-respawn-testing/src/texture.rs`): every header version (pak v8/7/6 x tex v8/9/10), permanent mip ordering and ranged reads, missing mandatory starpak -> `MissingDependency`, array slices back to back, missing name, unsupported/broken cases (swizzle, ASTC, unknown format -> unsupported; zero permanent, zero width, 40 mips, truncated data -> `Corrupt`), unknown asset version 11 -> `UnsupportedVersion`, headers served with pointers as `.mem` offsets, pointer to missing page -> `Corrupt`. Starpak: full chain (optional + streamed + permanent), missing optional starpak falls back with a warning, missing mandatory/TOC entry -> `MissingDependency`, oversized mips -> `Corrupt`, Oodle block decodes to its length, broken Oodle -> `Corrupt`, Snowflake -> `UnsupportedVersion`, huge starpak reads only the TOC and requested ranges. Patch (`patch.rs`) and CLI (`material_cli.rs`, `names.rs`) cover listing/names of `.txtr` entries. Real-pak test (`rpak/real.rs`, needs game data, not run here): all `txtr` of a pak are recognised and named; one pak lists 11 textures.
Edge cases not covered: RTech-compressed streamed mip, v10 `unkMipLevels != 0`, v9/v10 largest-first stream with array, cube maps.

## Open questions
- Cube maps (`layerCount & 2`) and `depth`: ignored; `faces` is always 1. Needs a real cubemap sample.
- Snowflake (codec 2) streamed mips, compressed arrays, swizzled (console) textures, v10 `unkMipLevels`: unsupported, no real files seen.
- Unmapped formats (R10G10B10A2, R11G11B10F, R9G9B9E5, depth, ASTC, int/snorm variants) would need new `PixelFormat` variants and DDS/PNG mappings.
- No metadata/JSON target (usage flags, resource flags) for `txtr`; wanted?
