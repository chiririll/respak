# shdr · Shader (rpak asset, standalone)
> **Verified against** Apex depot 1172471 `paks/Win64/common.rpak` (2459 `shdr`), `common_early.rpak` (119) and `startup_heightmap_renderer.rpak` (15): **only version 21 with a 48-byte header occurs** in the current build. The v21 findings below are real data; v8-v17 layouts remain rsx-only and unverified (no such asset exists in the current build; v19 not seen either).
**rsx sources:** `game/rtech/assets/shader.{h,cpp}` (`LoadShaderAsset`, `PostLoadShaderAsset`, `ShaderAsset` constructors, `ExportShaderMetaData`, `ExportRawShaderAsset`, `ConstructMSWShader`, `ExportMSWShaderAsset`, `ResourceBindingFromDXBlob`, `ConstBufVarFromDXBlob`, `Shader_CreateInputLayoutFromFlags`), `core/render/dxshader.h` (`eShaderType`), `core/shaderexp/multishader.h` (MSW container), `core/filehandling/validation.cpp` (known versions / header sizes) · **Games / versions:** Titanfall 2 (v8), Apex Legends (v12-v21; v8 is the R2 layout) · **Priority hint:** S/M. The DXBC blobs are the whole payload; the useful output is `.fxc` files plus a small JSON. Low value for most users, needed only for modding / debugging shaders. Respak support: **partially** (see "Reuse in respak").

Related: [shds](shds.md) (shader set, references two `shdr`), [matl](matl.md) (contains the already-built `shds` -> `shdr` -> DXBC helper).

## Summary
A compiled DirectX shader program (vertex, pixel, geometry, hull, domain or compute). One asset holds a list of **variants**: several DXBC (SM5) blobs of the same shader compiled for different feature combinations (dither fade, instancing, ...), each with a pair of vertex-input-layout flags. Later variants may be references to earlier ones (dedupe). A user would convert it to one `.fxc` file per variant (raw DXBC, loadable by any D3D disassembler such as `fxc /dumpbin` or RenderDoc) plus a JSON sidecar with type, feature bytes, input flags and reference indices. respak today reads only variant 0's bytes, to get texture slot names for a material.

## Where it lives
- Container: rpak asset. The header goes in the header page; the variant table and the DXBC blobs are in the asset's CPU/data page (`data_mem_offset`); no starpak.
- FourCC as stored: bytes `shdr` (rsx writes the constant as `'rdhs'`, i.e. the little-endian u32 of the bytes `s h d r`; `AssetType_t::SHDR = MAKEFOURCC('s','h','d','r')` in `game/asset.h`). `entry.kind == *b"shdr"`. Extension `shdr`. Virtual path `<pak>.rpak/0x<GUID16>.shdr`.
- Name: the header holds a page pointer to a C string (`name`). rsx turns it into `shader/<name>` and appends `.rpak` if missing (`LoadShaderAsset`). When `name` is null rsx falls back to `SetAssetNameFromCache` (guid -> name cache from outside the pak). In respak the path is guid-based; the stored name is metadata only.
- Export folder in rsx: `shader/` (`s_AssetTypePaths[SHDR] = "shader"`).

## Versions and detection
rsx handles asset versions **8, 12, 13, 14, 15, 16, 17, 19, 21** (`LoadShaderAsset`; `validation.cpp` `SHDR_VERSIONS` lists 8, 12-17, 19, not 21). Anything else: `assertm(false, "unaccounted asset version, will cause major issues!")`, nothing loaded.

| asset version | rsx header struct | header size | notes |
|---|---|---|---|
| 8 | `ShaderAssetHeader_v8_t` | 32 | Titanfall 2 |
| 12 | `..._v12_t` | 32 | Apex early |
| 13 | `..._v13_t` | 40 | Apex S9 |
| 14 | `..._v14_t` | 48 | |
| 15, 16, 17 | `..._v15_t` | 56 | `assertm(headerStructSize == 56)` |
| 19, 21 | `..._v14_t` (reused; "man idfk" in rsx) | 48 (validation lists 19 -> 48; **21 confirmed 48 on real data**) | all 2593 real assets have a data page; **21 is the only version in the current Apex build** |

