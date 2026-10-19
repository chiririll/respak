# efct, rpsk · Particle effect and particle script
**rsx sources:** `game/rtech/assets/particle_script.{h,cpp}` (`ParticleScriptAssetHeader_v8_t`, `ParticleScriptAsset`, `LoadParticleScriptAsset` :5, `InitParticleScriptAssetType` :42), `game/asset.h` (`AssetType_t::EFCT`/`RPSK` :39-40, `s_AssetTypePaths` :192-193), `docs/assets/rpak/ParticleEffect.md`, `docs/assets/rpak/ParticleScript.md` (rsx docs, the second is an empty stub) · **Games / versions:** Apex Legends only. `efct` is not in Titanfall 2 (particles there are loose `.pcf` files in VPKs). `rpsk` only ever existed as version 8 and "is now cut from the game" (comment in `LoadParticleScriptAsset`) · **Priority hint:** low. `rpsk` is S (a 136-byte header of guids and a name, readable but not useful alone). `efct` is unknown size (rsx has no code for it, so nothing to port); treat as L if it is ever picked up.

> **Real-data check (Apex depot 1172471, current build).** `efct`: present in large numbers (23 in `mp_lobby.rpak`, ~10,000 in `common_mp.rpak`, ~23,000 in `common.rpak`); asset version **36**, entry header size **24** (see below). `rpsk`: **not present** in any of the 115 non-cosmetic paks listed (`common`, `common_mp`, `common_early`, `startup*`, `ui*`, `mp_lobby*`, all `mp_rr_*`, `script_*`, `localization_*`, `subtitles_*`, ...); the several thousand cosmetic paks (`charm_*`, `gcard_*`, `loadscreen_*`, `material_stickers_*`, `root_lgnd_*`) were not listed. This is consistent with rsx's "cut from the game" comment, but `rpsk` layout is unverified.

> **Scope of what was read.** `rpsk`: header and loader read in full. `efct`: rsx has **no** loader, header or export code; the kind is only a FourCC and a path name. Everything about the `efct` layout is unknown. No game file was opened.

## Summary
- `efct` (particle effect): per rsx docs, "effectively an rpak-ified version of a PCF file" (Valve particle system file, a DMX extension). Defines particle effects for the engine. Natural conversion target would be PCF/DMX, but nothing in rsx tells how the rpak form stores it.
- `rpsk` (particle script): a GPU particle script. A header with two texture guids, five shader guids (system-effect compute, process compute, render vertex, geometry, pixel), a name, and an "uber static buffer" of constants. A user could at most get the name, the referenced guids and the raw static buffer.

