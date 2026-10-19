# font · UI font atlas
**rsx sources:** `game/rtech/assets/ui_font_atlas.{h,cpp}` (`UIFontAtlasAssetHeader_v6_t` / `_v12_t`, `UIFontHeader_v6_t` / `_v7_t` / `_v12_t`, `UIFontTexture_v6/v7/v12_t`, `UIFontUnicodeMap_t`, `LoadUIFontAtlasAsset`, `PostLoadUIFontAtlasAsset`, `UIFontHeader::TextureFromUnicode` / `TextureFromUnicodeMap` / `TextureFromGlyph` / `ParseProportions`, `UIFontCharacter_t::SetBounds`, `ExportUIFontAtlasAsset`) · **Games / versions:** asset versions 6 (Titanfall 2 tech test `r2tt`), 7 (R2 + Apex s0-s15.1), 10 (Apex s16), 12 (Apex s16.1 to retail); 8, 9, 11 skipped by the game · **Priority hint:** medium-low, M. Glyph-lookup tables are non-trivial; the output is only a glyph atlas split into per-character images. Not a TTF font: the glyphs are pre-rendered into a `txtr`.

## Summary
A UI font atlas: one big texture (a `txtr` referenced by guid) holding pre-rendered (signed-distance style, treated as plain pixels by rsx) glyph images, plus for each font a table mapping a Unicode code point (or, in v12, a glyph index) to a rectangle in that texture. A user would convert it to the atlas image plus a per-character JSON (code point, rectangle), or to per-character images.

## Where it lives
rpak asset, FourCC `font` (rsx binding `'tnof'`), header alignment 8; rsx path prefix `ui_font_atlas` (`game/asset.h:189`). Name: from the referenced atlas texture's name: `ui_font_atlas/<txtr name>.rpak` (`PostLoadUIFontAtlasAsset`; rsx asserts that `StringToGuid(name)` equals the asset guid). If the texture has no name, `SetAssetNameFromCache()`. Fonts inside the asset carry their own name string (`name` ptr, may be NULL; rsx falls back to `font_<fontIndex>` in preview, `unnamed_<i>` in export). Name in the volume: `ui_font_atlas/<txtr name>.rpak`, found by stored name via the atlas txtr, else guid only.

## Versions and detection
| Asset version | Asset header | Font header | Texture entry |
|---|---|---|---|
| 6 | v6 (0x28) | `UIFontHeader_v6_t` 0x40 | `_v6_t` 0x20 + unicode maps |
| 7, 10 | v6 (0x28) | `UIFontHeader_v7_t` 0x60 | `_v7_t` 0x20 + chunk tables |
| 12 | v12 (0x68) | `UIFontHeader_v12_t` 0x90 | `_v12_t` (0x24 by field layout, see quirks) + glyph tables |

Selected by `pakAsset->version()` only (`LoadUIFontAtlasAsset`); other versions are an assert and the asset is skipped. Proposed: score 10 for kind `font`, `Version { major: asset.version, minor: 0 }` for 6/7/10/12, else `UnsupportedVersion { font, <v> }`. Priority order for support: 12 (retail), then 7/10, then 6.

## Layout
Little-endian; pointers are page pointers (8 bytes, relocated). Text or enums not listed are in the rsx header comments.

### Asset header, versions 6, 7, 10 (`UIFontAtlasAssetHeader_v6_t`, 0x28)
| Off | Type | Field | Meaning |
|---|---|---|---|
| 0x00 | u16 | `fontCount` | fonts in the atlas |
| 0x02 | u16 | `unk_2` | count of the data at `unk_18` (unknown) |
| 0x04 | u16 | `width` | atlas width in px |
| 0x06 | u16 | `height` | atlas height in px |
| 0x08 | f32 | `widthRatio` | "like ui image atlas" (probably 1/width) |
| 0x0C | f32 | `heightRatio` | probably 1/height |
| 0x10 | ptr | `fonts` | array of font headers (`fontCount`) |
| 0x18 | ptr | `unk_18` | unknown data |
| 0x20 | u64 | `atlasGUID` | guid of the atlas `txtr` |

