# rtk · UI RTK
**rsx sources:** `game/rtech/assets/ui_rtk.{h,cpp}` (`RTKAssetHeader_v2_t`, `RTKAssetHeader_v2_1_t`, `RTKAssetHeader_v2_2_t`, `LoadRTKAsset`, `ExportRTKAsset`), `game/asset.h` (`AssetType_t::RTK`, prefix `ui_rtk`), `docs/assets/rpak/RTK.md` · **Games / versions:** Apex Legends only (not Titanfall 2), asset version 2, three header sizes (season 16, 16.1, later) · **Priority hint:** low (S). A text blob in a small header.

## Summary
An RTK element script: the source text of a UI element (stored as text; the text language is not documented by rsx). A user converts it to a `.rtk` text file. The optional guid array lists assets the script uses.

## Where it lives
rpak asset, FourCC `rtk` (stored `72 74 6B 00`, rsx `MAKEFOURCC('r','t','k','\0')`, binding `'\0ktr'`), extension `rtk`. rsx names it `ui_rtk/<elementName>.rpak` (`elementName` e.g. `stats_menu\dropdown_button`, so the pak name contains backslashes; normalise to `/`: `ui_rtk/<elementName>`, then `.rtk`). The name is stored in the header (`elementName`), so no guid lookup is needed. `fullScriptPath` is the original depot path (e.g. `content\r2\rtk\stats_menu\dropdown_button.rtk`). No starpak data.

## Versions and detection
Asset version must be 2 (rsx asserts; other values are not handled). The layout is told apart by the asset header size (`headerStructSize`): `0x20` = v2.0 (S16), `0x28` = v2.1 (S16.1, adds `guids`), `0x30` = v2.2 (adds `unk_20`). Other sizes: rsx logs once and drops the asset. Proposed: score kind `rtk` = 10, `Version { major: 2, minor: 0/1/2 }` by header size (0x20/0x28/0x30); version != 2 or other header size -> `UnsupportedVersion { rtk, <v> }`. Format ids `rtk@2.0`, `rtk@2.1`, `rtk@2.2`.

## Layout
Little-endian, 64-bit page pointers (relocated).

v2.0, `0x20` bytes:

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | page ptr | `elementName` | NUL string, short path |
| 0x08 | 8 | page ptr | `fullScriptPath` | NUL string, depot path |
| 0x10 | 4 | i32 | `elementDataSize` | script size in bytes |
| 0x14 | 4 | i32 | `unk_1C` | unknown |
| 0x18 | 8 | page ptr | `elementData` | script text |

v2.1, `0x28` bytes: 0x00 `elementName`, 0x08 `fullScriptPath`, 0x10 page ptr `guids` (array of asset guids; count not in the header, see open questions), 0x18 i32 `elementDataSize`, 0x1C i32 `unk_1C`, 0x20 page ptr `elementData`.

v2.2, `0x30` bytes: as v2.1 up to 0x1C; 0x20 i64 `unk_20` (unknown); 0x28 page ptr `elementData`.

## Data outside the header
The names, the guid array and the script text are page data (`elementData` points to `elementDataSize` bytes). Not compressed here; no starpak. Whether the text is NUL-terminated and whether `elementDataSize` includes the terminator is unknown (rsx preview prints it with `TextUnformatted`, which needs a terminator; export writes exactly `elementDataSize` bytes).

## Dependencies
v2.1/2.2 `guids`: array of asset guids used by the script (rsx keeps the pointer but never reads it; length unknown). Not needed to write the text. Could map to respak links (project.md §5.5, `rpak.guid`) once the count is known.

## Decoding
1. Pick the layout by header size; read `elementName`, `fullScriptPath`, `elementDataSize`, `elementData` (offset per layout).
2. Check `0 <= elementDataSize` and that the range lies in the page data (else `Corrupt`).
3. The output is the raw bytes `elementData[0..elementDataSize]`.

## Output
rsx writes the raw bytes to `<export>/<ui_rtk or full path>/<name>.rtk` (`ExportRTKAsset`; extension forced to `rtk`; no settings). Proposed respak: capability `Text` (or the existing text/bytes capability) with the script, `Metadata` (name, kind `rtk`, guid); edge `rtk -> rtk` writing the text. If JSON is wanted: `name`, `guid`, `version`, `fullScriptPath`, `guids` (hex strings, when known), `script` (file name).

## rsx quirks
Version minor is derived from header size and written back with `SetAssetVersion({2,1/2})`; the asset is registered as v2 first. Name gets a spurious `.rpak` suffix and keeps backslashes. Header field names `unk_1C` do not match their offsets (0x14 in v2.0). `elementDataSize` is an `int` widened to `int64_t` in `RTKAsset`. `guids` is never read; `unk_20` never read. `RTK.md` in rsx docs is a stub (only: not in Titanfall 2).

## Reuse in respak
`rpak.asset` (`detect_kind`, `rpak_asset`, header size from `asset.data()`), `txtr::read_name`-style NUL string reader, the text output path used by other text kinds (e.g. `subt` / `stgs` if they emit text). New: header-size dispatch and the byte copy.

## Tests
Synthetic: three headers (0x20/0x28/0x30) with a short script, check the text and name; header size 0x38 -> `UnsupportedVersion`; version 3 -> `UnsupportedVersion`; `elementDataSize` past the page -> `Corrupt`; negative size; backslash name. Real files: Apex `rtk` assets (e.g. `ui_rtk/...` in the UI paks of S16+); no Steam runs without permission.

## Open questions
1. Is `elementData` NUL-terminated, and does `elementDataSize` include it? Strip a trailing NUL on export?
2. How many guids in the `guids` array (v2.1/2.2), and is `unk_1C` / `unk_20` a count?
3. Names: keep backslashes or convert to `/`, and keep or drop the `.rpak` suffix quirk?