Proposed respak mapping (when a real `shdr` reader is written): `Detect` = score 10 for any `rpak.asset` of kind `shdr` (as `matl`, `detect_kind`); `Version::new(asset version)` with `minor = 0`. Accept 8, 12, 13, 14, 15, 16, 17, 19, 21 by `(version, header length)`; anything else -> `UnsupportedVersion { format: "shdr", version: "<v> (header of <n> bytes)" }`. Header length per the table; 19/21 with 56-byte headers are unverified (open question).

## Layout
Little-endian, pointers are page pointers (already relocated in the header served by the volume; 0 = null). `eShaderType` is a `uint8_t` enum (`core/render/dxshader.h`): `0 PIXEL, 1 VERTEX, 2 GEOMETRY, 3 HULL, 4 DOMAIN, 5 COMPUTE, 6 Invalid`; values `8` and `9` have been observed in v15 ("unsupported"). rsx treats `type >= 6` as invalid and skips export.

### Header (all versions share the first fields)
| offset | size | field | versions |
|---|---|---|---|
| 0x00 | ptr | `name` (C string, page pointer) | all |
| 0x08 | u8 | `type` (`eShaderType`) | all |

**v8 (32 bytes)**
| offset | size | field |
|---|---|---|
| 0x0C | i32 | `numShaders` (count of variants) |
| 0x10 | ptr | `unk_10` (unknown) |
| 0x18 | ptr | `shaderInputFlags` (-> array of i64, see below) |

**v12 (32 bytes)**
| offset | size | field |
|---|---|---|
| 0x09 | 7 x i8 | feature bytes `unk_9[0..7]` (signed `char` on MSVC) |
| 0x10 | ptr | `unk_10` |
| 0x18 | ptr | `shaderInputFlags` |

**v13 (40 bytes):** v12 plus `0x20: u8[8] unk_20` ("weird", unknown).
**v14 (48 bytes):** v12 plus `0x20: u64 unk_20`, `0x28: ptr unk_28` (called `unk_30` in v15). Used also for v19/v21.

**v15, v16, v17 (56 bytes)**
| offset | size | field |
|---|---|---|
| 0x09 | 9 x i8 | feature bytes `unk_9[0..9]` (struct has `unk_9[9]` then `unk_12[6]`; the 6 bytes at 0x12 are not read) |
| 0x18 | ptr | `unk_18`: sometimes a page pointer ("m_ppShader"), sometimes a **guid** of a parent shader (data-less children) |
| 0x20 | ptr | `shaderInputFlags` |
| 0x28 | u64 | `unk_28` |
| 0x30 | ptr | `unk_30` |

### Variant count (`numShaders`)
- v8: the header's `numShaders` i32.
- v12-v14: `n = unk_9[0]` if it is not `-1`. If `unk_9[0] == -1`, compute from the feature bytes (`b1..b6 = unk_9[1..6]`):
  ```
  v6 = 2 * ((b1 != 0) + 1);  if (!b2) v6 = (b1 != 0) + 1;
  v7 = v6;                   if (b3) v7 *= 2;
  n  = 2 * v7;               if (!b6) n = v7;
  ```
  The 7 feature bytes are also kept as `shaderFeatures` (8-byte buffer, top byte unused).
- v15-v17 (and v19/v21 read through the v14 struct, so they use the v12-v14 formula above; rsx comment `[rika]: definitely different on v16`):
  ```
  n = unk_9[0];
  if (n == -1) { n = (unk_9[1] != 0) + 1;
      if (unk_9[2]) n *= 2;  if (unk_9[3]) n *= 2;
      if (unk_9[4]) n *= 2;  if (unk_9[7]) n *= 2; }
  ```
- No data page (`dataPagePtr == 0`, v15+ only): `numShaders = 0`, `data = null`, `type` kept as is (often an invalid value). rsx never exports these.