### Asset header, version 12 (`UIFontAtlasAssetHeader_v12_t`, 0x68)
| Off | Type | Field |
|---|---|---|
| 0x00 | u16 | `fontCount` |
| 0x02 | u16 | `unk_2` (count for `unk_40`) |
| 0x04 | u16 | `width` |
| 0x06 | u16 | `height` |
| 0x08 | f32 | `widthRatio` |
| 0x0C | f32 | `heightRatio` |
| 0x10 | 40 bytes | `unk_10` (unknown) |
| 0x38 | ptr | `fonts` |
| 0x40 | ptr | `unk_40` (was `unk_18`) |
| 0x48 | u64 | `atlasGUID` |
| 0x50 | ptr | `unk_50` |
| 0x58 | ptr | `unk_58` |
| 0x60 | i32[2] | `unk_60` |

### Font header v6 (`UIFontHeader_v6_t`, 0x40)
| Off | Type | Field | Meaning |
|---|---|---|---|
| 0x00 | ptr | `name` | NUL string (maybe NULL) |
| 0x08 | u16 | `fontIndex` | index in the game's global font list (`GetFontIndex(StringToGuid(name))`; the macro in the source is broken and unused) |
| 0x0A | u16 | `numUnicodeMaps` | |
| 0x0C | u32 | `numTextures` | |
| 0x10 | f32 | `proportionScaleX` | |
| 0x14 | f32 | `proportionScaleY` | |
| 0x18 | f32[2] | `unk_18` | |
| 0x20 | f32 | `proportionSizeScale` | becomes `proportions[0].scaleSize` (scaleBounds = 1.0) |
| 0x24 | u16 | `numGlyphChunks` | |
| 0x26 | u16 | `textureIndex` | base image index in the atlas-wide image list |
| 0x28 | ptr | `unicodeMap` | `UIFontUnicodeMap_t[numUnicodeMaps]` |
| 0x30 | ptr | `textures` | `UIFontTexture_v6_t[numTextures]` |
| 0x38 | ptr | `unk_38` | `UIFont_UNK_t[numGlyphChunks]`, kerning? |

`UIFontUnicodeMap_t` (12 bytes): `i32 unicodeFirst`, `i32 unicodeLast`, `i32 textureIndex` (index into `textures`).
`UIFontTexture_v6_t` (0x20): `f32 unk_0` (kerning?), `u16 unk_4`, `u8 unk_6`, `u8 unk_7`, `f32 posBaseX @8`, `f32 posBaseY @0xC`, `f32 posMinX @0x10`, `f32 posMinY @0x14`, `f32 posMaxX @0x18`, `f32 posMaxY @0x1C`. Proportion index is always 0.
`UIFont_UNK_t` (8): `i32 unk_0`, `f32 unk_4`.

### Font header v7 / v10 (`UIFontHeader_v7_t`, 0x60)
| Off | Type | Field | Meaning |
|---|---|---|---|
| 0x00 | ptr | `name` | |
| 0x08 | u16 | `fontIndex` | |
| 0x0A | u16 | `numProportions` | max 8 (`FONT_MAX_PROPORTIONS`) |
| 0x0C | u16 | `numGlyphChunks` | unused |
| 0x0E | u16 | `numUnicodeChunks` | 64 code points per chunk |
| 0x10 | i32 | `glyphIndex` | unused |
| 0x14 | i32 | `unicodeIndex` | base code point, subtracted from input |
| 0x18 | u32 | `numTextures` | |
| 0x1C | f32 | `proportionScaleX` | |
| 0x20 | f32 | `proportionScaleY` | |
| 0x24 | f32[2] | `unk_24` | |
| 0x2C | u32 | `textureIndex` | |
| 0x30 | ptr | `unicodeChunks` | `u16[numUnicodeChunks]`, per-chunk index into the two tables below |
| 0x38 | ptr | `unicodeChunksIndex` | `u16[]`, base texture index per table entry |
| 0x40 | ptr | `unicodeChunksMask` | `u64[]`, bit per code point of the chunk |
| 0x48 | ptr | `proportions` | `UIFontProportion_v7_t[numProportions]` |
| 0x50 | ptr | `textures` | `UIFontTexture_v7_t[numTextures]` |
| 0x58 | ptr | `unk_58` | kerning? |

