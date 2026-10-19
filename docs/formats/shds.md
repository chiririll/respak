# shds · Shader set (rpak asset)
> **Verified against** Apex depot 1172471 `paks/Win64/common_early.rpak` (37), `mp_lobby.rpak` (43), `common.rpak` (2317, headers only), `startup_heightmap_renderer.rpak` (2): **only version 15 with an 80-byte header occurs** in the current build. The v15 findings are real; v8-v14 are rsx-only and unverified (no such asset in current paks).
**rsx sources:** `game/rtech/assets/shaderset.{h,cpp}` (`ShaderSetAssetHeader_v8/v11/v12/v13/v14/v15_t`, `ShaderSetAsset`, `LoadShaderSetAsset`, `PostLoadShaderSetAsset`, `ExportMSWShaderSetAsset`), `core/shaderexp/multishader.h` (`CMultiShaderWrapperIO`, MSW file), `core/filehandling/validation.cpp` (`SHDS_VERSIONS`, header sizes), `game/asset.h` (`SHDS`) · **Games / versions:** Titanfall 2 (v8), Apex Legends (v11-v15; newer Apex versions are not in rsx) · **Priority hint:** S. A 64-112 byte descriptor with two guids and a few counters. Standalone value is low; it is the link that matl uses. Respak support: **no** as a format (the README marks it `no`); the pixel-shader guid lookup already exists as a helper in `matl` (`pixel_shader_guid`).

Related: [shdr](shdr.md) (the shaders a set points to), [matl](matl.md) (reads the pixel shader guid out of a `shds` header to name texture slots).

## Summary
A shader set (a "program") pairs one vertex shader with one pixel shader (both `shdr` assets, by guid) and records how many texture inputs each uses, how many samplers, and the range of resource bind points. A material (`matl`) references exactly one shader set (`matl` header 0x58). A user would convert it to a small JSON (guids, counts, referenced names) or to rsx's MSW container; the interesting content is in the two `shdr` assets.

## Where it lives
- Container: rpak asset, header page only. No CPU/data page content is read, no starpak.
- FourCC as stored: bytes `shds` (rsx constant `'sdhs'` = LE u32 of `s h d s`; `AssetType_t::SHDS = MAKEFOURCC('s','h','d','s')`). `entry.kind == *b"shds"`. Extension `shds`. Virtual path `<pak>.rpak/0x<GUID16>.shds`.
- Name: v8-v14 header holds a page pointer `name` (C string at offset 0x08); rsx stores `shaderset/<name>.rpak` (adds the `shaderset/` prefix and `.rpak` suffix if missing, `LoadShaderSetAsset`). **v15 name is not read** (all fields except the shader guids are left zero) and rsx asks the guid -> name cache (`SetAssetNameFromCache`). In respak the path is guid-based; the name is metadata.
- rsx export folder: `shaderset/`.

## Versions and detection
rsx handles asset versions **8, 11, 12, 13, 14, 15** (`LoadShaderSetAsset`; unknown -> `assertm(false, "Unknown ShaderSet asset version")`). Version 13 has two header sizes, told apart by `headerStructSize` (80 = the v12 struct, rsx sets the minor version to 1; 112 = the v13 struct).

| asset version | header size | rsx struct | respak `Version` |
|---|---|---|---|
| 8 | 88 | `ShaderSetAssetHeader_v8_t` | 8 |
| 11 | 64 | `..._v11_t` | 11 |
| 12 | 80 | `..._v12_t` | 12 |
| 13 | 80 | `..._v12_t` | 13.1 |
| 13 | 112 | `..._v13_t` | 13 |
| 14 | 64 | `..._v14_t` | 14 |
| 15 | 80 | `..._v15_t` (mostly unknown) | 15 |
| any other, or a known version with another size | | | `UnsupportedVersion { format: "shds", version: "<v> (header of <n> bytes)" }` |

`Detect`: score 10 for any `rpak.asset` of kind `shds` (as `matl`, no content sniffing). The existing helper (`pixel_shader_guid`) already matches `(version, header len)` for the pixel guid offsets (same table as below) and errors on other versions. The helper does not check the header length except `(13, 80)` -> v12 layout; a full reader should require the exact lengths in the table.