### Shader input flags (`shaderInputFlags`)
Page pointer to an array of `i64`. Per variant there are **two** values: `flags[2*i]` and `flags[2*i+1]` (`ExportShaderMetaData`, `ConstructMSWShader`). With a null pointer rsx writes `0x0, 0x0` for every variant. The first value of a vertex shader is the vertex-layout bit set decoded by `Shader_CreateInputLayoutFromFlags` (only used for D3D preview, not needed for export):
| bit | meaning |
|---|---|
| 0 / 1 | POSITION unpacked (`float3`) / packed (`R32G32_UINT`) |
| 4 | COLOR (`R8G8B8A8_UNORM`) |
| 8 / 9 | NORMAL unpacked (`float3`) / packed (`R32_UINT`) |
| 10 / 11 | TANGENT float3 / float4 (no element is created in rsx) |
| 12 | BLENDINDICES (`R8G8B8A8_UINT`) |
| 13 / 14 | BLENDWEIGHT unpacked (`R32G32_FLOAT`) / packed (`R16G16_SINT`) |
| 24 + 4*n, 4 bits each | TEXCOORDn format: 0 none, 1 `R32_FLOAT`, 2 `R32G32_FLOAT`, 3 `R32G32B32_FLOAT`, 4 `R32G32B32A32_FLOAT`, 5 `R16G16_SINT`, 6 `R16G16B16A16_SINT`, 7 `R16G16_UINT`, 8 `R16G16B16A16_UINT`, 9 `R32G32_UINT`; 10-15 undefined in the game (rsx clamps to 0) |

### Variant table (data page, `data_mem_offset`)
`ShaderAssetCPU_t { char* data; int dataSize; }` is the first variant. The table is an array of fixed-stride entries starting at the data page offset (`PostLoadShaderAsset`):
| offset in entry | size | field |
|---|---|---|
| 0x00 | ptr | page pointer to the DXBC blob (null allowed) |
| 0x08 | i32 | length: `> 0` real blob (length in bytes), `< 0` reference to entry `~len` (`!size` in Rust), `0` empty |
| 0x0C | 4 bytes | unknown / padding |
| 0x10 | ptr | **vertex shaders only**: third qword, purpose unknown (rsx comment: "pretty sure this is to fit another pointer", the vertex-layout DXBC per old comments) |

Stride: **16 bytes** (2 qwords) for all types except vertex shaders, **24 bytes** (3 qwords) for `type == Vertex` (`v12 += 3` vs `+= 2` for v12+; `entrySize = 24 / 16` for v8). For v8 negative lengths are clamped to 0 and there are no references; `isNullBuffer` and `isZeroLength` both mean `len == 0`.
The reference index is **in entries** (`~len` is also what rsx writes as `refIndex`); the game comment says `shaderInstances[2 * ~len]`, so the exact index space (entry vs shader) is unconfirmed (open question).

