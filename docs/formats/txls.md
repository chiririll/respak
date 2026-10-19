# txls · Texture list
> **Verified against** Apex `paks/Win64/common_early.rpak` (depot 1172471): header layout (0x18 bytes, version 1), the three fields, name strings, guid array and the guid rule below, checked on all 810 entries.
**rsx sources:** `game/rtech/assets/texture_list.{h,cpp}` (`TextureListHeader_v1_s`, `TextureList_LoadAsset`, `TextureList_PostLoadAsset`, `TextureList_ExportAsset`), `FormatTextureAssetName` in `texture.cpp` · **Games / versions:** Apex Legends, asset version 1 only (still latest at `R5pc_r5-241_J17_CL8728955`) · **Priority hint:** low, S. Small, but useful for naming `txtr` assets (the camo skins list gives names to textures that carry none).

## Summary
A list of texture names (and, in client paks, their guids). The only known instance is the weapon camo skins list. rsx uses it to name the `txtr` assets it references, and can export the names as JSON. A user would convert it to JSON `{ "textures": [...] }`. In respak it could feed a name map for `rpak.guid`.

## Where it lives
rpak asset, FourCC `txls` (rsx binding `'slxt'`), header alignment 8. rsx output prefix `texture_list` (`game/asset.h:185`; the map also has a second `TXLS -> texture_extension` entry that is ignored because `std::map` keeps the first insertion). Name: `texture_list/camo_skins.rpak`, found by hashing that string with `RTech::StringToGuid` and comparing to the asset guid (only if it matches; `s_TextureListCamoSkinsPath`). Otherwise unnamed (`<guid>.txls`). rsx "is pretty sure" only one such asset exists. Server paks hold a variant without guids (see below).

## Versions and detection
Version 1 only; others are an assert + skip in rsx. Proposed: score 10 for kind `txls`, `Version { major: asset.version, minor: 0 }`, else `UnsupportedVersion { txls, <v> }`.

## Layout
Little-endian. Asset header `TextureListHeader_v1_s`, 0x18 bytes:

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | page ptr | `textureGuids` | `u64[numTextures]`; NULL in server paks |
| 0x08 | 8 | page ptr | `textureNames` | array of `numTextures` page pointers, each to a NUL-terminated string |
| 0x10 | 8 | u64 | `numTextures` | count |

Each name is an asset path of a texture without prefix or extension (rsx adds both): `FormatTextureAssetName` yields `"texture/" + name + ".rpak"` unless the name already starts with `texture/` or ends with `.rpak`. The guid of the texture asset is `StringToGuid` of that full name (`texture/<name>.rpak`).

## Verified on real data
`common_early.rpak`: `numTextures` = 810; every `textureGuids[i]` equals `string_to_guid("texture/" + name + ".rpak")` (810 of 810, so respak's `string_to_guid` is the same hash and the assumption in open question 3 holds); every one of the 810 guids is a `txtr` asset in the same pak. Stored names use **backslashes** and no `texture/` prefix or `.rpak` suffix, e.g. `models\camo_skins_r5\weapons\shared\camo_common_01_col`. The asset's own guid is the hash of `texture_list/camo_skins.rpak`, so the hard-coded name is right. Header pointers (relocated) are `.mem` offsets; the name array holds `{page, offset}` pairs.

## Data outside the header
All in pages: the guid array, the pointer array and the strings. No stream, no compression. Note the guid array being a pointer to guids matters at the rpak level: guids stored in an asset's pages are declared as dependencies by the packer, which is why server paks (no textures inside) leave `textureGuids` NULL.

## Dependencies
Client list: `textureGuids[i]` are `txtr` guids (dependencies in the rpak). Server list: only names; rsx derives the guid by hashing the formatted name. Output of the converter does not need the textures. For linking: names map to `rpak.guid` entries of kind `txtr` (project.md 5.5). rsx's `PostLoad` resolves each guid via `FindAssetByGUID` among loaded paks, and names the texture asset `texture/<name>.rpak` (`SetAssetName(.., true)`); missing assets are skipped silently.

## Decoding
1. Read the header; `numTextures` is u64, bound it (reject > e.g. 1 << 20 or past the page).
2. Follow `textureNames`; for each index read the string pointer then the NUL string (bounded, UTF-8 / ASCII).
3. If `textureGuids` is non-null read `numTextures` u64.
4. For each entry: `fullName = format(name)`; guid = stored guid, else `string_to_guid(fullName)` (respak has this in `rpak.guid`; verify it is the same hash).

## Output
rsx export (`TextureList_ExportAsset`): `<export dir>/<parent of asset name>/<file>.json`, written by hand:
```json
{
	"textures": [
		"name/with/slashes",
		"..."
	]
}
```
Names are written raw (the stored name, not the formatted one), run through `FixSlashes` (backslash to slash), no JSON string escaping. Guids are not exported. Proposed respak: `Metadata` (kind `txls`), JSON edge `txls -> txls.json` with `{ "version": 1, "textures": [ { "name": "...", "guid": "0x..." | null } ] }` (guid null when absent, escape strings properly). Also usable as a name source for `txtr` (`rpak.guid`).

## rsx quirks
- Only one asset known; its name is hard-coded.
- Server variant has no guids; rsx hashes names instead.
- Names are used raw in the JSON export but with `texture/` + `.rpak` added when naming the assets.
- No JSON escaping in export.
- The preview prints `%llX` guids; N/A for server paks.

## Reuse in respak
`rpak.asset` (page pointer / string reading), `rpak.guid`, `txtr::read_name` conventions for the `texture/` prefix. New: header parser, JSON writer.

## Tests
Synthetic: header with 2 names + guids; one with NULL guid pointer; `numTextures = 0`; a name already prefixed `texture/` and one already ending `.rpak`; a pointer outside pages. Real: `paks/Win64/common_early.rpak` holds the single `txls`, guid `0x1736EB94052FD8B8` = `string_to_guid("texture_list/camo_skins.rpak")`, version 1, header size 0x18, 810 textures.

## Open questions
1. The server-variant list (NULL `textureGuids`) was not found; not present in the Apex depot 1172471 paks examined (only `common_early.rpak` has a `txls`). Unverified.
2. Should respak add the names to a global name map or only emit JSON?
