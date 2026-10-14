# impa · Impact definition
**rsx sources:** `game/rtech/assets/impact.h` (includes `weapon_definition.h`: "Impact Definitions use the same structures as weapon definitions"), `game/rtech/assets/impact.cpp` (`LoadImpactAsset`, `R_GetImpactDefinitionAsString`, `ExportImpactAsset`, `InitImpactAssetType`), `core/main.cpp:97, 169` · **Games / versions:** Apex Legends; no version check · **Priority hint:** low–medium value (impact effects table), **S** on top of [wepn](wepn.md) (same structs, same writer).

## Summary
**An rpak asset, not VPK** (grouped under "vpk" in `asset.h:67` like `wepn`). The compiled form of an impact effect keyvalue table (surface → effect/sound). Same tree of blocks/key-values as `wepn`.

## Where it lives
rpak asset, FourCC `impa` (`asset.h:67`), binding `'apmi'` (`impact.cpp:109`), `headerAlignment = 8`. rsx name: `impact/<weaponName field>.rpak` (`impact.cpp:14`), not cached.

## Versions and detection
No version check in rsx; read as `WepnAssetHeader_v1_t`. Proposed: kind `impa` → 10, versions as found (open question), header ≥ 0x20.

## Layout
Identical to [wepn.md](wepn.md) §Layout: `WepnAssetHeader_v1_t` (0x20, `weaponName` holds the impact table name), `WepnData_v1_t` (0x30), `WepnKeyValue_v1_t` (0x18), `valueType` 0/1/2 = int/float/string (`weapon_definition.h:4-53`).

## Data outside the header
Page data only; no streaming, no compression.

## Dependencies
None followed by rsx (effect/sound names are plain strings).

## Decoding
Same recursive walk as `wepn`.

## Output
rsx writes `<export>/impact/<name>.txt` (`impact.cpp:62-101`), same text as `wepn` **without** the `// val unk` / `// child unk` comment lines (`impact.cpp:18-60`). Proposed: `impa → impa.txt`, `impa → impa.json`, sharing the `wepn` code with a "comments" switch.

## rsx quirks
Same as wepn: export returns `false` on success, `%f` floats, broken quoting for unknown types, no bounds checks. Code duplicated from `weapon_definition.cpp`: one shared module in respak.

## Reuse in respak
The `wepn` module (structs, walker, writers).

## Tests
Same synthetic tree as wepn read as `impa`; check that no comment lines appear. Real: Apex impact assets (paks unknown).

## Open questions
1. Asset versions in the wild; does the `unk_20`/`unk_28` data exist for impacts too.
2. Merge `wepn` and `impa` into one slice (recommended).
