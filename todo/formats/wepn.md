# wepn · Weapon definition
**rsx sources:** `game/rtech/assets/weapon_definition.h` (`WepnValueType_t`, `WepnValue_u`, `WepnKeyValue_v1_t`, `WepnData_v1_t`, `WepnAssetHeader_v1_t`), `game/rtech/assets/weapon_definition.cpp` (`LoadWeaponDefinitionAsset`, `R_GetWeaponDefinitionAsString`, `ExportWeaponDefinitionAsset`, `InitWeaponDefinitionAssetType`), `core/main.cpp:96, 168` · **Games / versions:** Apex Legends; layout named "v1", **rsx checks no version** · **Priority hint:** medium value (weapon stats), **S**. Shares its structs and writer with [impa](impa.md): implement both in one slice.

## Summary
**An rpak asset, not VPK.** `asset.h:66` lists it under "vpk" only because the data is the compiled form of the Source/Respawn `scripts/weapons/*.txt` keyvalue files that used to live in VPKs. It is a tree of named blocks, each with typed key/value pairs (int, float, string) and child blocks. User output: KeyValues text (`.txt`), or JSON.

## Where it lives
rpak asset, FourCC `wepn` (`asset.h:66`), binding `'npew'` (`weapon_definition.cpp:111`), `headerAlignment = 8`. Name not stored as a path: rsx builds `weapon/<weaponName>.rpak` from the header (`weapon_definition.cpp:14`), not added to the cache, so it does not hash to the GUID (unverified; the real asset name is unknown). Proposed volume path: by GUID as now; the `Metadata` name = `weapon/<weaponName>`.

## Versions and detection
rsx registers no version check; all assets are read as v1 (`weapon_definition.cpp:6-16`). Asset versions seen: unknown. Proposed: kind `wepn` → 10; accept the asset version(s) found on real paks (open question), `Version { major: v, minor: 0 }`; header size must be ≥ 0x20, else `Corrupt`.

## Layout
Natural alignment; all `ptr` are page pointers.

### `WepnAssetHeader_v1_t` (`weapon_definition.h:44-53`, 0x20)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr→cstr | weaponName | |
| 0x08 | 8 | ptr→`WepnData_v1_t` | rootKey | root block |
| 0x10 | 8 | ptr | unk_10 | "mostly null" |
| 0x18 | 8 | ptr | unk_18 | "mostly null" |

### `WepnData_v1_t` block (`weapon_definition.h:29-42`, 0x30)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr→cstr | name | block name |
| 0x08 | 4 | u8[4] | unk_8 | unknown |
| 0x0C | 2 | u16 | numValues | key/values in this block |
| 0x0E | 2 | u16 | numChildren | child blocks |
| 0x10 | 8 | ptr | childKVPairs | `numValues` × `WepnKeyValue_v1_t` |
| 0x18 | 8 | ptr | childObjects | `numChildren` × `WepnData_v1_t` |
| 0x20 | 8 | ptr→u16[] | unk_20 | one u16 per value; meaning unknown |
| 0x28 | 8 | ptr→u16[] | unk_28 | one u16 per child; meaning unknown |

### `WepnKeyValue_v1_t` (`weapon_definition.h:20-27`, 0x18)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr→cstr | key | |
| 0x08 | 4 | u8[4] | unk_8 | unknown |
| 0x0C | 4 | u32 | valueType | `0` INTEGER, `1` FLT, `2` STRING (`weapon_definition.h:4-9`) |
| 0x10 | 8 | union | value | INTEGER: u64; FLT: f32 in the low 4 bytes; STRING: page ptr→cstr |

## Data outside the header
All page data (`.mem`). No streaming, no compression.

## Dependencies
None.

## Decoding
Recursive walk from `rootKey`; bounds-check every pointer and count, cap recursion depth (rsx doesn't). Only follow `value` as a pointer when `valueType == 2`.

## Output
rsx writes `<export>/weapon/<weaponName>.txt` (or the name's parent dir with `exportPathsFull`) (`weapon_definition.cpp:64-103`):
```
<ind>"<name>"
<ind>{
  per value: <ind>\t// val unk: <unk_20[j]>
             <ind>\t"<key>" "<value>"          int: decimal u64; float: std::to_string (%f, 6 decimals); string as is
             unknown type: "<key>" ""unk" // <key>: unknown. rawval: <HEX>"
  blank line
  per child: <ind>\t// child unk: <unk_28[i]>
             <recursive with ind + \t>
             blank line between children
<ind>}
```
Proposed respak: `wepn` from `rpak.asset`, `Metadata`; edges `wepn → wepn.txt` (KeyValues, rsx-compatible, the `// unk` comments optional) and `wepn → wepn.json` (`{ "name", "values": [{ "key", "type", "value", "unk" }], "children": [...] }`). Shared code with `impa`.

## rsx quirks
- Export returns `false` even on success. Strings and floats not escaped/round-trippable (`%f` loses precision; prefer shortest round-trip floats, list it as a behaviour change).
- The unknown-type branch emits broken quoting. No bounds or depth checks. Name `.rpak` suffix is odd.

## Reuse in respak
`rpak.asset` + `.mem` pointer reads (as `matl`, `uiia`), `Metadata`; JSON pattern from `matl_json.rs`.

## Tests
Synthetic: root with one int, one float, one string and one child with a value; zero values/children; unknown type 3; pointer out of `.mem` → `Corrupt`. Real (no Steam without permission): Apex `common.rpak` weapon definitions; diff the text with rsx.

## Open questions
1. Asset versions in the wild and whether the layout ever changed.
2. Meaning of `unk_8`, `unk_20`/`unk_28` (sort/hash index?), `unk_10`/`unk_18`.
3. Text output exactly like rsx (with comments) or clean KeyValues + JSON?
4. Real asset name (to hash to the GUID).