`UIFontProportion_v7_t` (8): `f32 scaleBounds`, `f32 scaleSize`. `UIFontTexture_v7_t` (0x20): as v6 but byte 6 is `u8 unk_6` and byte 7 is `u8 proportionIndex`.

### Font header v12 (`UIFontHeader_v12_t`, 0x90)
| Off | Type | Field | Meaning |
|---|---|---|---|
| 0x00 | ptr | `name` | |
| 0x08 | u16 | `fontIndex` | must be < 32 (`MAX_FONTS`) |
| 0x0A | u16 | `numProportions` | |
| 0x0C | u16 | `numGlyphChunks` | |
| 0x0E | u16 | `numUnicodeChunks` | |
| 0x10 | i32 | `glyphIndex` | base glyph index |
| 0x14 | i32 | `unicodeIndex` | base code point |
| 0x18 | u32 | `numGlyphTextures` | |
| 0x1C | u32 | `numUnicodeTextures` | |
| 0x20 | f32 | `proportionScaleX` | |
| 0x24 | f32 | `proportionScaleY` | |
| 0x28 | f32[2] | `unk_28` | |
| 0x30 | u32 | `textureIndex` | |
| 0x34 | i32 | `errorGlyph` | |
| 0x38 | ptr | `glyphChunks` | `u16[numGlyphChunks]` |
| 0x40 | ptr | `unicodeChunks` | `u16[numUnicodeChunks]` |
| 0x48 | ptr | `glyphChunksIndex` | `u16[]` |
| 0x50 | ptr | `unicodeChunksIndex` | `u16[]` |
| 0x58 | ptr | `glyphChunksMask` | `u64[]` |
| 0x60 | ptr | `unicodeChunksMask` | `u64[]` |
| 0x68 | ptr | `proportions` | `UIFontProportion_v12_t[numProportions]` |
| 0x70 | ptr | `glyphTextures` | `UIFontTexture_v12_t[numGlyphTextures]`; NULL on Switch |
| 0x78 | ptr | `unicodeTextures` | `UIFontTexture_v12_t[numUnicodeTextures]` |
| 0x80 | ptr | `unk_80` | |
| 0x88 | ptr | `unk_88` | `UIFont_UNK_t[]` (not counted by `numGlyphChunks`?) |

`UIFontProportion_v12_t` (0x18): `f32 scaleBounds`, `f32 scaleSize`, `i32 unk_8`, `f32 unk_C`, `i32 unk_10`, `f32 unk_14`.
`UIFontTexture_v12_t`: `f32 unk_0 @0`, `u16 unk_4 @4`, `u16 unk_6 @6`, `i8 unk_8 @8`, `i8 proportionIndex @9` (max 29, "only 30 vectors"), `u8 unk_A[2] @0xA`, `f32 posBaseX @0xC`, `posBaseY @0x10`, `posMinX @0x14`, `posMinY @0x18`, `posMaxX @0x1C`, `posMaxY @0x20`. The field list sums to 0x24 bytes; the rsx comment says "per texture, 32 bytes". rsx indexes with `sizeof` of the C++ struct (0x24), so the stride is 0x24 if the program works for v12; confirm on real data.

Constants: `FONT_TEXTURE_IDX_INVALID = 0xFFFFFFFF`, `FONT_UTF16_QMARK = '?'`, `FONT_UTF16_BOX = 0x25A1`, `FONT_MAX_PROPORTIONS = 8`.