## Where it lives
- Container: rpak.
- `efct`: FourCC stored `efct` (file order); rsx's enum is `MAKEFOURCC('e','f','c','t')` (`asset.h:39`). No type binding is registered (grep of `main.cpp` has no `efct` init), so rsx does not load it. rsx path prefix `effect` (`asset.h:192`). Extension in respak: `<path>.efct`. Name: unknown.
- `rpsk`: FourCC stored `rpsk`; rsx registers it reversed as the literal `'kspr'` (`particle_script.cpp:47`), alignment 8 (`:48`). rsx path prefix `particle_script` (`asset.h:193`). Extension in respak: `<path>.rpsk`.
- Name (`rpsk`): **stored.** The header has a `name` page pointer to a NUL-terminated string `X`; rsx names the asset `particle_script/X.rpak` (`:29`, the `.rpak` suffix is rsx's own convention for the name, as written). In debug rsx asserts `StringToGuid("particle_script/X.rpak") == asset guid` (`:32-33`), so the guid is derived from that exact string.
- Streamed data: none known.

## Versions and detection
- `rpsk`: asset version **8** only. Any other version: rsx `assert`s and returns (`:21-25`) -> `UnsupportedVersion`. Proposed: kind `rpsk`, score 10 on kind, `Version { major: 8, minor: 0 }`.
- `efct`: versions unknown (not handled in rsx). Proposed: do not claim any version -> always `UnsupportedVersion`, or do not register a decoder.

## Layout
### `rpsk` v8, `ParticleScriptAssetHeader_v8_t`, 0x88 (136) bytes, align 8, little-endian
Offsets computed by hand from the struct (natural alignment, no `static_assert` in rsx). "page ptr" = 8-byte pointer relocated by the rpak volume.
| off | size | type | field | meaning |
|---|---|---|---|---|
| 0x00 | 16 | u64[2] | `textures` | guids of two `txtr` assets (0 = none) |
| 0x10 | 16 | u8[16] | `unk_10` | "texture input slots ?" (rsx comment), unknown |
| 0x20 | 32 | u64[4] | `unk_20` | unknown |
| 0x40 | 8 | page ptr | `name` | NUL-terminated string |
| 0x48 | 8 | page ptr | `uberStaticBuffer` | `uberStaticBufferSize` bytes of constants |
| 0x50 | 8 | u64 | `systemEffectComputeShader` | guid (`shds`/`shdr`, presumed) |
| 0x58 | 8 | u64 | `processComputeShader` | guid |
| 0x60 | 8 | u64 | `renderVertexShader` | guid |
| 0x68 | 8 | u64 | `renderGeometryShader` | guid |
| 0x70 | 8 | u64 | `renderPixelShader` | guid |
| 0x78 | 4 | u32 | `unk_78` | unknown |
| 0x7C | 4 | u32 | `uberStaticBufferSize` | bytes |
| 0x80 | 8 | u8[8] | `unk_80` | "last two null and never referenced in update func (whereas first six are)" (rsx comment) |

### `efct`
Unknown. No struct in rsx. Observed on real data (`mp_lobby.rpak`, 23 assets, all identical in shape): version 36, header size 24, bytes `u64 0 | page ptr (relocated) | u32 0 | u32 1`; the 8 bytes at 0x00 are zero (not the `u64::MAX` of a null pointer), so they are a plain value, not a pointer. The pointed-to block starts with a 16-byte hash-like value, a small count (2 or 4) and `{offset, count}` style u32 pairs, i.e. an offset table, not a PCF/DMX text. No starpak stream or optional stream. Meaning of all of it is unknown.

## Data outside the header
- `rpsk`: `name` string and `uberStaticBuffer` in page data (`uberStaticBufferSize` bytes). No starpak, no compression known.
- `efct`: unknown.

## Dependencies
- `rpsk`: two texture guids (`textures[0..2]`, presumably `txtr`) and five shader guids. rsx only stores them (`ParticleScriptAsset`), the preview and resolution are a todo (`// [rika]: todo preview textures and shaders ?`). Map to respak linking (project.md §5.5) as guid references; the output does not need them to be resolved if only the metadata JSON is written.
- `efct`: unknown (a PCF references materials and child systems by name, but the rpak form is unseen).

## Decoding
`rpsk`: read 0x88 bytes at the asset header, follow `name` for the string, copy `uberStaticBufferSize` bytes from `uberStaticBuffer` if non-null. No bounds are checked by rsx (it reinterprets the header in place); respak needs bounded reads and `Corrupt` on a bad pointer or size. `efct`: nothing to decode.

## Output
- rsx: **no export** for either (`.e = { nullptr, 0, nullptr, 0ull }` for `rpsk`; `efct` unregistered). It only loads `rpsk` into `ParticleScriptAsset` for the asset list.
- Proposed respak: `rpsk` -> JSON (`name`, `textures` as guid strings, five shader guids as guid strings, `unk_*` as hex, `uber_static_buffer_size`) plus the raw buffer as `<name>.bin` if wanted. Low value. `efct` -> nothing (unsupported kind). A JSON-only target would use an existing metadata/text capability; no new capability needed.

## rsx quirks
- Name suffix `.rpak` appended to a path under `particle_script/`; hard-coded and asserted in debug against the guid. Keep for guid derivation only.
- Version 8 only, asserted; the type was removed from the game.
- No bounds checks (header reinterpreted in place).
- rsx docs for `efct` say "Not yet implemented"; `rpsk` doc is empty.

## Reuse in respak
`rpak.asset` (header bytes, page pointer relocation as in `txtr.rs`), `rpak.guid` for the seven guid fields. All parsing is new and tiny for `rpsk`; nothing for `efct`.

## Tests
- Synthetic `rpsk`: 0x88-byte header with name pointer to `"fx_test\0"`, a 16-byte buffer, distinct guids; assert round-trip. Edge cases: null name, null buffer with size 0, size larger than the page, version != 8.
- Real files: none for `rpsk` (absent from current paks). `efct` samples: `paks/Win64/mp_lobby.rpak` (23), `common_mp.rpak`, `common.rpak`.

## Open questions
- Is `efct` worth reverse engineering without rsx help (PCF/DMX in rpak form)? Likely no.
- `rpsk` could not be found in any current pak (see *Verified on real data*), so `unk_10` / `unk_20` / `unk_78` and the shader asset kind of the five guids cannot be checked on current data; the layout stays rsx-only and unverified.
- Should `rpsk` be exposed at all, or is it only listed (no output)?
- What the `efct` header fields and its CPU data are (only the version and header size are known).
