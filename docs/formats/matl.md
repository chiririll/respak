# matl · Material (rpak asset, v15+), with the `shdr`/`shds` helper
**rsx sources:** `game/rtech/assets/material.{h,cpp}` (`LoadMaterialAsset`, header structs of asset versions 15-23), `shaderset.{h,cpp}`, `shader.{h,cpp}` (`ResourceBindingFromDXBlob`) · **Games / versions:** Apex Legends, asset versions 15-23 (v12 / Titanfall 2 not supported) · **Status:** BUILT in respak (`respak-respawn`); this file describes what the code does, not a proposal.

## Summary
A material: identity, a shader set reference, five related material guids, sampler/feature words and a flat array of texture guids (one per shader bind point). respak reads only that subset (no constant buffers, no render state, no `msnp` content) and exports a `matl.json` bundle: `material.json` plus the textures converted to a chosen format. Texture slot names come from the pixel shader's DXBC resource table (`shds` -> `shdr`).

## Where it lives
rpak asset, FourCC `matl` (stored as the 4 kind bytes `matl`; `entry.kind == *b"matl"`). Virtual path `<pak>.rpak/0x<GUID16>.matl` (e.g. `paks/lvl.rpak/0x00000000000000A1.matl`); the asset name (`material/...`) is a string in the header, not the path. Extension `matl`; output format `matl.json` (extension `json`). Helper kinds: `shds` (shader set), `shdr` (shader), both only read as references, no format registered for them.

## Versions and detection
- `Detect` (`MatlRead::score`): `detect_kind(source, "matl")` = score **10** for any `rpak.asset` of kind `matl`; `Detected.version = Version::new(asset version)`. No content sniffing.
- `MatlHeader::parse(version, header)` picks the layout by `(asset version, header length)`:

| asset version | header length | layout | `Version` in `FormatId` |
|---|---|---|---|
| 15-21 | any | A (snapshot absent, glue 0x88) | `major` only |
| 22, 23 | 256 | A | `major` only |
| 22, 23 | 192 | B (snapshot at 0x08, glue 0x84) | `major.1` (23.1) |
| 22, 23 | 200 | B | `major.2` (23.2) |
| anything else (incl. 12, 14, 24, 22/23 with other sizes) | | | `UnsupportedVersion { format: "matl", version: "<v> (header of <n> bytes)" }` |

Note the 192/200 variants also match v22 (code accepts them; rsx only has them at v23). Header lengths for 15-21 are not checked (tests use 0x100 and 240).

## Layout
Little-endian; header as served by the volume; pointer slots are already relocated `.mem` offsets (page pointers); `0` or `0xFFFFFFFFFFFFFFFF` = null. Offsets read by respak (everything else in the header is ignored, unknown):

| offset | size | field | layouts |
|---|---|---|---|
| 0x08 | u64 | snapshot guid (`msnp`) | B only (else reported 0) |
| 0x10 | u64 | material guid | all |
| 0x18 | ptr | name (`material/...` C string) | all |
| 0x20 | ptr | surfaceProp string | all |
| 0x28 | ptr | surfaceProp2 string | all |
| 0x30 | u64 | depthShadowMaterial guid | all |
| 0x38 | u64 | depthPrepassMaterial | all |
| 0x40 | u64 | depthVSMMaterial | all |
| 0x48 | u64 | depthShadowTightMaterial | all |
| 0x50 | u64 | colpassMaterial | all |
| 0x58 | u64 | shader set (`shds`) guid | all |
| 0x60 | ptr | textureHandles (array of u64 guids) | all |
| 0x68 | ptr | streamingTextureHandles (follows the array directly) | all |
| 0x72 / 0x74 / 0x76 | u16 x3 | width, height, depth | all |
| 0x78 | 4 bytes | samplers (shown as LE u32) | all |
| 0x80 | u32 | features | all |
| 0x88 / 0x8C | u32 x2 | glueFlags, glueFlags2 | A |
| 0x84 / 0x88 | u32 x2 | glueFlags, glueFlags2 | B |