## Verified on real data (v21, 48-byte header)
All 2593 `shdr` assets read are **version 21, header size 48**; types seen are 0 (PIXEL, 1963 in `common.rpak`) and 1 (VERTEX, 496) plus 5 (COMPUTE) in `common_early`; no 2, 3, 4, 8, 9. Every one in `common.rpak` has a data page and its entry 0 is a valid pointer with `len > 0` (2459 of 2459).
- **Header (v21).** `0x00` name pointer: **zero in all samples** (no name stored; names come from a cache). `0x08` u8 `type`. `0x09..0x0F` seven feature bytes: byte 0 is always `0xFF` (-1), bytes 2, 5, 6 always 0, byte 1 is 0/1, byte 3 is 0/1/2, byte 4 is 0/1/2/4. `0x10` pointer to **`n * 24` zero bytes** (runtime slots, one 3-qword record per variant). `0x18` pointer to **`n * 16` bytes: `shaderInputFlags`** (`flags[2*i]`, `flags[2*i+1]` per variant, as rsx exports). `0x20` is **either a pointer or a small integer** (seen 0, 1, 5, 15, 341); when it is a pointer its target is about 10 bytes per variant (a pointer plus 2 bytes); `0x28` is a pointer to **`n` u32 values** (one per variant, e.g. 25, 482 / 483, 92 / 100 / 2052 / 2228, 3194 / 3207 / 3205 / 3218; meaning unknown, not the DXBC instruction count).
- **Variant count (`numShaders`).** Derived from the length of the `shaderInputFlags` array (`n * 16`, checked on all 2459 `common.rpak` shaders): `n = ((b1 != 0) + 1) * (b3 != 0 ? 2 : 1) * (b4 != 0 ? 2 : 1)` where `b1, b3, b4` are the feature bytes at header offsets `0x0A`, `0x0C`, `0x0D` (`unk_9[1]`, `unk_9[3]`, `unk_9[4]`; `unk_9[2]` is always 0 and `unk_9[5..6]` too, so the v15 formula with or without its `unk_9[2]`/`unk_9[7]` terms gives the same result). `unk_9[0]` is always -1, so the `n = unk_9[0]` shortcut never applies. **The v12-v14 formula that rsx uses for v19/v21 is wrong here: it matches only 203 of 2459.** Values of `n` seen: 1 (99), 2 (470), 4 (957), 8 (933).
- **Variant table** (at the data page): `n` entries, stride 16 (stride 24 for `type == VERTEX`; the first 121 vertex shaders of `common.rpak` and those of `common_early` parse consistently). Entry = `{ u32 page, u32 offset, i32 len, u32 x }` where `(page, offset)` is the **stored page pointer** (the table sits in `.mem`, so it is not relocated; resolve it with the page bases). `len > 0`: the pointer leads to a `DXBC` blob (1513 of 1513 checked blobs start with `DXBC`). **A variant whose pointer is `(0, 0)` while `len > 0` is very common**: about half the entries (1429 null versus 1513 valid in the first 600 shaders): these variants are listed with their size but their bytes are not in the pak; entry 0 was never null. The blobs are stored right after the table (entry 0's blob follows the `n * stride` table bytes) and are contiguous.
- **Vertex entries (stride 24).** `x` (offset `0x0C`) is the **byte size of a vertex-input semantic list** and the third qword (`0x10`) is a **pointer to that list**, stored just before the blob. The list is a sequence of `{ u8 nameLength, u8 semanticIndex, char name[nameLength] }` ending with one `0` byte; `x` includes that terminator. Example for `x = 38`: `08 00 "POSITION" 0B 00 "BLENDWEIGHT" 0C 00 "BLENDINDICES" 00`. Names seen: `POSITION`, `NORMAL`, `TEXCOORD`, `COLOR`, `BLENDWEIGHT`, `BLENDINDICES`, `SV_VERTEXID`; all 116 lists parse exactly. This resolves open question 4 (for non-vertex entries `x` is 0).
- **References** (`len < 0`): 15 seen in `common_early`, all with `~len < own index` (they point to an earlier entry). That is consistent with the entry index but cannot tell entry index from shader index (every entry is one variant).
- The `0x18` flags and the feature bytes are per shader; the vertex-layout flags in `flags[2*i]` (for example `0x4030002000D41`) were not decoded.

## Data outside the header
- Page data only: name string, `shaderInputFlags` array, the variant table and the DXBC blobs. No starpak streaming, no compression layer of its own (the rpak volume decompresses pages).
- A DXBC blob (magic `DXBC`) is validated by rsx with `DXBCHeader::isValid()`; the first non-DXBC variant makes `PostLoadShaderAsset` return early (it silently stops post-processing; a quirk, see below).
- DXBC container (all in `shader.h`, byte-exact):
  - `DXBCHeader` (0x20 bytes): `u32 'DXBC'`, `u8[16] hash`, `u16 major, u16 minor`, `u32 containerSize`, `u32 blobCount`; followed by `u32 blobOffset[blobCount]` (offsets from the container start).
  - Each blob: `{ u32 fourcc, u32 size }` + data. `RDEF` = reflection.
  - `RDEFBlobHeader` (0x3C bytes, at blob data): `u32 constBufferCount @0, u32 constBufferOffset @4, u32 boundResourceCount @8, u32 boundResourceOffset @0xC, u8 versionMinor @0x10, u8 versionMajor @0x11, u16 shaderType @0x12 (0x4353 CS, 0x4453 DS, 0x4753 GS, 0x4853 HS, 0xFFFE VS, 0xFFFF PS), u32 compilerFlags @0x14, u32 creatorOffset @0x18 (compiler signature string), u32 id @0x1C ('RD11'), u32 unk[7]`. All offsets are relative to the start of the RDEF blob data.
  - `RDEFResourceBinding` (32 bytes): `nameOffset, type (D3D_SHADER_INPUT_TYPE), returnType, dimension, numSamples, bindPoint, bindCount, flags`, all u32.
  - `RDefConstBuffer` (24 bytes): `nameOffset, constCount, constOffset, constBufSize, type, flags`; `RDEFConst` (40 bytes): `nameOffset, startOffset, size, flags, typeOffset, defaultValue, reserved[4]`; `RDEFType` (36 bytes): `class, type, rows, columns, elements, members, offset, unk` (u16 x8) then `reserved[4], nameOffset` (u32).

## Dependencies
- None as outgoing references from the `shdr` itself, except the **v15+ parent guid** at `unk_18` for data-less child shaders (`#ifdef SHADER_CHILD_HACK` in `PostLoadShaderAsset`: look up the parent by guid, copy its header (keeping the child's name) and use the parent's CPU page). The macro is not defined by default in what was read, so rsx normally exports nothing for them.
- Referenced *by* `shds` (vertex and pixel shader guids, [shds](shds.md)) and indirectly by `matl` through its shader set. Respak resolves with `locate(guid, "shdr", origin_pak, cx)` (`rpak.guid`).

## Decoding
1. `read`: take `rpak.asset` kind `shdr`; pick the header layout by `(version, header length)`; read `name` (page string, optional), `type`.
2. Compute `numShaders` (formulas above). With `dataPage == 0` and version >= 15: zero variants, type may be invalid, skip.
3. Stride = 24 if `type == 1` (Vertex) else 16. For `i` in `0..numShaders`: read `{ptr, len}` at `data + stride*i`. `len > 0` -> resolve `ptr` via `mem_offset_of`, bounds-check, read `len` bytes (max 16 MiB as in `shader::bytecode`); `len < 0` -> reference to entry `!len` (must be `< numShaders`, keep it as a reference, do not copy); `len == 0` -> empty; null ptr with `len > 0` -> "null buffer" (report, do not fail the whole asset).
4. Read the input flags array: `2 * numShaders` i64 values at `shaderInputFlags` (page pointer), or zeros when null.
5. Optionally parse `DXBC`/`RDEF` for `compilerStrings` (compiler signature, `creatorOffset`), texture bindings (`type == 2`, see matl) and constant-buffer variables (`ConstBufVarFromDXBlob`, used by rsx material export, not needed for `shdr` itself).

### Already covered by matl's helper vs new
- **Covered** (`respak-respawn/src/assets/shader.rs::bytecode`, `codec/dxbc.rs::texture_slots`): reading data-page entry 0 with stride 16, `size < 0` reference to entry `!size` (index 1..=1024), 16 MiB cap, bounds check against `.mem`, DXBC `RDEF` bound-resource scan (texture bind points only, 32-byte entries, no SM5.1).
- **New for `shdr`**: header parse per version (name, type, feature bytes, input flags pointer, variant count), vertex stride 24, iterating all variants (not just entry 0), input flags, validated `numShaders`, DXBC-level `compilerStrings` and constant-buffer reflection if wanted, the output formats below, and registering `shdr` as a format (`Detect` + `FormatId("shdr")`).

## Output
rsx has two export settings (`InitShaderAssetType`: `{"Raw", "MSW"}`):
- **Raw** (`ExportRawShaderAsset`): `<export>/shader/<filename>.json` (`ExportShaderMetaData`) plus `<stem>_<idx>.fxc` for every variant with a non-null buffer and `size > 0` (idx = variant index; references and empty variants write nothing). The exported name keeps the full path of the asset name when `exportPathsFull` is set, otherwise `shader/<filename>`.
  ```json
  { "type": "PIXEL", "name": "shader/....rpak", "features": "<hex of 7 feature bytes, no 0x>",
    "inputFlags": ["0x..", "0x.."],      // two entries per variant, in order
    "refIndices": [65535, 0, ...] }      // 65535 = not a reference, else the referenced entry index
  ```
  (`features` is printed as an uppercase hex `u64` of the first 7 bytes; leading zeros are not padded.)
- **MSW** (`ExportMSWShaderAsset`, `multishader.h`): the MultiShaderWrapper file `.msw` (a rsx/repak custom container, version 3), `fileType = 0` (SHADER):
  | offset | size | field |
  |---|---|---|
  | 0 | 3 | magic `M S W` (`'M' | 'S'<<8 | 'W'<<16`) |
  | 3 | 1 | version = 3 |
  | 4 | 1 | file type: 0 shader, 1 shader set |
  | 5 | 16 | `MultiShaderWrapper_Shader_t` (packed): `u64` low 56 bits `shaderFeatures`, high 8 bits `numShaderDescriptors`; `u32 nameLength` (incl. NUL, 0 if no name); `u32 nameOffset` (absolute) |
  | 21 | 32 x n | `MultiShaderWrapper_ShaderDesc_t` per variant: `u32 a, u32 b` + `u64 inputFlags[2]`. Standard buffer: `a = length, b = offset from the start of this descriptor`; reference: `a = referenced index, b = 0`; null entry: `a = b = 0xFFFFFFFF` |
  | then | ... | buffers back to back, then the NUL-terminated name |
  The shader *type* is not stored in the MSW; the reader sets `INVALID`.

Proposed respak targets (not implemented): format `shdr.json` ("Raw" equivalent; bundle: `<name>.json` + `<name>_<idx>.fxc`, with the same field names and `refIndices`) and capability `Metadata` (name, kind `shdr`, guid, dependencies = parent guid for v15+ children). A raw `fxc`/`dxbc` blob capability for variant 0 is a new capability if wanted; not needed otherwise. MSW is an rsx-specific container; recommend skipping it (open question). Round-trip / import: none, see [shds](shds.md) for repak's expectations on MSW.

## rsx quirks
- `LoadShaderAsset` asserts on unknown versions and returns without loading; v15 header size is asserted but v19/v21 (reusing the 48-byte struct) are not validated.
- v15 constructor does not fill `shaderFeatures` (stays zero), so Raw/MSW export writes `features = 0` for v15-v17 even though `unk_9` carries the bytes. v12-v14 do copy them.
- v12 constructor does not zero-initialise `shaderFeatures` (garbage in the 8th byte); the exporter masks with `0x00FFFFFFFFFFFFFF` for that reason.
- `numShaders` for v19/v21 uses the v12-v14 formula because rsx reuses the v14 struct; **real v21 data disagrees** (the formula matches 203 of 2459): the count follows the v15-style rule on feature bytes 1, 3 and 4 (see *Verified on real data*).
- `PostLoadShaderAsset` returns (not `continue`s) on the first buffer that is not DXBC, so remaining variants lose their compiler strings; harmless for export.
- `ResourceBindingFromDXBlob` / `ConstBufVarFromDXBlob` use only `cpu->data` (variant 0) and do not follow references; respak's `shader::bytecode` additionally follows entry 0 -> `!size` (kept).
- The preview indexes input flags with `(3 - numFlagsPerShader) * i`, which does not match the exporter's `2*i` for vertex shaders; trust the exporter.
- The vertex input layout is built from `inputFlags[0]` of entry 0 only, with `float` offsets from magic shift tables; irrelevant for file export (drop).
- Data-less children (no CPU page, invalid type, guid in `unk_18`) are skipped on export (`type >= Invalid` -> `Log` + `return false`).
- `eShaderType` values 8 and 9 (v15) are unsupported/unknown.
- Keep: variant order, 2-flags-per-variant, `~len` reference encoding. Drop: D3D device creation, input layout creation, MSW container unless asked.

## Reuse in respak
- Existing: `respak-respawn/src/assets/shader.rs` (`bytecode(asset: &RpakAsset)`, `pixel_shader_guid`) and `codec/dxbc.rs` (`texture_slots`); `rpak.asset` (`read_header`, `read_mem`, `mem_offset_of`, `data_mem_offset`); `Reader`; `rpak.guid` `locate`; `BundleData` for multi-file output; `respak-respawn-testing` builder `PakBuilder::shdr` (v12, 32-byte zero header, one 16-byte entry) and `dxbc_blob`.
- New: a `shdr` module (header parse per version, variant iteration, stride 24 for vertex), `shdr.json` format and options, `Metadata` capability, and the `Detect`/`FormatId` registration. `bytecode()` can be refactored into "read variant `i`" and keep its current signature for matl.
- Status today: **partially**: only the pixel shader's first variant is read, as an internal helper; there is no `shdr` format registered and no export.

## Tests
- Unit (synthetic): headers for v8 (32 B), v12 (32 B), v13 (40 B), v14 (48 B), v15 (56 B) with distinct `name`, `type`, `shaderInputFlags` pointers; feature bytes covering `unk_9[0] = 3` and `unk_9[0] = -1` with the b1..b6 combinations, to check both `numShaders` formulas against the C code above; unsupported pairs `(9, 32) (18, 56) (20, 56) (21, 100)`; truncated headers never panic.
- Variant table: a vertex shader (stride 24) with entries `[blob, ref ~0 -> index 0, empty, null ptr + len>0]` and a pixel shader (stride 16) with the same; reference to an out-of-range index -> `Corrupt`; bytecode past end of `.mem` -> `Corrupt`; `len > 16 MiB` -> `Corrupt`; no data page on v15+ -> zero variants, no error.
- Input flags: null pointer -> zeros; `2*n` values read in order.
- DXBC: reuse `dxbc_blob`; also `RDEF` with 0 resources; blob that is not DXBC -> exported as raw bytes, compiler string empty.
- Export: JSON fields (`features` uppercase hex without 0x, `refIndices` 65535 for non-refs), `.fxc` count equals standard variants.
- Real files: `paks/Win64/common.rpak` (2459 `shdr`), `common_early.rpak` (119), `startup_heightmap_renderer.rpak` (15); all v21, header 48.

## Open questions
1. Do we want a `shdr` format at all (raw `.fxc` + JSON) or leave it as the matl helper? Which output: `shdr.json` bundle, only the first variant, or the MSW container?
2. v19 real header size: no v19 asset exists in the current build (v21 is 48 bytes, settled). v8-v17 are untested.
3. Reference index space: all 15 references seen point to an earlier entry; entry vs shader index is still not distinguishable.
4. What `unk_10`..`unk_28` pointers' extra targets mean in v21: `0x10` is `n * 24` zero bytes (runtime), `0x20` a pointer-or-integer, `0x28` an `n`-element u32 array; and what the per-variant u32 and the feature bytes (`b1`, `b3`, `b4` selecting variant dimensions; `b2`, `b5`, `b6` always 0) mean.
5. Why about half of all variants have a null pointer with a non-zero length (variants not shipped in the pak).
6. Child shaders with only a parent guid (`unk_18`): not observable in v21 (the 48-byte header has no such field; every asset has its own data page).
7. Should the vertex-layout flags be decoded into semantic names in the JSON, or kept as raw hex as rsx does? (A real semantic list is now available per vertex-shader variant.)
