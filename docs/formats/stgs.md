# stgs · Settings
**rsx sources:** `game/rtech/assets/settings.{h,cpp}` (`SettingsAssetHeader_v1_t`, `_v2_t`, `SettingsMod_s`) · **Games / versions:** Apex Legends, header of 72 bytes (v1) or 80 bytes (v2) · **Priority hint:** built (M). This spec describes `crates/respak-respawn/src/assets/stgs/`.

## Summary
A settings object (weapon / item values and mods). It holds a raw value buffer; the `stlt` it names gives the field names and types, so respak outputs the decoded values as JSON plus the raw bytes and the mod table.

## Where it lives
rpak asset, FourCC `stgs`, extension `stgs`; the name is stored in the header (`name` pointer; empty = unnamed). No streamed data.

## Versions and detection
Score 10 for kind `stgs`. The layout is picked by the header length, not the asset version (as in rsx): 72 -> v1, 80 -> v2, other lengths -> `UnsupportedVersion { "header of N bytes" }`. Format id `stgs@1` / `stgs@2`.

## Layout
Little-endian; pointers are page pointers (0 or `u64::MAX` = none).

| Off v1 | Off v2 | Type | Field |
|---|---|---|---|
| 0x00 | 0x00 | u64 | guid of the `stlt` (0 = none) |
| 0x08 | 0x08 | ptr | value buffer |
| 0x10 | 0x10 | ptr | name |
| 0x18 | 0x18 | ptr | string blob (mod strings) |
| none | 0x20 | 8 bytes | unknown (v2 only) |
| 0x20 | 0x28 | u32 | `uniqueID` (item flavor id) |
| 0x24 | 0x2C | 4 bytes | padding |
| 0x28 | 0x30 | ptr | mod names (array of string pointers) |
| 0x30 | 0x38 | ptr | mod values (array of `SettingsMod_s`) |
| 0x38 | 0x40 | u32 | value buffer size |
| 0x3C | 0x44 | u32 | `singlePlayerModCount` (JSON `modFlags`) |
| 0x40 | 0x48 | u32 | mod name count |
| 0x44 | 0x4C | u32 | mod value count |

`SettingsMod_s` (12 bytes): `0x00 u16 nameIndex`, `0x02 u16 type` (0 int_add, 1 int_multiply, 2 float_add, 3 float_multiply, 4 bool, 5 number, 6 string), `0x04 u32 valueOffset` (absolute offset of the target field in the value buffer), `0x08 u32 value` (union: int, float bits, bool in the low byte, or string offset into the string blob).

## Data outside the header
All in `.mem`. A value buffer over 16 MiB or a count over 16M is `Corrupt`; strings are at most 4 KiB and must end inside `.mem`. No compression.

## Dependencies
The `stlt` by guid (`dependencies()` returns it when non-zero). Resolved with `matl::locate(guid, "stlt", pak)`; when not found (`NotFound` / `MissingDependency`) `layoutName` and `settings` stay `null` and the raw values are still output.

## Decoding
Read the header, name, values, mod names and mods (a string mod reads `strings + raw`). If the `stlt` is found, decode the buffer recursively by its `Layout`: bool (1 byte), int, float, float2 / float3 (JSON arrays), string / asset / asset_noprecache (8-byte pointer slot, text through `.mem`, `null` if unresolved); static `array`: elements start at the field offset rounded up to the sub layout's alignment, stride = `total_size` rounded up to alignment, count = sub `element_count`; `array_dynamic`: `{i32 count, u32 offset}` in the buffer, elements at `offset` from the buffer start. Nesting beyond 8 or an array that cannot fit in the buffer is `Corrupt`. Floats print with their shortest f32 digits.

## Output
`Metadata`, `NameSource` (every non-empty string at any depth of `settings`, plus string mods). Edge `stgs -> stgs.json` (no options): `name`, `guid`, `version`, `layout` (guid), `layoutName`, `uniqueId`, `settings`, `values` (buffer as hex), `modFlags`, `modNames`, `modValues` (`index`, `name`, `type`, `offset`, `raw` as `0x%08X`, `value` int / float / bool / string, `null` for `number` and unknown types). No `Table` capability.

## rsx quirks
rsx spells the mod types `int_multipy` / `float_multipy`; respak writes `int_multiply` / `float_multiply`. The header is chosen by size. The type of a `number` mod depends on the target field and is not resolved (`null`). The v2 unknown 8 bytes and the v1 padding are ignored.

## Reuse in respak
`stlt` reader (`StltAsset::layout`), `matl::locate` / `pak_of`, `Reader`, mediator `read_as`.

## Tests
`stgs/tests.rs`: string collection, decoding of scalars / vectors / static arrays, dynamic array out of range. Gaps: no header parse test, no read-edge or JSON test, no `PakBuilder` fixture, no real-pak test.

## Open questions
1. Read-edge and JSON tests are missing. 2. `number` mods unresolved. 3. Meaning of the v2 extra 8 bytes and of `singlePlayerModCount` (`modFlags`). 4. Which paks the layout lookup searches (whatever `locate` searches; not checked against a real pak where the `stlt` lives in `common.rpak`).