Texture slot count: no count field. `slots = (streaming - textures) / 8` (rsx does the same). Valid when both pointers non-null, `streaming >= textures`, difference divisible by 8, `slots <= 128` (`MAX_SLOTS`); either pointer null -> 0 slots; otherwise `Corrupt("matl texture array bounds are inconsistent")`. Slot value 0 = empty.

## Data outside the header
Page (`.mem`) data only: name/surfaceProp strings and the texture guid array. Strings via `txtr::read_name(volume, at, mem_len)` (failure -> `None`, not an error). The array range is checked against the `.mem` length (`Corrupt` if it runs past). No starpak streaming, no compression for the material itself.

## Dependencies
- Textures: each non-zero slot guid is a `Dependency { guid, name: None }` (`Metadata::dependencies`), resolved as `rpak.guid` -> `rpak.asset` of kind `txtr` through `locate(guid, "txtr", origin_pak, cx)` (`GuidRef`; searches the origin pak first, then `--search` / `--cache` paks, project.md 5.5). The 5 depth/colpass guids, the snapshot guid and the surface props are NOT followed (printed as hex only).
- Shader chain for slot names (below): `shds` guid (header 0x58) -> pixel shader `shdr` guid -> DXBC. Located with kinds `shds` / `shdr`; the `shdr` is searched from the pak the `shds` was found in.
- Reused by `rmdl` (textures per material, `shader_slots`), `arig`, `aseq`, `stgs`, `uimg` (`locate`, `pak_of`, `TextureTarget`).

## Decoding
1. `read`: take `rpak.asset` (kind `matl`), `MatlHeader::parse(entry.version, read_header())`.
2. Read name and both surface props from `.mem`; read `slots * 8` bytes at `textures_at` as u64 LE guids.
3. Result `MatlAsset { header, name, surface_props, textures }` with `FormatId("matl")` and the version above; `bound_textures()` = non-zero `(slot, guid)`.

### Helper: `shds` -> `shdr` -> DXBC (`assets/shader.rs`, `codec/dxbc.rs`)
**Pixel shader guid in the `shds` header** (`pixel_shader_guid(version, header)`):

| `shds` version | guid at |
|---|---|
| 8 | 0x50 |
| 11 | 0x38 |
| 12 | 0x48 |
| 13 | 0x68, or 0x48 when the header is 80 bytes |
| 14 | 0x38 |
| 15 | 0x48 (rsx never resolves it; guess from the table, untested on real data) |
| other | `UnsupportedVersion { format: "shds" }` |

**`shdr` bytecode** (`shader::bytecode`): the asset's data page (`data_mem_offset`) holds a list of 16-byte entries `{ slot: [u8;8] (page pointer), size: i32, 4 bytes unused/unknown }`. Entry 0 is read (first variant only, as rsx). If `size < 0` it repeats entry `!size` (index must be 1..=1024, else `Corrupt`). `size <= 0` -> `Corrupt("shdr has no bytecode")`; the pointer is resolved with `mem_offset_of`; length max 16 MiB and must lie in `.mem`. The `shdr` header itself is not parsed.

**DXBC `RDEF`** (`dxbc::texture_slots(blob) -> BTreeMap<bind point, name>`): magic `DXBC` (bytes 0..4), 16-byte hash, version, total size, chunk count u32 at 0x1C, then u32 chunk offsets from 0x20; chunk = `{fourcc, size u32, data}`. First `RDEF` chunk: `resCount` u32 at data+8, `resOffset` u32 at data+0xC (relative to chunk data). Entry = 32 bytes `{nameOffset u32 @0, type u32 @4, returnType, dimension, numSamples, bindPoint u32 @20, bindCount, flags}`. Only `type == 2` (SIT_TEXTURE) entries are kept; name is a NUL-terminated UTF-8 string at `nameOffset` in the chunk; a bind point declared twice keeps the first name. Everything bounds-checked (`Corrupt("DXBC: ...")`). SM5.1 (40-byte entries) is not handled.

