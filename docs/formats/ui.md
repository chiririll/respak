# ui · UI layout (RUI)
**rsx sources:** `game/rtech/assets/ui.{h,cpp}` (`UIAssetHeader_t`, `UIAssetArg_t`, `UIAssetArgCluster_t`, `UIAssetStyleDescriptor_t`, `UIAssetMapping_t`, `LoadUIAsset`, `PreviewUIAsset`, `ExportUIAsset`, `ExportUIAsset_CPP`), `game/asset.h` (`AssetType_t::UI`, path prefix) · **Games / versions:** Titanfall 2 (asset versions 29, 30) and Apex Legends (39, 40, 42) · **Priority hint:** low (S). rsx only exports the argument table, not the render program.

## Summary
A RUI element: a compiled UI script. The asset carries the element size, the table of script arguments (type, name, default value) and opaque render data (style descriptors, render jobs, transform data, mappings). rsx understands only the argument table and the default-value block. A user would convert it to a text/JSON description of the arguments (name, type, offset, default); the render program cannot be decoded (meaning unknown).

## Where it lives
rpak asset, FourCC `ui` (stored `75 69 00 00`, rsx `AssetType_t::UI = MAKEFOURCC('u','i',0,0)`; binding `'\0iu'`), extension `ui`. Pak name `<guid>.ui` unless named. Name: header `name` pointer (NUL string, e.g. `ui/<name>.rpak` in rsx, i.e. rsx prefixes `ui/` and appends `.rpak`; respak should use `ui/<name>`); if `name` is null rsx falls back to its guid-name cache (`SetAssetNameFromCache`). Asset header is in the pak's header pages; everything else (default values, args, names, clusters, descriptors, render jobs) is reached through page pointers. No starpak data.

## Versions and detection
rsx accepts asset versions 29, 30 (Titanfall 2) and 39, 40, 42 (Apex) with the same struct (`LoadUIAsset`); any other version asserts and the asset is skipped. rsx does not vary the layout by version (its source comments hint at differences, see quirks). The only version dependence in rsx is `UI_ARG_TYPE_UIHANDLE` (below): version > 30 stores a 4-byte id, otherwise a string pointer. Proposed: score kind `ui` = 10, `Version::new(asset.version)`; versions 29/30/39/40/42 accepted, else `UnsupportedVersion { ui, <v> }`. Format id `ui@<version>`.

## Layout
Little-endian, 64-bit pointers. All pointers are page pointers (relocated by the rpak volume). `UIAssetHeader_t` (offsets computed from the struct, rsx has no `static_assert` on the total; `0x68` bytes if no trailing fields exist; confirm against `asset.data().header_size`):

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | page ptr | `name` | element name, NUL string; may be null |
| 0x08 | 8 | page ptr | `defaultValues` | block of `defaultValuesSize` bytes, argument default values (also the start of the RUI data struct) |
| 0x10 | 8 | page ptr | `transformData` | opaque transform bytes (size/meaning unknown) |
| 0x18 | 4 | f32 | `elementWidth` | element width |
| 0x1C | 4 | f32 | `elementHeight` | element height |
| 0x20 | 4 | f32 | `elementWidthRatio` | |
| 0x24 | 4 | f32 | `elementHeightRatio` | |
| 0x28 | 8 | page ptr | `argNames` | name string table, NUL-terminated strings, indexed by `UIAssetArg_t.nameOffset`; may be null (stripped) |
| 0x30 | 8 | page ptr | `argClusters` | array of `argClusterCount` x `UIAssetArgCluster_t` (0x12 bytes each, packed as listed) |
| 0x38 | 8 | page ptr | `args` | array of `argCount` x `UIAssetArg_t` (8 bytes each) |
| 0x40 | 2 | i16 | `argCount` | |
| 0x42 | 2 | i16 | `unk_10` | "count", unknown |
| 0x44 | 2 | u16 | `ruiDataStructSize` | total size of the runtime RUI data struct (>= `defaultValuesSize`) |
| 0x46 | 2 | u16 | `defaultValuesSize` | size of the `defaultValues` block |
| 0x48 | 2 | u16 | `styleDescriptorCount` | |
| 0x4A | 2 | u16 | `unk_A4` | unknown |
| 0x4C | 2 | u16 | `renderJobCount` | |
| 0x4E | 2 | u16 | `argClusterCount` | |
| 0x50 | 8 | page ptr | `styleDescriptors` | `styleDescriptorCount` x `UIAssetStyleDescriptor_t` (0x44 bytes) |
| 0x58 | 8 | page ptr | `renderJobs` | opaque render program bytes, size unknown |
| 0x60 | 8 | page ptr | `mappingData` | `UIAssetMapping_t` (0x10 bytes) |