## Layout
Little-endian; pointer slots in the header are page pointers (already relocated), `char*` null = no name. Offsets from the C structs (static_asserts in the header confirm the sizes and a few offsets: v8 `textureInputCounts` @0x18, `reserved_vsInputLayoutIds` @0x22; v15 `pixelShader` @72).

| field | v8 (88) | v11 (64) | v12 / v13.1 (80) | v13 (112) | v14 (64) | v15 (80) |
|---|---|---|---|---|---|---|
| `reserved_vftable` u64 | 0x00 | 0x00 | 0x00 | 0x00 | 0x00 | 0x00 (zero in files) |
| `name` ptr | 0x08 | 0x08 | 0x08 | 0x08 | 0x08 | 0x08, **null in all real v15 sets** |
| `reserved_inputFlags` u64 | 0x10 | 0x10 | 0x10 | 0x10 | absent | unknown |
| `textureInputCounts[2]` u16 x2 | 0x18 | 0x18 | 0x18 | 0x18 | 0x10 | **0x10** (verified) |
| `numSamplers` u16 | 0x1C | 0x1C | 0x1C | 0x1C | 0x14 | **0x14** (verified) |
| `firstResourceBindPoint` | u16 @0x1E | u8 @0x1E | u8 @0x1E | u8 @0x1E | u8 @0x16 | unknown |
| `numResources` | u16 @0x20 | u8 @0x1F | u8 @0x1F | u8 @0x1F | u8 @0x17 | unknown |
| `reserved_vsInputLayoutIds` | u8[32] @0x22 | u8[16] @0x20 | u8[32] @0x20 | u8[32] @0x20 | absent | unknown |
| unknown block | none | none | none | `unk_40` u8[32] @0x40 | `unk_28` u8[16] @0x18, `unk_38` u64 @0x28 (value `0x200`; rsx names are off vs offsets) | unknown |
| **`vertexShader` guid** u64 | **0x48** | **0x30** | **0x40** | **0x60** | **0x30** | **0x40** |
| **`pixelShader` guid** u64 | **0x50** | **0x38** | **0x48** | **0x68** | **0x38** | **0x48** |

Notes:
- 0x1E/0x1F are two separate bytes in v11-v14 but v8 has two u16 (0x1E and 0x20; bytes 0x22 on are the layout ids).
- `reserved_*` fields are runtime slots (zero in the file). `reserved_vsInputLayoutIds` is filled by the game with an index per vertex-shader variant, `0xFF` = no shader for that index (v8 comment).
- `textureInputCounts` (u16 x2) mean different things by version:
  - v8: `[0]` = vertex shader textures, `[1]` = pixel shader textures.
  - v11-v14: `[0]` = pixel shader textures, `[1]` = total, vertex = `[1] - [0]` (rsx comment: if equal the textures are used only by the pixel shader; usually the vertex shader uses 0).
- A guid of 0 = no such shader. Vertex and pixel guids refer to `shdr` assets, possibly in another pak (rsx resolves with `g_assetData.FindAssetByGUID`).
- The pixel-shader offsets are exactly the ones in `crates/respak-respawn/src/assets/shader.rs`: v8 0x50, v11 0x38, v12 0x48, v13 0x68 (0x48 for the 80-byte header), v14 0x38, v15 0x48. They agree with this table.
- No count field for textures: the counts above are all rsx records. The material's texture array size is `(streaming - textures) / 8` (matl), the bind-point range is in the shader set.

## Verified on real data (v15)
All 80 `shds` read in full (`common_early`, `mp_lobby`) are **version 15, header size 80** (2317 of 2317 in `common.rpak` have the same version and size).
- **Guids confirmed:** `vertexShader` @0x40 and `pixelShader` @0x48 (never 0). In `common_early` + `mp_lobby`, 64 of 80 vertex guids and 73 of 80 pixel guids are `shdr` assets of the *same* pak; the rest live in another pak (e.g. `common.rpak`). The 80-byte layout is `0x00..0x3F` header fields, `0x40` vertex guid, `0x48` pixel guid.
- **Name:** the `name` pointer at 0x08 is zero in all 80 (no name stored in v15), so names must come from a cache.
- **Counters (v14-like layout, so v15 shares it):** `0x10` u16 `textureInputCounts[0]` (pixel textures, seen 1..8), `0x12` u16 `textureInputCounts[1]` (total; `[0] <= [1]` in 80 of 80, equal in 69, so vertex textures = `[1] - [0]` is 0 in most sets), `0x14` u16 `numSamplers` (3 in 53, also 1, 2, 4, 5), `0x16` u8 `firstResourceBindPoint` and `0x17` u8 `numResources` (both 0 in all 80). Sixteen sets (`common_early`) have all of these at 0.
- `0x3A` holds a small byte (2 in 58 sets, 1 in 5, 0 elsewhere): the same "value `0x200`"-style field that v14 calls `unk_38`; everything else in `0x00..0x3F` (including `0x18..0x37`) is zero. So the first 64 bytes of v15 are almost entirely runtime/zero; only the counters and that byte carry data.