## Output
Edge `matl -> matl.json`, options `{"type":"matl.json","textures":{"type":"png", ...}}` (`textures` default `{"type":"png"}`; `deny_unknown_fields`; the inner object is the target format's own options). Result is a bundle written into the material's folder:
- `material.json` (pretty JSON, key order): `name`, `guid`, `version` ("23.1" style), `width`, `height`, `depth`, `shaderSet`, `surfaceProp`, `surfaceProp2`, `glueFlags`, `glueFlags2` (`0x%08X`), `samplers` (`0x%08X` of the LE u32), `features` (`0x%08X`), `depthShadowMaterial`, `depthPrepassMaterial`, `depthVSMMaterial`, `depthShadowTightMaterial`, `colpassMaterial`, `textures`. Guids are `0x`-prefixed 16-digit hex; absent strings are `null`.
- `textures`: `{ "<slot name>": { "slot": n, "guid": "0x...", "file": "textures/<name>.<ext>" | "textures/<name>" (bundle result) | null } }`.
- `textures/<name>.<ext>`: each texture is `locate`d, converted via the mediator to the `textures` target; extension from the target format. A shared texture is written once per material.
- Slot name = DXBC variable name sanitised (`[A-Za-z0-9_-]`, else `_`); duplicate -> `<name>_<slot>`; missing -> `slot_<n>`. If the slot-name chain fails (no shader set, unlocatable `shds`/`shdr`, bad version, bad DXBC) one warning `material <label>: texture slots are not named (...); using slot_<n>` and export continues. A texture that cannot be located/converted gets `file: null` and a warning. Cancellation propagates.
- Capability: `matl` provides `Metadata` (name, kind `matl`, guid, dependencies). No `Bitmap`/model capability.

## rsx quirks
- Texture count derived from pointer difference, unchecked in rsx; respak validates (max 128).
- rsx never resolves the `shds` v15 pixel shader and reads only variant 0 of a shader; respak copies both.
- rsx trusts every DXBC offset; respak bounds-checks.
- Duplicate bind point keeps the first name (kept).
- Header fields not exported by respak: constant buffer / dynamic buffer data, render flags, `msnp` content, per-texture streaming handles (the streaming array is only used as the end marker).

## Reuse in respak
`rpak.asset` (`read_header`, `read_mem`, `mem_offset_of`, `data_mem_offset`), `rpak.guid` locate (`GuidRef`), `txtr` (`read_name`, `Bitmap` conversions), `Reader`, `BundleData`. `matl` module exports `locate`, `pak_of`, `shader_slots` to `rmdl`, `arig`, `aseq`, `stgs`, `uimg`.

## Tests
- Unit (`matl/mod.rs`): every supported layout (15, 16, 21 @240, 22 @256, 23 @192/@200, 15 @0x100) parses identical fields; unsupported pairs `(12,240) (14,240) (24,240) (22,230) (23,264)`; inconsistent texture arrays -> `Corrupt`; short header and all truncations never panic.
- Unit (`shader.rs`): pixel-shader offset per `shds` version incl. `(13, 80)`; unknown version / short header -> error.
- Unit (`dxbc.rs`, blob from `respak_respawn_testing::dxbc_blob`): texture bind points, other resource types skipped, duplicate bind point, garbage/truncation/byte-flip/huge chunk count, missing `RDEF`.
- CLI (`respak-cli/tests/material_cli.rs`): `convert .../0x00000000000000A1.matl --to {"type":"matl.json"}` with `--search` / `--cache` (texture found in another pak -> `textures/slot_0.png`), `copy`, `list`. Fixture builder: `PakBuilder::matl` (v16, 240 B), `shds` (v12, 80 B), `shdr` (v12).
- Real files: none documented; no Steam runs without permission.

## Open questions / gaps
1. No end-to-end test of the named-slot path (`matl` -> `shds` -> `shdr` -> names) in the matl tests; only unit pieces. `rmdl` tests may cover `shader_slots` (not checked).
2. `shds` v15 offset (0x48) is unverified; matl v22-23 pair with newer `shds` versions that the table does not list (no layout for them -> falls back to `slot_<n>`).
3. Layout B accepted for v22 too; true v22 header sizes in the wild unconfirmed.
4. Unread header fields (constant buffers, flags, `msnp`) and DXBC SM5.1; no `matl.json` import or round trip.
5. Header length for v15-21 is not validated; reading past it fails with a short-read error (`Corrupt`).