rsx never reads `transformData`, `renderJobs`, `mappingData` or `renderJobCount`; their layout is unknown.

`UIAssetArg_t` (8 bytes):

| Off | Type | Field | Meaning |
|---|---|---|---|
| 0 | u8 | `type` | `UIAssetArgType_t` (below) |
| 1 | u8 | `unk_1` | unknown |
| 2 | u16 | `dataOffset` | byte offset of the value in `defaultValues` / the RUI struct; 0 for type NONE |
| 4 | u16 | `nameOffset` | offset into `argNames` |
| 6 | u16 | `shortHash` | 16-bit hash of the name; used as the name when `argNames` is null |

`UIAssetArgCluster_t` (rsx struct, 0x12 bytes without padding; array stride unconfirmed, see open questions): 0x00 u16 `argIndex`; 0x02 u16 `argCount` (hash table size); 0x04 u8 `hashMul`; 0x05 u8 `hashAdd`; 0x06 u16 `unk_6`; 0x08 u16 `defaultValuesSize` (equals the header field); 0x0A u16 `ruiDataStructSize` (equals the header field); 0x0C 6 bytes gap (unknown). A cluster is the argument-name hash table (hashMul/hashAdd are the hash constants; the hash function itself is not in rsx).

`UIAssetArgType_t` (u8): 0 NONE, 1 STRING (`char*`, 8), 2 ASSET (`char*`, 8), 3 BOOL (4 bytes, Win32 BOOL; rsx reads it as `bool*`), 4 INT (i32), 5 FLOAT (f32), 6 FLOAT2 (8), 7 FLOAT3 (12), 8 COLOR_ALPHA (4 x f32, 16), 9 GAMETIME (f32), 0xA WALLTIME (f32 in `s_typeSizes`, but exported as i64; inconsistent), 0xB UIHANDLE (size differs by game: string pointer r2, 4-byte id r5), 0xC IMAGE (`char*`, 8), 0xD FONT_FACE (size unknown), 0xE FONT_HASH (i32), 0xF ARRAY (size unknown). Export names: ``, `string`, `asset`, `bool`, `int`, `float`, `float2`, `float3`, `Color`, `GameTime`, `WallTime`, `UIHandle`, `Image`, `FontFace`, `FontHash`, `array`.

`UIAssetStyleDescriptor_t` (0x44 bytes, observed Apex S10): 0x00 u16 `type` (0 text, 1 unknown, 2 ellipse); 0x02 `UIColor_t colors[3]` (4 x u16 each, 0x18 bytes); 0x1A 4 bytes gap; 0x1E u16 `fontIndex`; 0x20 u16[4] unknown; 0x28 u16 `textSize`; 0x2A u16 `textStretch`; 0x2C u16[4] unknown; 0x34 16 bytes gap. rsx never reads descriptors.

`UIAssetMapping_t` (0x10): u32 `dataCount`, u16 `unk_4`, u16 `unk_6`, page ptr `data` (f32 array). Unused by rsx.

Pointer-valued defaults (STRING/ASSET/IMAGE, and UIHANDLE on r2) are page pointers inside the `defaultValues` block; they must be relocated and point to NUL strings. The magic float `0xF149F2CA` (-1e30) is a common "unset" placeholder.

## Data outside the header
All data is in the pak's pages (no starpak, no compression). `defaultValues` is `defaultValuesSize` bytes; `argNames` has no stored length (read NUL strings at offsets). Bytes before the smallest non-NONE `dataOffset` (`minArgDataOffset`) are "shader data": unannotated constants used by internal rendering code (no names, no types). Bytes between `defaultValuesSize` and `ruiDataStructSize` are runtime-only (not stored).

## Dependencies
None resolved by rsx. ASSET / IMAGE / UIHANDLE (r5) defaults hold names or ids of other assets (string like `ui_image/...`; r5 UIHANDLE `ui:<hex>`), only exported as text. Expose them as plain strings in JSON; do not link. `rpak.guid` is not needed.