## Data outside the header
None. The header holds only the name string (page data), the two guids and counters. Nothing is compressed or streamed. The shaders themselves are `shdr` assets ([shdr](shdr.md)).

## Dependencies
- `vertexShader`, `pixelShader` -> `shdr` assets by guid. rsx resolves them in `PostLoadShaderSetAsset` for v8-v14 only (the function returns early for v15, so a v15 set never gets `vertexShaderAsset` / `pixelShaderAsset`, even though the guids are read). Respak: `locate(guid, "shdr", &pak_of(shds.key()), cx)` (searches the pak the `shds` came from first, then `--search` / `--cache`, project.md 5.5) as `matl` does for the pixel shader.
- Referenced *by* `matl` header 0x58 (shader set guid). The five depth/colpass material guids in `matl` are separate materials, not shader sets.
- A converted output does not need the shaders to exist; unresolved guids become `null` / hex only (as rsx's `writer.SetShaderSetHeader` writes guids even when the assets are missing).

## Decoding
1. `read`: take `rpak.asset` kind `shds`, `read_header()`; require `(version, header length)` from the table, else `UnsupportedVersion`.
2. Read `name` (v8-v14) via `txtr::read_name(volume, at, mem_len)` (failure -> `None`); skip for v15.
3. Read the counters at the version's offsets; compute `numPixelShaderTextures`, `numVertexShaderTextures` as above. `reserved_*` are ignored. v15: only the two guids.
4. Read both guids at the version's offsets (u64).
5. Optional resolution: `locate` both as `shdr`; print or embed their names and types.

Pseudocode for the counters (rsx constructors):
```
v8 : vs = counts[0];             ps = counts[1]
v11+: ps = counts[0];            vs = counts[1] - counts[0]   // u16 subtraction; unsigned wrap possible if counts[1] < counts[0] (rsx does not check)
```

## Output
rsx has two export settings (`{"MSW", "MSW (Packed)"}`), both write one `.msw` file (`ExportMSWShaderSetAsset`, `CMultiShaderWrapperIO`). Path: `<export>/<asset name>` with extension replaced by `.msw` (the asset name already starts with `shaderset/`; rsx creates only the `shaderset/` directory, see quirks).

MSW (version 3, packed structs, `fileType = 1` SHADERSET):
| offset | size | field |
|---|---|---|
| 0 | 3 | magic `MSW` (`'M' | 'S'<<8 | 'W'<<16`) |
| 3 | 1 | version = 3 |
| 4 | 1 | file type = 1 (0 = shader) |
| 5 | 8 | `pixelShaderGuid` |
| 13 | 8 | `vertexShaderGuid` |
| 21 | 2 | `numPixelShaderTextures` |
| 23 | 2 | `numVertexShaderTextures` |
| 25 | 2 | `numSamplers` |
| 27 | 1 | `firstResourceBindPoint` (truncated to u8) |
| 28 | 1 | `numResources` (truncated to u8) |
| 29 | 4 | `pixelShaderOffset` (absolute file offset, 0 = not embedded) |
| 33 | 4 | `vertexShaderOffset` (absolute, 0 = not embedded) |
| 37 | ... | when packed: the pixel shader block, then the vertex shader block, each a `MultiShaderWrapper_Shader_t` shader record as in [shdr](shdr.md) (16-byte header, 32-byte descriptors, buffers, name) |

"MSW" (not packed) writes only guids and counters; "MSW (Packed)" also embeds the two `shdr` assets' variant buffers (when `pixelShaderAsset` / `vertexShaderAsset` resolved via `PostLoad`; none for v15).

Proposed respak targets (not implemented; propose, the user decides):
- Format `shds.json`, one file next to the asset:
  ```json
  { "name": "shaderset/...", "guid": "0x...", "version": "13.1",
    "vertexShader": "0x...", "pixelShader": "0x...",
    "vertexShaderTextures": 0, "pixelShaderTextures": 6, "samplers": 6,
    "firstResourceBindPoint": 0, "numResources": 0 }
  ```
  (guids as `0x`-prefixed 16-digit hex, as in `matl.json`; names resolved through `shdr` appended when located). Capability: `Metadata` (name, kind `shds`, guid, dependencies = the two shader guids). No bitmap/model capability.
- Skip the MSW container (rsx / repak specific, undefined for other tools).
- Import / pack: none.

## rsx quirks
- Wrong/misleading struct field names: v14 `unk_28` is actually at 0x18 and `unk_38` at 0x28 (derived offsets); the v13 comment says "newer versions of v13 use [the v12] header, omitting unk_40".
- v15: `name`, counts and layout are not read; `PostLoad` returns early for v15, so rsx never resolves its shaders (matl's guid offset 0x48 for v15 is a table guess confirmed only by `static_assert(offsetof(.., pixelShader) == 72)`; untested on real data).
- v8 and later disagree on the counts and on field widths (`u16` vs `u8`); the `textureInputCounts` order is swapped.
- `vs = counts[1] - counts[0]` is a plain u16 subtraction (can wrap).
- `ExportShaderSetAsset` creates `<export>/shaderset` but writes to `<export>/<asset name>`; with names like `shaderset/foo.rpak` the folders can disagree with the export root or `exportPathsFull`. Not needed in respak.
- `MAX_SHADERS_PER_ASSET_R2 = 32` is the size of the v8/v12/v13 layout id array.
- Keep: version-aware offsets, `13.1` for the 80-byte v13. Drop: runtime slots, MSW.

## Reuse in respak
- Existing: `shader::pixel_shader_guid(version, header)` in `assets/shader.rs` (same offsets; keep and share, or make `shds` module the owner and let matl call it), `matl::locate` / `pak_of` (guid resolution), `txtr::read_name`, `Reader`, `rpak.asset`, `respak-respawn-testing` `PakBuilder::shds` (v12, 80 bytes, pixel at 0x48).
- New: a `shds` header module with the full table (name, counts, vertex guid and the length checks), the `shds.json` format (if chosen), `Metadata` capability and `Detect`.

## Tests
- Unit: a header per row of the table (88, 64, 80, 80 as 13.1, 112, 64, 80) with a distinct sentinel in every field; assert name, counters (including swapped v8 counts, `vs = counts[1] - counts[0]`), vertex and pixel guids; unsupported `(9, 80) (16, 80) (8, 80) (12, 64) (14, 80)`; short header (`< 0x50` for v8) -> `Corrupt` without panic; truncation fuzz.
- Round trip with the existing helper tests (`pixel_shader_guid` on every row stays equal to the new reader).
- Guid 0 / unresolved shader guid -> `null` in JSON and a warning, not an error.
- CLI: `convert .../0x....shds --to {"type":"shds.json"}` with the shaders in `--search` pak.
- Real files: `paks/Win64/common_early.rpak` (37), `paks/Win64/mp_lobby.rpak` (43), both version 15 / 80 bytes.

## Open questions
1. Is a standalone `shds` format wanted (JSON of guids and counters), or only the internal helper used by matl?
2. `shds` versions >= 16 that rsx does not know are not in the current build (all real sets are v15, whose counter offsets are now confirmed); the only open part of v15 is the meaning of the byte at 0x3A and whether `numResources` / `firstResourceBindPoint` are ever non-zero (they are 0 in all 80).
3. v13: confirm that the 80-byte v13 header (= v12 struct) is the S9+ layout and 112 is S8 or earlier; the proposed `13.1` follows rsx. (Not present in current paks.)
4. Meaning of `reserved_inputFlags`, `unk_40`, `unk_28`, `unk_38` (`0x200`) and v14's missing `reserved_inputFlags`. (Not present in current paks.)
5. How should the `shds.json` carry the shader texture counts for v8 (swapped order): normalise to `pixel/vertex` (proposed) or keep the raw order?
6. Should the output include the resolved `shdr` names and types (needs `locate`, optional)? Note that about a fifth of the vertex guids and a tenth of the pixel guids are in another pak.
