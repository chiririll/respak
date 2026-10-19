# odl · On-demand-loaded assets (`odla`, `odlc`, `odlp`)
> **Verified against** Apex `paks/Win64/common.rpak` (depot 1172471): all 3907 `odla` and 1375 `odlp` assets. `odlc` was **not** checked (it lives in `startup.rpak`, which could not be read).
**rsx sources:** `game/rtech/assets/odl_asset.h` (`ODLAssetHeader_t`, `ODLAsset`), `game/rtech/assets/odl_asset.cpp` (`ODLAsset::ODLAsset`, `LoadODLAsset`, `ExportODLAsset`, `PreviewODLAsset`, `InitODLAssetType`), `game/rtech/assets/odl_pak.h` (`ODLPakHeader_t`), `game/asset.h:74-76, 155-157, 229-231`, `core/features.h:8` (`HAS_ODL_ASSET`), `core/main.cpp:92, 164` · **Games / versions:** Apex Legends rpaks that stream models/stickers on demand · **Priority hint:** low-medium; **S** (a tiny JSON pointer record). Useful only as dependency metadata.

> **Scope warning.** Only `odla` is implemented. `odlp` has a header struct (`ODLPakHeader_t`) that `odla` reads through, but no type registration; `odlc` has a FourCC and a display name ("odl_ctx") only and is **not supported by rsx** (layout unknown).

## Summary
"On-demand loaded" assets are stubs in a base rpak that stand in for a real asset living in a different, later-loaded rpak. `odla` names that asset and its guid, `odlp` describes the pak that contains it, and `odlc` is a context asset of unknown purpose. A user would convert an `odla` to a small JSON record (original asset name, guid, containing pak).