## Decoding
1. Read the header; check version; relocate pointers.
2. Read `args[0..argCount]`; for each non-NONE arg compute the value address `defaultValues + dataOffset` (bounds check `dataOffset + size(type) <= defaultValuesSize`; rsx only asserts `dataOffset < defaultValuesSize`).
3. Name: `argNames ? cstr(argNames + nameOffset) : hex(shortHash)`.
4. Value by type: STRING = deref ptr, NUL string; ASSET/IMAGE = deref ptr string; UIHANDLE = version > 30 ? u32 hex : string ptr; BOOL = nonzero u32 byte; INT i32; FLOAT/GAMETIME f32; FLOAT2/3 and COLOR 2/3/4 f32; WALLTIME i64 (rsx); FONT_HASH u32 hex; FONT_FACE and ARRAY unsupported (rsx prints `UNSUPPORTED`).
5. `minArgDataOffset` = min `dataOffset` over non-NONE args (0xFFFF if none).

## Output
rsx export setting `TXT` (`<name>.txt`): per cluster `// ArgCluster <i> Hash Consts: <mul>/<add>`, then one line per non-NONE arg `"<typeName> <name>@<dataOffset>\t<value>"` (string quoted with `EscapeString`; asset/image/uihandle as `$"..."`; bool `true`/`false`; floats comma-separated), last line `// Shader Data Size: <minArgDataOffset>`. Setting `C++` (`<name>.cpp`): a `struct <name>_s` with `SHADER_DATA(n)` for the leading unannotated bytes, one member per arg sorted by offset (name or `var_<hash>_<index>`), `char unk_data[ruiDataStructSize - defaultValuesSize]`, and `static_assert(offsetof(...))` lines.

Proposed respak: capability `Metadata` (name, kind `ui`, guid) plus a text/JSON edge `ui -> ui.json`. JSON: `name`, `guid`, `version`, `width`, `height`, `widthRatio`, `heightRatio`, `ruiDataStructSize`, `defaultValuesSize`, `clusters` (`hashMul`, `hashAdd`, `argCount`), `args` (`index`, `name` or null, `hash`, `type`, `offset`, `value`), `shaderDataSize`, `styleDescriptorCount`, `renderJobCount`. A TXT edge matching rsx is optional.

## rsx quirks
`UIAssetHeader_t` comments carry stray offsets (`unk_A4`, `unk_10`) from a different (probably 32-bit or IDA) layout; do not trust them. `s_typeSizes` has `walltime` as 4 bytes while export reads 8; UIHANDLE and BOOL (`sizeof(BOOL)`) sizes are game-dependent. `ExportUIAsset_CPP` dereferences `argClusters` unconditionally and writes a hash comment only when `argNames` is null. `std::sort` comparator in the preview is not a strict weak ordering. Asserts only on `dataOffset < defaultValuesSize`, no bounds checks on strings: respak maps to `Corrupt`. Hash function for `shortHash` / cluster tables is not implemented in rsx. FONT_FACE, ARRAY unsupported.

## Reuse in respak
`rpak.asset` (`detect_kind`, `rpak_asset`, page pointer reading), `txtr::read_name`-style NUL string helper, JSON/text bundle output as in `uiia`. New: arg table parser, value formatter.

## Tests
Synthetic: build a pak with one `ui` asset (0x68 header, 2-3 args of different types, one STRING with a page pointer, names table; one with `argNames = null`; one with a NONE arg). Edge cases: version 28/41 -> `UnsupportedVersion`; `dataOffset` past `defaultValuesSize` -> `Corrupt`; `argCount` 0; `name` null. Real files: any `ui` asset in the Apex `ui_*.rpak` / `common.rpak` (no Steam runs without permission; list candidates from the rsx cache first).

## Open questions
1. Asset header size per version (0x68 assumed) and whether Titanfall 2 (29/30) matches; cluster stride (0x12 vs padded 0x14).
2. Is it worth decoding anything beyond the arg table (render jobs, transform data, mappings, style descriptors are unknown)?
3. Output: JSON only, or also rsx-compatible TXT / C++?
4. WALLTIME and UIHANDLE sizes per game.