## Data outside the header
Everything (font headers, tables, strings) is in rpak pages. The pixels are in the separate `txtr` asset named by `atlasGUID` (rsx picks mip 0, `GetTextureDataForMip`, so any swizzle is undone). No stream or compression of its own.

## Dependencies
`atlasGUID` -> `txtr` (required for any image output; rsx asserts it exists and takes the texture's name for the font atlas). Resolve with `rpak.guid` kind `txtr` (project.md 5.5), through `locate`. The `txtr` format must be BC/RGBA known to `Bitmap` (rsx maps via `s_PakToDxgiFormat`, fails on unknown format).

## Decoding
1. Read the asset header by version, then `fontCount` font headers from `fonts` (stride 0x40 for v6, 0x60 for v7/v10, 0x90 for v12).
2. Generic font: for v6 `proportions[0] = (1.0, proportionSizeScale)`, `numProportions = 1`, `glyphIndex = unicodeIndex = 0`, `numUnicodeTextures = numTextures`, `numGlyphTextures = 0`. Copy `numProportions` (<= 8) proportions and the texture arrays.
3. Image list size = `lastFont.textureIndex + (version >= 12 ? lastFont.numGlyphTextures : lastFont.numUnicodeTextures)`. A font's images occupy `[textureIndex, textureIndex + count)` (assumes the last font has the highest base).
4. For each font: `imageBoundsScale[i] = (scaleBounds_i * proportionScaleX, scaleBounds_i * proportionScaleY)`; `imageSizeBase[i] = (scaleSize_i * imageBoundsScale[i].x, scaleSize_i * imageBoundsScale[i].y)`.
5. Per texture `t` (v12 with glyph textures and `fontIndex != 10`: the `glyphTextures`; else the unicode/legacy `textures`) with `p = proportionIndex`:
   - if `posMinX == posMaxX`: empty glyph. `mins = (1,1)`, `maxs = (0,0)`, `posX = atlas width`, `posY = atlas height`, `width = height = 0`.
   - else `mins = scaleBounds*pos_min + posBase - sizeBase`, `maxs = scaleBounds*pos_max + posBase + sizeBase` (all x and y with the matching entries above), `posX = u16(mins.x * atlasWidth + 0.5)`, `posY = u16(mins.y * atlasHeight + 0.5)`, `width = u16((maxs.x - mins.x) * atlasWidth + 0.5)`, `height` likewise. Coordinates are normalized atlas UV; "output closer but maybe still a bit off, handled in the shader" says rsx, i.e. the rounding is approximate.
6. Character lookup (to assign a code point to each image):
   - v6 (`TextureFromUnicodeMap`): linear scan of the unicode maps for the first with `unicodeLast > c`; valid if `unicodeFirst <= c`; texture index = `map.textureIndex + c - map.unicodeFirst`. Else fall back to `'?'`; if that fails, invalid. Max code point scanned: `((lastMap.unicodeLast >> 6) + 1) << 6`.
   - v7 / v10 / v12 unicode (`TextureFromUnicode`): `f = c - unicodeIndex`; `chunk = (u32)f >> 6`; if `chunk < numUnicodeChunks`: `bit = f & 63`, `ti = unicodeChunks[chunk]`, `mask = unicodeChunksMask[ti]`; if `mask & (1<<bit)` then index = `unicodeChunksIndex[ti] + popcount(mask & ((1<<bit)-1))`. If not found try `U+25A1`, then invalid. Max code point scanned `(numUnicodeChunks + 1) << 6`.
   - v12 glyph (`TextureFromGlyph`): same with `glyphIndex`, `glyphChunks`, `glyphChunksMask`, `glyphChunksIndex`, `numGlyphChunks`; the fallback uses `errorGlyph` (see quirks). Max scanned `(numGlyphChunks + 1) << 6`.
   - `SetCharacterForImages` loops `c = 0..max`, takes the first code point for each image index (later duplicates are ignored, "unsupported unicode often map to one image"; in v6 everything unmapped becomes the `'?'` image), stops when all images are assigned.

## Output
rsx export (`ExportUIFontAtlasAsset`, settings `PNG (Atlas)`, `PNG (Textures)`, `DDS (Atlas)`, `DDS (Textures)`):
- Atlas: `<export>/<ui_font_atlas>/<stem>.png|dds` (full atlas texture).
- Textures: `<...>/<stem>/<fontname|unnamed_i>/U+<xxxx lowercase hex>.png|dds`, one image per character with non-zero size, cut from the converted RGBA8 atlas at `(posX, posY, width, height)`; only `numUnicodeTextures` per font is exported.
Proposed respak: `Bitmap` for the atlas (via the linked `txtr`) and a bundle `font.json` + `atlas.<ext>` (+ optional `glyphs/<font>/U+XXXX.png`), JSON:
```json
{ "version": 12, "atlas": "0x..", "width": 2048, "height": 2048,
  "fonts": [ { "name": "...", "fontIndex": 3, "unicodeIndex": 32, "glyphIndex": 0, "errorGlyph": 0,
               "proportions": [ { "scaleBounds": 1.0, "scaleSize": 0.0 } ],
               "characters": [ { "index": 0, "codepoint": 65, "glyph": null, "proportion": 0, "x": 0, "y": 0, "w": 0, "h": 0 } ] } ] }
```
Proposed capabilities: `Metadata`, `Bitmap` (atlas), dependency on `txtr`.

## rsx quirks
- `TextureFromGlyph` fallback: `glyph = LOBYTE(errorGlyph) << 16` looks wrong, and the loop ends when `glyph == 0`; do not copy, define the fallback yourself.
- Glyph-index lookups (`font->glyphTextures`) are used in v12 except `fontIndex == 10`; export and preview only iterate `numUnicodeTextures`, so v12 glyph-only fonts export nothing or collide on `U+ffffffff` (`utf16 == -1` unassigned). Fix in respak (export glyphs by index).
- `CreateTextureForImage` compares `height == height` (typo for atlas height), harmless.
- `FONT_TEXTURE_IDX_INVALID` is checked as `idx >= numTextures` in `SetCharacterForImages`.
- v12 `UIFontTexture` comment says 32 bytes but the fields sum to 36; Switch data has NULL `glyphTextures`.
- `GetFontIndex` macro (`... & 1f`) is invalid and unused.
- rsx does no bounds checks; respak must map to `Corrupt`.
- Mask/index table sizes are not stored; they are the count of distinct entries referenced by the chunk tables (maximum chunk table value + 1). Compute that rather than trust a count.

## Reuse in respak
`rpak.asset` (page pointer reading, string reading), `rpak.guid` + `locate` for the atlas `txtr`, `txtr` `Bitmap`/`read_mip`, PNG writing (`matl::TextureTarget`, `respak_formats`), JSON bundle pattern from `uiia`. New: version-dependent header parsers, the chunk/mask lookup, bounds computation.

## Tests
Synthetic: v7 font with `unicodeIndex = 32`, one chunk with mask `0b101` and `unicodeChunksIndex = 4`: code points 32 and 34 map to images 4 and 5; absent code point falls back to `U+25A1`; v6 map with two ranges and fallback to `'?'`; empty glyph (`posMinX == posMaxX`) gives size 0; `proportionIndex >= numProportions`, `fontCount = 0`, `textureIndex` overlap, NULL names. Real: UI paks with `ui_font_atlas` entries (names unknown here; not verified). No Steam runs without permission.

## Open questions
1. v12 `UIFontTexture` stride (0x24 vs 0x20) and glyph-chunk table sizes need confirmation on real data.
2. Meaning of `unk_*` fields and whether `unk_18` / `unk_40` / `unk_88` (kerning?) are needed for anything.
3. Export granularity: atlas only, or also per-glyph images; glyph vs unicode keying in v12.
4. Is a TTF-like or BMFont (`.fnt`) output wanted?