## Where it lives
- **rpak assets.** FourCCs `odla`, `odlc`, `odlp` (`asset.h:74-76`). Display names: "odl_asset", "odl_ctx", "odl_pak". rsx registers the `odla` binding with `.type = 'aldo'` (multi-char literal = file bytes `o d l a`), name "On Demand Loaded Asset", header alignment 8. `odlp` and `odlc` have no binding.
- Name in the rpak volume: `odla` is named `odl_asset/<assetName>` where `<assetName>` is the string the header points to (`LoadODLAsset`, `odl_asset.cpp:41`); the name is **stored** (via the header's `assetName` pointer). For `odlp`/`odlc`: no name rule in rsx (unknown; `odlp` is found only by guid).
- Export path in rsx: `<export>/odl_asset/<parent of name>/<file>.json`; note the name already starts with `odl_asset/` so the directory is doubled (see quirks).
- No streamed data.

## Versions and detection
- No version switch anywhere; the header is a single layout (`ODLAssetHeader_t`, `static_assert` 0x28). The asset version is never inspected. Real data: `odla` is version 2, `odlp` version 1 (the only versions in `common.rpak`).
- Proposed score: rpak asset FourCC `odla` → 10; header size == 0x28 → +2. `Version { major: asset_version, minor: 0 }`; any size other than 0x28 → `UnsupportedVersion`.
- `HAS_ODL_ASSET` is a compile-time define (`features.h:8`); with it off, nothing is registered.

## Layout
### `ODLAssetHeader_t` (`odl_asset.h:3-14`, 0x28 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | u64 | odlPakAssetGuid | guid of the `odlp` asset describing the pak that holds the real asset |
| 0x08 | 8 | page ptr | assetName | `char*`, NUL string: name of the asset `originalAssetGuid` refers to |
| 0x10 | 8 | u8[8] | unk10 | unknown |
| 0x18 | 8 | u64 | originalAssetGuid | guid of the real asset |
| 0x20 | 8 | u64 | placeholderAssetGuid | guid of a stand-in asset used while the real pak loads (`*_loading.rmdl` for models); **0 for sticker ODL assets**. Models only per comment |

### `ODLPakHeader_t` (`odl_pak.h:3-6`, 0x18 bytes) — the `odlp` header
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | page ptr | pakName | `char*`, NUL string: file name of the pak holding the on-demand assets |
| 0x08 | 0x10 | u8[16] | gap8 | unknown |

### `odlc`
No struct. Unknown.

## Verified on real data
`common.rpak`:
- **`odla`: asset version 2** (not unversioned), entry header size 0x28 (40) in all 3907. Layout confirmed: `odlPakAssetGuid` @0, `assetName` page pointer @8, `unk10` @0x10 (**all zero** in 3907), `originalAssetGuid` @0x18, `placeholderAssetGuid` @0x20 (zero for 2296, non-zero for 1611: sticker assets have 0, model assets (`mdl\techart\...*_w.rmdl`) have a placeholder, as the header comment says).
- `assetName` is stored with **backslashes** (`material\stickers\epic\v24\onesies_01\sticker_epic_v24_onesies_01_rgdp.rpak`, `mdl\techart\mshop\characters\legends\bloodhound\bloodhound_v21_pilot_level03_w.rmdl`). **`originalAssetGuid == string_to_guid(assetName)`** for all 3907 (`string_to_guid` already maps `\` to `/`), so the stored name can be verified the same way as `subt`, and the name is the real name of the target asset. The `odla`'s own guid is not the hash of the stored name nor of `odl_asset/<name>` (0 of 400).
- The `odlp` named by `odlPakAssetGuid` is **in the same pak** for all 3907 `odla`.
- **`odlp`: asset version 1**, entry header size 0x18 (24). `pakName` is a page pointer to a NUL-terminated pak **file name** (e.g. `material_stickers_epic_v24_onesies_01_sticker_epic_v24_onesies_01.rpak`, `root_lgnd_skins_techart_mshop_characters_legends_loba_loba_v22_ophitman_w.rpak`); each of 400 sampled names exists as `paks/Win64/<name>` in the depot (case-insensitively; the case in the string differs from the file's, e.g. `Humans`). `odlp`'s own guid is not the hash of the pak name.
- `ODLPakHeader_t.gap8` (16 bytes at 0x08) takes only **two values** in 1375 samples, each = `u64 constant` + `u64 1`: `b6cab32dda502482` (801 assets, all `root_lgnd_skins_*` model paks) and `4525a9dcf293b9de` (574 assets, all `material_stickers_*` paks). So it looks like a category/type key (model-skin pak vs sticker pak) followed by the number 1, not padding.

## Data outside the header
Only the two NUL strings (`assetName`, `pakName`), which live in the rpak's page data and are reached through page pointers. No starpak, no compression of its own.

## Dependencies
- `odlPakAssetGuid` → the `odlp` asset (looked up by guid with `g_assetData.FindAssetByGUID`, only in already-loaded paks). If absent, `pakName` is unavailable (rsx prints `(unknown)`).
- `originalAssetGuid` → the real asset, in another pak (not necessarily loaded). Used only in preview.
- `placeholderAssetGuid` → placeholder model asset (0 = none). Not resolved by rsx.
- respak mapping: these are cross-pak links by `rpak.guid`, not required for the JSON output; an unresolved `odlp` should leave `pak` as `null` rather than a `MissingDependency` error.

## Decoding
```
odla(header @ page ptr):
  name   = cstr(header.assetName)                    // page pointer
  pakGuid, origGuid, placeholderGuid = header.{0x00, 0x18, 0x20}
  asset.name = "odl_asset/" + name
  odlp = find_asset_by_guid(pakGuid)                 // may be missing
  pak  = odlp ? cstr(ODLPakHeader_t(odlp.header).pakName) : unknown
```
Nothing else is decoded.

## Output
rsx export (`ExportODLAsset`): one JSON file per `odla`, written as a hand-formatted string:
```json
{
	"odlGuid": "<hex of the odla asset's own guid, std::format("{:x}")>",
	"name": "<original asset name>",
	"pak": "<pak file name or (unknown)>"
}
```
Strings are not escaped. `originalAssetGuid`, `placeholderAssetGuid` and `odlPakAssetGuid` are **not** written. Preview: delegates to the preview of the original asset if it is loaded and its post-load finished, else a message "Can't preview this asset: you must also select '<pak>'".
Proposed respak: format `odla` → JSON (new small `OdlRef` JSON capability or plain `Blob` JSON). Suggested shape (superset of rsx, hex guids as strings): `{ "guid", "name", "pak", "pak_guid", "original_guid", "placeholder_guid" }`. `odlp` → `{ "pak": "<name>" }` (same JSON family). `odlc` → none.

## rsx quirks
- Export path doubles `odl_asset/` (name already has the prefix plus `exportPath.append("odl_asset/")`).
- `LoadODLAsset` allocates `new ODLAsset` and attaches it as extra data; never freed by this code.
- `ODLAsset` constructor takes the `odlp` at construct time; if its pak is loaded later, `originalAssetPakName` stays null forever.
- JSON built by string concatenation without escaping; `GetOriginalAssetName()` is dereferenced unconditionally.
- The header comment that placeholder is used "only for model assets" is an assumption.
- Preview keeps a function-static `lastPreviewedODLAsset`.
Keep: the layouts and name rule. Drop: the export path doubling, the leaks.

## Reuse in respak
`rpak.asset` (page pointer resolution of the two strings), `rpak.guid`, simple JSON writer. New: the `odla`/`odlp` readers (tiny) and an optional cross-pak guid lookup for `pak`.

## Tests
- Synthetic `odla` header (0x28 bytes): `odlPakAssetGuid=0x1111`, `assetName` → "models/x.rmdl", `originalAssetGuid=0x2222`, `placeholderAssetGuid=0`; matching `odlp` header (0x18) with `pakName` → "x.rpak". Expect JSON as above and name `odl_asset/models/x.rmdl`. Without the `odlp` asset: `pak` unknown, no error.
- Edge: null `assetName` pointer (`Corrupt`), header shorter than 0x28, non-UTF-8 name.
- Real: `paks/Win64/common.rpak` (3907 `odla`, 1375 `odlp`).

## Open questions
1. What is `odlc` (layout, purpose)? Needs reverse engineering outside rsx. Three `odlc` exist in `startup.rpak`, which could not be read (its chunks were not cached and the Steam login had expired), so nothing was checked.
2. Meaning of `ODLPakHeader_t.gap8` (two constants, see *Verified on real data*) and of `unk10` (always zero in 3907 samples).
3. Should respak resolve `pak` across multiple loaded rpaks, and should the output keep `pak_guid`/`original_guid` (rsx drops them)? (On real data the `odlp` is always in the same pak as the `odla`.)
4. Use of `odla` as a link target for dependency resolution (project.md §5.5) rather than a standalone export? The guid relation below makes that possible.
5. How the guids of an `odla` / `odlp` asset itself are derived (neither equals the hash of the stored name or of `odl_asset/<name>`).
