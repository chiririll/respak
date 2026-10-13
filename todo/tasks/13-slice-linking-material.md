# 13 · Slice: linking, a material with its textures
**Status:** in-progress (built; waiting for the demo) · **Depends on:** 12 · **See:** PROJECT.md §5.5, §5.6 (built-ins 2, 3), §6 (GUID lookup), §7, §12

## Goal
Exporting a material also exports its textures, which may live in other paks, into the material's own folder. This is the first converter that asks the mediator for other values.

## Decisions (user)
- Material output: **JSON** (`material.json` in the material's folder). Textures: converted through the mediator, target **configurable** via a `textures` option (a nested target, default `{"type":"png"}`).

## rsx review
(Draft for the gate. Offsets were computed from the structs in `material.h`, not checked against real pak data.)

**What it does.** `rsx/src/game/rtech/assets/material.{h,cpp}`: `LoadMaterialAsset` (header), `PostLoadMaterialAsset` (shaderset, snapshot, textures), `ExportMaterialStruct` (JSON). Variant by pak asset version: 12 and 15 have their own structs; 16–21 share the v16 struct (240 B); 22/23 branch on `headerStructSize`: 256 = v22, 192 = v23.1, 200 = v23.2. Pointers are already relocated by the pak loader.
- Common layout (v15–v22): guid 0x10, name 0x18, surfaceProp 0x20/0x28, five depth/colpass guids 0x30–0x50, shaderSet guid 0x58, `textureHandles` ptr 0x60 (u64 guid array, slot index = shader bind point, 0 = empty), `streamingTextureHandles` ptr 0x68, width/height/depth i16 at 0x72–0x76, samplers 0x78, features 0x80. v23.x adds a snapshot guid at 0x08, drops dxStates (they come from the `msnp` asset) and shifts the later fields. v12 is a different layout (own table).
- No explicit texture count: it is `(streamingTextureHandles − textureHandles) / 8`. The streaming array is empty in the pak (filled at runtime), so a material never needs streamed data itself.
- Slot semantic names (`albedoTexture`, …) come from the shaderset → pixel shader DXBC RDEF chunk. Fallback: the texture's own `type` byte through `s_TextureTypeMap` (`_col`, `_nml`, `_gls`, …).
- Name may be a partial path on old versions; rsx repairs it by brute-forcing suffixes against the GUID.

**Problems.** Unknown v22/v23 header size asserts then null-derefs; CPU pointer unchecked for v12–v22; texture count unvalidated pointer difference; `numStreamingTextureHandles` unused; dead `sizeof(const char*)` prefix length; format-less log call; v16/v22 and the six `MaterialAsset` constructors are copy-paste; v23 dx states silently zero if the snapshot isn't loaded; only `dxStates[0]` exported; legacy-type materials not exported at all; global state (`g_assetData`, `g_rsxSettings`, `g_dxHandler`) and D3D buffers created in post-load; whole-pak-in-memory assumption.

**Fit.**
- One `matl` handler (`rpak.asset` → `matl`) with a per-(version, header size) offset table; unknown size → `UnsupportedVersion`, count validated against the asset size → `Corrupt`.
- Reads only the header and the GUID array via ranged reads on `.mem`; no shader, snapshot, uber or D3D work.
- `Metadata::dependencies()` returns the non-zero texture GUIDs (this is what the GUID lookup consumes).
- Behaviour changes vs rsx (for the demo comparison): no DXBC parsing, so slot names are `slot_<n>` plus the texture-type suffix when the texture is readable; no `.uber`; only the header fields below are exported; unknown header sizes error instead of crashing.

**rsx review addendum: shader chain** (`shader.{h,cpp}`, `shaderset.{h,cpp}`; offsets computed from the structs, not checked against real data).
- *What it does:* `material.shaderSet` → `shds`, whose pixel-shader **guid** sits at a per-asset-version offset (v8 0x50, v11 0x38, v12 0x48, v13 0x68 (header size 80 = v12 layout), v14 0x38, v15 0x48) → `shdr`. The DXBC bytecode is not in the `shdr` header: it is entry 0 of the asset's CPU page, `{ptr@0, i32 size@8}` (a negative size refers to entry `~size`). DXBC: `"DXBC"`, chunk count u32@0x1C, chunk offsets from 0x20, chunk = `{fourcc, size, data}`. In the first `RDEF` chunk (offsets relative to its data): resCount@8, resOffset@0xC, 32-byte entries `{nameOff, type, …, bindPoint@20, …}`; type 2 = texture; name is a NUL-terminated string. Result: bind point → name (`albedoTexture`, …).
- *Problems:* no bounds checks anywhere (blob offsets, name offsets, chunk count); no NUL check; only entry 0 is read and refs/null/size 0 aren't handled; duplicate bind points silently first-wins; array `BindCount` ignored; rsx never resolves `pixelShaderAsset` for shds v15 (so v15 materials get no names there); SM5.1 (40-byte entries) unsupported; only the pixel shader's variant 0.
- *Fit:* `dxbc` module in respak-respawn with `texture_slots(&[u8]) -> Result<BTreeMap<u32,String>>` (checked arithmetic, errors, first-wins kept and documented); a small `shds` reader (pixel-shader guid per version) and a `shdr` reader (CPU-page entry 0 via ranged read on `.mem`, follows one negative-size ref). Unlike rsx, v15 `shds` is read (offset 0x48). Any failure in the chain → `slot_<n>` + `Warning`.
- *Behaviour changes vs rsx:* v15 shds names now resolve; malformed shaders fall back instead of reading out of bounds.

**Gate answers (user)**
1. Slot names: **parse the shader** (shaderSet → `shds` → pixel shader → DXBC RDEF bound resources). If anything in that chain fails (shader missing, unsupported, corrupt), fall back to `slot_<n>` and publish a `Warning`. This replaces the "no DXBC parsing" behaviour change above; it adds an rsx review addendum for `shader`/`shaderset` (below) and tests 12–13.
2. Versions: port 15, 16–22, 23.1, 23.2 now; **v12 (Titanfall 2 legacy layout) goes to the backlog**.
3. JSON fields (approved): name, guid, version, width/height/depth, shaderSet guid, surfaceProp(s), glue flags, samplers, features, `textures: {slot: {guid, file}}`, plus depth/colpass material guids. dxStates, uber data and animation left out. OK?

## Design (proposed)
**respak-respawn**
- `matl.rs`: `MatlRead` (score on `kind == b"matl"`), `MatlAsset` (Metadata only: name, guid, deps), `MatlHeader`/`MatlLayout` offset table.
- `locate.rs`: `RpakLocate` edge `rpak.guid → rpak.asset`, searching §6 order: same pak, `common.rpak` beside it, `RespawnPlugin::search`, plugin-cache GUID index. `RespawnPlugin::new().search([...])`.
- `matl_json.rs`: `MatlJsonEncode` (`matl → json` via `TypedTarget<MatlJsonOptions { textures: Target }>`): for each non-zero slot, `cx.mediator.convert(guid → texture, options.textures)`, `cx.write("<folder>/textures/<name>.<ext>")`, then the JSON as the main blob.
**respak-core**
- `Cx`: call chain + `cache() -> Option<PluginCache>` (namespaced by plugin id + `cache_version`; errors become `Warning`).
- Middleware: Dedup (`Shared` future per (key, target)) and Cycle guard (chain + wait-for graph → `DependencyCycle`), inserted in the §5.6 order.
- A handler-side way to write into its output's composite folder (today only the engine does that).
- `ConvertReport` gains `dependencies` (key + format) per output.
**CLI:** `--search <glob>...`, `--cache <dir>`.

## Progress
- **Core done** (tests in `respak-core/tests/{linking,cache}.rs`, clippy clean): call chain on `Cx`; `CycleGuardMiddleware` and `DedupMiddleware` (with the wait-for graph); `PluginCache` (`cx.cache("<plugin id>")`, the id is passed because `Cx` isn't per plugin; `Plugin::cache_version()`, `EngineBuilder::cache`); `dependencies` in `OutputResult::Ok` (values a converter requested directly).
- **Design change:** the material converter returns a `BundleData` (`material.json` + `textures/*`) and the engine writes it into the composite folder, instead of a new handler-side write path. This reuses §5.5 as is and keeps the output-conflict tracking.
- **respak-respawn done:** `matl` (`matl.rs`: layouts 15–21, 22, 23.1, 23.2), `dxbc.rs` (RDEF texture slots, bounds-checked), `shader.rs` (`shds` pixel-shader guid per version, `shdr` bytecode), `locate.rs` (`GuidRef`, `RpakLocate` with the §6 order and the cache index), `matl_json.rs` (`matl -> matl.json`, bundle of `material.json` + `textures/*`), `RespawnPlugin::search`, `cache_version` 1, `RpakAsset::mem_offset_of` (pointers inside page data are not relocated by the volume).
- **CLI done:** `--search <glob>` (repeat for several) and `--cache <dir>`.
- Tests: `respak-core/tests/{linking,cache}.rs`, unit tests in `matl.rs`/`dxbc.rs`/`shader.rs`, `respak-cli/tests/{material,material_cli}.rs`. `cargo test --workspace` and clippy `-D warnings` pass; no plugin-to-plugin dependency.
- **Design changes / notes for the demo**
  - Output: `matl.json` target, `{"type":"matl.json","textures":{"type":"png"|"dds"|…}}`; JSON has `textures: {<slot name>: {slot, guid, file}}` (`file` is `null` and a warning is published when a texture cannot be found or converted; the export still succeeds).
  - `cx.cache(plugin_id)` instead of `cx.cache()`; the material converter returns a bundle instead of writing through `cx.write`.
  - `dependencies` = values a converter requested directly (one level).
  - Offsets of the material, shader set and shader headers come from the rsx structs and are **unverified against real paks**: the demo is where they get checked. Expect to adjust `matl.rs` / `shader.rs` tables.
  - Dedup is for concurrent requests only (see the backlog).

## Tests (proposed, for the gate)
Fixtures are synthetic rpaks built in tests.
1. `matl` score: matches only `matl` entries; version/variant detection for 15, 16–22, 23.1 (192 B), 23.2 (200 B); unknown header size → `UnsupportedVersion`; absurd texture count → `Corrupt`.
2. Header parse per layout: guid, name, size, texture GUID array with empty slots kept in place.
3. `dependencies()` lists only non-zero texture GUIDs.
4. Locate order: same pak wins over `common.rpak`, over search list, over cache index; none → `MissingDependency`; never scans unrelated paks.
5. GUID index goes to the plugin cache and is reused; a failing cache only produces a `Warning`.
6. Material export: texture in `common.rpak` lands in `<material>/textures/…`, JSON references it by relative path, `textures` option switches PNG ↔ DDS.
7. Two materials sharing a texture each write their own copy; the texture is decoded once (dedup counter).
8. Dedup: N concurrent identical requests run the handler once; sequential ones run again.
9. Cycle guard: A→B→A returns `DependencyCycle`; concurrent A→B / B→A does not deadlock.
10. Report lists `dependencies`; cancellation mid-material returns `Cancelled` with no partial JSON.
11. CLI: `--search` and `--cache` parse and reach the plugin (in-process with `Memory` services).
12. DXBC RDEF parser (synthetic blobs): texture bind points → names; truncated/garbage blob → error, not panic.
13. Slot naming: shader resolves → semantic names in JSON; shader missing or corrupt → `slot_<n>` and a `Warning`, export still succeeds.

## Scope
Material parsing is ported from rsx (`assets/material.{h,cpp}`, all versions and variants). Review it first, then port it (PROJECT.md §12).

**respak-respawn:**
- `matl`: read from `rpak.asset`, `Metadata` (name, guid, dependencies).
- `rpak.guid → rpak.asset` (`RpakLocate`) with the §6 search order; `RespawnPlugin::new().search([...])`.
- A material output that converts its textures through the mediator and writes them via `cx.write` into the composite folder. **The output format (JSON? something else?) is the user's call.**

**Core:**
- Dedup middleware: concurrent requests for the same (key, target) share one `Shared` future.
- Cycle guard: the call chain plus a wait-for graph → `DependencyCycle`.
- Plugin cache (`cx.cache()`, namespaced by plugin id + `cache_version`), first used for the GUID → file index. Cache errors become warnings.
- `dependencies` in the report.

**CLI:** `--search <glob>...`, `--cache <dir>`.

## Demo
The user exports a level material whose textures live in `common.rpak`. Two materials sharing a texture each write their own copy, and that texture is decoded once per run.
