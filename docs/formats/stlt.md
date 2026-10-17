# stlt · Settings layout
**rsx sources:** `game/rtech/assets/settings_layout.{h,cpp}` (`SettingsLayoutHeader_v0_t`, `SettingsField_t`, `SettingsLayoutAsset`) · **Games / versions:** Apex Legends, asset version 0 (72-byte header) · **Priority hint:** built (S). This spec describes `crates/respak-respawn/src/assets/stlt/`.

## Summary
The schema of a `stgs` value buffer: a hash table of fields (name, type, offset) plus sub layouts for arrays. respak decodes it into a `Layout` tree, used by `stgs`, and exports it as JSON.

## Where it lives
rpak asset, FourCC `stlt`, extension `stlt`; the name is stored in the header (`name` pointer; sub layouts have none). Found from a `stgs` by guid. No streamed data.

## Versions and detection
Score 10 for kind `stlt`. An asset version other than 0 is `UnsupportedVersion`; a header length other than 72 is `Corrupt`. Format id `stlt@0`.

## Layout
Little-endian, 72-byte header (the same struct is the stride of the sub layout array in `.mem`):

| Off | Type | Field |
|---|---|---|
| 0x00 | ptr | name |
| 0x08 | ptr | field table: `hashTableSize` slots of 8 bytes |
| 0x10 | ptr | field map: `fieldCount` entries of 4 bytes |
| 0x18 | u32 | hashTableSize |
| 0x1C | u32 | fieldCount |
| 0x20 | u32 | extraDataSizeIndex (kept as stored) |
| 0x24 | u32 | hashStepScale (ignored) |
| 0x28 | u32 | hashSeed (ignored) |
| 0x2C | i32 | arrayValueCount (1 plain, N static array, -1 dynamic array) |
| 0x30 | u32 | totalBufferSize (bytes of one element) |
| 0x34 | u32 | unknown (0 so far) |
| 0x38 | ptr | string data (names, help texts) |
| 0x40 | ptr | sub layout headers (array of 72-byte headers) |

Field slot (8 bytes): `u16 type`, `u16 nameOffset` (into string data), `u32` = `valueOffset` (low 24 bits) | `subLayoutIndex` (high 8 bits). Empty slots exist. Field map entry (4 bytes): `u16` slot index, `u16` help text offset (into string data). Types: 0 bool, 1 int, 2 float, 3 float2, 4 float3, 5 string, 6 asset, 7 asset_noprecache, 8 array, 9 array_dynamic (other values are `Corrupt`). Alignment: bool 1; int, float, float2, float3, array_dynamic 4; string, asset, asset_noprecache 8; array none. Layout alignment = maximum over every slot's type (empty slots included), minimum 1. Element stride = `totalBufferSize` rounded up to the alignment. Sub layout count = highest sub index of any slot + 1.

## Data outside the header
`.mem` only. Name window: up to `0x10000 + 256` bytes from the string data. Table size or field count over `1<<20` is `Corrupt`; sub layout nesting above 8 is `Corrupt`. Sub headers live in `.mem`, so their pointers are still `{page, offset}` and go through `mem_offset_of` (an all-zero slot is null).

## Dependencies
None.

## Decoding
Read the table and map, derive alignment and sub count from all slots, build the `Field`s from the map (the slot index must be below `hashTableSize`), sort by offset, recurse into the sub headers. A field naming a sub layout when none exist is `Corrupt`. Names and help are NUL strings (lossy UTF-8; a bad offset gives an empty string).

## Output
`Metadata` only (no value capability). Edge `stlt -> stlt.json` (no options): `name`, `guid`, `layout` = `{elementCount, totalSize, extraDataSizeIndex, alignment, fields: [{name, type, offset, help, layout?}]}`; `layout` is nested the same way for `array` / `array_dynamic` fields (`null` if the index is missing). Type names: `bool`, `int`, `float`, `float2`, `float3`, `string`, `asset`, `asset_noprecache`, `array`, `array_dynamic`.

## rsx quirks
The hash parameters are not needed: respak walks the field map instead of hashing names. Field order comes from sorting by offset.

## Reuse in respak
`rpak.asset` helpers, `rmdl::reader`; consumed by `stgs`.

## Tests
`stlt/tests.rs`: stride rounding, the name helper. Gaps: no header / read-edge / JSON fixture, no real-pak test (only used indirectly by `stgs`, which has none either).

## Open questions
1. Fixture-based read tests and a real pak check. 2. Meaning of `extraDataSizeIndex` and of header `0x34`. 3. Other asset versions (none known).
