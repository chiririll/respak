# stlt · Settings layout
**rsx sources:** `game/rtech/assets/settings_layout.h` (`eSettingsFieldType`, `SettingsField_t`, `SettingsFieldMap_t`, `SettingsLayoutHeader_v0_t`), `game/rtech/assets/settings_layout.cpp` (`LoadSettingsLayoutAsset`, `ParseAndSortFields`, `ExportSettingsLayoutAsset`), `core/filehandling/validation.cpp:50-57` · **Games / versions:** Apex Legends, asset version 0 only · **Status:** read + JSON implemented (`respak-respawn/src/stlt.rs`, `stlt_json.rs`), **not yet run against a real pak**.

## Summary
The schema of a [stgs](stgs.md) value buffer: the names, types and offsets of its fields, and the layouts of its arrays. Several `stgs` share one `stlt`.

## Where it lives
rpak asset, FourCC `stlt` (rsx binds `'tlts'`, `headerAlignment = 8`). The header stores a name pointer (rsx adds it to its cache); sub layouts have none. Version 0, 72-byte header; respak refuses other versions.

## Layout
### `SettingsLayoutHeader_v0_t` (0x48)
| Off | Type | Field | Meaning |
|---|---|---|---|
| 0x00 | ptr→cstr | name | null for sub layouts |
| 0x08 | ptr | fieldData | hash table of `hashTableSize` × `SettingsField_t` |
| 0x10 | ptr | fieldMap | `fieldCount` × `SettingsFieldMap_t`, the occupied buckets |
| 0x18 | u32 | hashTableSize | power of 2 |
| 0x1C | u32 | fieldCount | |
| 0x20 | u32 | extraDataSizeIndex | index into a game table; kept as stored |
| 0x24 | u32 | hashStepScale | the game's name-hash lookup; unused here |
| 0x28 | u32 | hashSeed | same |
| 0x2C | i32 | arrayValueCount | 1 for a plain layout, N for a static array, -1 for a dynamic array |
| 0x30 | u32 | totalBufferSize | bytes of one element |
| 0x34 | u32 | unk | 0 so far |
| 0x38 | ptr | stringData | field names and help texts |
| 0x40 | ptr | subHeaders | contiguous array of 72-byte headers, one per sub layout; the count is not stored: `max(valueSubLayoutIdx) + 1` over the table |

Pointers of the asset header are relocated by the volume. Those **inside sub headers** live in `.mem` and are still stored as `{page, offset}` (resolved with `RpakAsset::mem_offset_of`).

### `SettingsField_t` (8)
`u16 dataType`, `u16 nameOffset` (into stringData; 0 = empty bucket), then a `u32`: bits 0–23 `valueOffset`, bits 24–31 `valueSubLayoutIdx`. Types: 0 bool, 1 int, 2 float, 3 float2, 4 float3, 5 string, 6 asset, 7 asset_noprecache, 8 array, 9 array_dynamic.

### `SettingsFieldMap_t` (4)
`u16 fieldBucketIndex`, `u16 helpTextIndex` (a byte offset into stringData despite the name).

## Decoding
Fields are `fieldData[fieldMap[i].fieldBucketIndex]` for `i < fieldCount`, sorted by `valueOffset`. Names are `nameOffset` bytes into stringData (a `u16`, so within the first 64 KiB). Alignment of a layout is the largest of its non-array fields (bool 1; int, floats, dynamic array 4; string, asset 8), over all `hashTableSize` slots, empty ones counting as bool. Element stride is `align(totalBufferSize, alignment)`.

## Output
`stlt → stlt.json`: `name`, `guid`, `layout` { `elementCount`, `totalSize`, `extraDataSizeIndex`, `alignment`, `fields` [{ `name`, `type`, `offset`, `help`, `layout` for arrays }] }. rsx writes a CSV (`fieldName, dataType, layoutIndex, helpText`) plus a JSON of `elementCount`, `extraDataSizeIndex` and `subLayouts` per layout; respak uses one nested JSON instead.

## Differences from rsx / quirks
- rsx dereferences `subHeaders` without a null check when an index above 0 exists; respak returns `Corrupt`.
- Counts, table sizes and nesting are bounded (hash table and field count 2^20, depth 8).
- Writing is not implemented.

## Tests
Unit: stride rounding, name cut at NUL (done). Missing: a synthetic pak with a `stlt` (plain, with a static array, with a nested sub layout), an out-of-range bucket or sub layout → `Corrupt`. Real (ask first): compare `stlt.json` with the rsx CSV for an item flavor layout.

## Open questions
1. ~~Whether sub-header pointers are `{page, offset}` in `.mem`~~ confirmed on `settings_itemtype_weapon_skin_layout.rpak` (0x5A7B54F5DE8F71BB). A null one is all zeros, which is page 0 offset 0 and must be checked before resolving.
2. Whether the field table can hold a name at offset 0 (rsx treats it as empty).
