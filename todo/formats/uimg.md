# uimg · UI image atlas
**rsx sources:** `game/rtech/assets/ui_image_atlas.h` (structs), `game/rtech/assets/ui_image_atlas.cpp` (`LoadUIImageAtlasAsset` :11, `PostLoadUIImageAtlasAsset` :116, `PreviewUIImageAtlasAsset` :232, `ExportUIImageAtlasAsset` :475, `InitUIImageAtlasAssetType` :677); `game/asset.h` (`s_AssetTypePaths`, `s_postLoadOrderOverrides`) · **Games / versions:** asset versions 10 and 11 (Apex Legends; which seasons is unknown) · **Priority hint:** useful (all HUD icons live in atlases), small to medium (M): the header is simple, the work is the sprite slicing and the guid link to `txtr`.

> **Notes for the index.**
> - `mt4a` ("material for aspect") and `txtx`: **not supported by rsx.** They show up only as comments in `game/asset.h` (:110, :114, :182) and `core/main.cpp` (:57, :130); no struct, loader or exporter exists.
> - `font`, `txan`, `txls`, `msnp`: research for those specs was stopped by the coordinator before it was finished, so no spec files were written for them. The `font` header structs were read (`ui_font_atlas.h`, v6/v7/v10/v12; it links to its atlas texture by guid the same way, `atlasGUID`), but the loader in `ui_font_atlas.cpp` was not.
> - This spec was written without a final check pass over rsx. Anything that wasn't read directly is marked unknown.

## Summary
A UI image atlas is one atlas texture (a regular `txtr` asset, linked by guid) plus a table of named sprites. Each sprite has a UV rectangle in the atlas, a render size and layout/anchor floats. The game uses them for HUD icons, badges, and similar. A user wants: the atlas as an image, every sprite cut out as its own image named by its path, and a JSON description of the sprites.

## Where it lives
- Container: rpak. Kind FourCC as stored: `uimg`. rsx writes it reversed, as the multi-char literal `'gmiu'` (`ui_image_atlas.cpp:683`). Header alignment 8 (`:684`).
- Extension in the rpak volume: `.uimg` (`<path>.uimg` or `0x<GUID>.uimg`).
- Name: the asset has **no name pointer**. rsx builds the name from the linked atlas texture's name: `"ui_image_atlas/" + <txtr name> + ".rpak"` (`ui_image_atlas.cpp:130-136`). The commented-out assert at `:134` suggests this string hashes to the asset guid with `StringToGuid`, but it is disabled, so that is unverified. If the texture has no name, rsx falls back to its GUID → name cache (`SetAssetNameFromCache`, `:140`).
- rsx's export folder prefix: `ui_image_atlas` (`s_AssetTypePaths`, `game/asset.h`).

## Versions and detection
| asset version | header | sprite hash entry | rsx |
|---|---|---|---|
| 10 | `UIImageAtlasAssetHeader_v10_t` (0x40) | `UIImageAtlasHash_v10_t`, 8 bytes | `ui_image_atlas.cpp:20, :69` |
| 11 | same header struct | `UIImageAtlasHash_v11_t`, 12 bytes | `ui_image_atlas.cpp:21, :78` |

- rsx tells them apart only by the pak asset entry's version (`pakAsset->version()`, `:15`). Any other version hits `assertm(false)` and then dereferences a null `uiAsset` (`:28-31`), which crashes.
- `uimg` is **not** in rsx's validation bank (`core/filehandling/validation.cpp`), so rsx has no list of known header sizes. Whether every v10/v11 asset has `headerSize == 0x40` is unknown.
- Proposed score: `rpak.asset` with kind `uimg` → 10, `Version::new(asset.entry.version)` (minor 0). Versions other than 10 and 11 → `UnsupportedVersion`. A header shorter than 0x40 → `Corrupt`.

## Layout
Little-endian. "page ptr" means an 8-byte pointer: inside the asset header the rpak volume has already relocated it to a `.mem` offset (null = `u64::MAX` or 0, same as in `txtr.rs`). The arrays it points to live in page data.

### Header `UIImageAtlasAssetHeader_v10_t` (v10 and v11), 0x40 bytes (`ui_image_atlas.h`)
| off | size | type | field | meaning |
|---|---|---|---|---|
| 0x00 | 4 | f32 | widthRatio | probably 1/width (not verified; rsx never reads it) |
| 0x04 | 4 | f32 | heightRatio | probably 1/height (not verified) |
| 0x08 | 2 | u16 | width | atlas width in pixels. rsx scales the bounds by this |
| 0x0A | 2 | u16 | height | atlas height in pixels |
| 0x0C | 2 | u16 | textureCount | number of sprites |
| 0x0E | 2 | u16 | unkCount | number of entries at `unk` |
| 0x10 | 8 | page ptr | textureOffsets | `UIImageAtlasOffset_t[textureCount]` |
| 0x18 | 8 | page ptr | textureDimensions | `UIImageAtlasDimension_t[textureCount]` |
| 0x20 | 8 | page ptr | unk | `unkCount` × 32 bytes ("eight floats", per the comment); meaning unknown, rsx never reads it |
| 0x28 | 8 | page ptr | textureHashes | `UIImageAtlasHash_v10_t[]` or `_v11_t[]`, `textureCount` entries |
| 0x30 | 8 | page ptr | textureNames | char blob of NUL-terminated sprite paths; may be null (names stripped) |
| 0x38 | 8 | u64 | atlasGUID | guid of the atlas `txtr` |

### `UIImageAtlasOffset_t`, 32 bytes, one per sprite (layout floats, used in game)
| off | type | field | rsx comment |
|---|---|---|---|
| 0x00 | f32 | cropInsetLeft | negative values clip the image inwards |
| 0x04 | f32 | cropInsetTop | |
| 0x08 | f32 | endAnchorX | where the image edge is, 1.0 = full length |
| 0x0C | f32 | endAnchorY | |
| 0x10 | f32 | startAnchorX | top-left corner as a fraction of the full image |
| 0x14 | f32 | startAnchorY | |
| 0x18 | f32 | scaleRatioX | distorts the axis |
| 0x1C | f32 | scaleRatioY | |
The field meanings are rsx's guesses (header comment). Pass them through without interpreting them.

### `UIImageAtlasDimension_t`, 4 bytes
| off | type | field | meaning |
|---|---|---|---|
| 0x00 | u16 | width | size the game renders the sprite at (not its size in the atlas) |
| 0x02 | u16 | height | |

### `UIImageAtlasHash_v10_t`, 8 bytes (v10)
| off | type | field |
|---|---|---|
| 0x00 | u32 | pathHash |
| 0x04 | u16 | unk_4 (unknown) |
| 0x06 | u16 | pathTableOffset: byte offset into `textureNames` |

### `UIImageAtlasHash_v11_t`, 12 bytes (v11)
| off | type | field |
|---|---|---|
| 0x00 | u32 | pathHash |
| 0x04 | i32 | pathTableOffset: byte offset into `textureNames` |
| 0x08 | i32 | unk_8 (unknown) |

### `UIImageAtlasBounds_t`, 16 bytes, one per sprite, in the asset's **CPU data** (`pakAsset->cpu()`, `:35-36`)
| off | type | field | meaning |
|---|---|---|---|
| 0x00 | f32 | minX | left edge, as a fraction of the atlas width |
| 0x04 | f32 | minY | top edge, as a fraction of the atlas height |
| 0x08 | f32 | sizeX | width, as a fraction |
| 0x0C | f32 | sizeY | height, as a fraction |

How `pathHash` is computed (which hash function, over which string) is unknown; rsx only displays it.

## Data outside the header
- Page data: the five arrays above, reached through the relocated header pointers (`.mem` ranged reads). Pointers inside page data are not involved.
- CPU data (`rpak.asset.data_mem_offset`): `textureCount` × 16-byte bounds. rsx asserts it exists (`:35`).
- No streamed data and no compression of its own. The atlas pixels belong to the linked `txtr` (with its own starpak and compressed mips).

## Dependencies
- `atlasGUID` → `txtr` (required for any image output). rsx finds it with `g_assetData.FindAssetByGUID` over every loaded pak (`:125`) and asserts it is found. It needs the texture post-loaded first, which `s_postLoadOrderOverrides` guarantees (`'rtxt'` before `'gmiu'`).
- In respak: `GuidRef::new(atlas_guid, "txtr", origin)` → `identify` → `txtr` with `Bitmap` (the pattern in `matl_json.rs::locate`). `Metadata::dependencies()` = `[{ guid: atlasGUID, name: None }]`.
- The asset name depends on the texture name (see Where it lives). The read edge must not convert the texture just to get a name. Options: leave the name `None` and let the volume/cache name it, or resolve the `txtr` name lazily in the JSON encoder. Open question.

## Decoding
```
h = read_header()                      // 0x40, pointers relocated
assert version in {10, 11}
n = h.textureCount
offsets = mem[h.textureOffsets .. + 32*n]
dims    = mem[h.textureDimensions .. + 4*n]
hstride = (version == 10) ? 8 : 12
hashes  = mem[h.textureHashes .. + hstride*n]
bounds  = mem[cpu .. + 16*n]
for i in 0..n:
    b = bounds[i]
    posX   = u16(trunc(b.minX * width))             // rsx :56, truncation, no rounding
    posY   = u16(trunc(b.minY * height))            // :57
    w      = u16(trunc(b.sizeX * width  + 0.5))     // :61, rounded
    h_     = u16(trunc(b.sizeY * height + 0.5))     // :62
    (hash, off) = version 10 ? (u32@0, u16@6) : (u32@0, i32@4)
    path = textureNames != null ? cstr(mem[textureNames + off]) : format("0x{hash:X}")
```
Notes:
- The names blob has no stored length. Read each name with a bounded ranged read (like `txtr::read_name`, 512 bytes) and check `off` against `.mem` length.
- A sprite with `w == 0 || h == 0` is invalid. rsx skips it when slicing (`:538, :580`).
- Slicing: decode atlas mip 0 to RGBA8 (sRGB kept if the source format is sRGB, `:162`). Then for each sprite, copy the rectangle `(posX, posY, w, h)`, clamped to the atlas. rsx copies with `CopySourceTextureSlice`; its clamping behaviour is unknown, so clamp in respak and warn.
- The atlas `width`/`height` in the header may differ from the texture's mip-0 size. rsx uses the header values for the rectangles and the texture's own size for the image. Whether they can differ is unknown (open question). If they do, scale by the header and warn.

## Output
**rsx** (`ExportUIImageAtlasAsset`, settings at `:679`):
| setting | output |
|---|---|
| PNG (Atlas) | `<dir>/<stem>.png`: mip 0 of the atlas texture |
| DDS (Atlas) | `<dir>/<stem>.dds`: mip 0, original (BC) format |
| PNG (Textures) | one PNG per sprite, RGBA8: `<dir>/<stem>/<sprite path parent>/<sprite filename>.png` (path logic `:545-562`, see quirks) |
| DDS (Textures) | the same, as uncompressed RGBA8 DDS |
| Metadata (JSON) | `<dir>/<stem>.json`: `{ atlas: <stem>, unkCount, images: [{ path, width, height, posX, posY, cropInsetLeft, cropInsetTop, endAnchorX, endAnchorY, startAnchorX, startAnchorY, scaleRatioX, scaleRatioY }] }`. Floats come from `std::to_string` (6 decimals). Sprites are in width-sorted order (see quirks) |
`<dir>` = export dir + the asset path's parent if `g_rsxSettings.exportPathsFull`, else `ui_image_atlas/`.

**Proposed respak:**
- `uimg`: read edge from `rpak.asset`, extension `uimg`, provides `Metadata` (name, guid, dependency on the atlas). The native type `UimgAsset { header, sprites: Vec<Sprite> }` keeps the sprites in **stored order** (index = stored index).
- Should `uimg` also provide `Bitmap` (the atlas)? It would have to resolve the guid inside `read`, and a read edge shouldn't convert dependencies. So no: images go through the JSON target, which links (open question).
- `uimg -> uimg.json` (a convert edge, a bundle in the atlas's own folder, like `uiia.json`):
  - `atlas.<ext>`: the linked `txtr` converted through the mediator with the `image` option (`TextureTarget`, default `png`).
  - with `"sprites": true` (default false? open question): `sprites/<sprite path>.<ext>`, each a `BitmapBuf` RGBA8 crop converted with the same `image` target. The path is sanitised: strip `..` and leading `/`, and keep the sprite path's subfolders.
  - `uimg.json`:
```json
{
  "name": "ui_image_atlas/…", "guid": "0x…", "version": 11,
  "width": 2048, "height": 2048, "widthRatio": 0.00048828125, "heightRatio": 0.00048828125,
  "atlas": { "guid": "0x…", "file": "atlas.png" },
  "unkCount": 0,
  "sprites": [
    { "index": 0, "path": "rui/…", "pathHash": "0x…",
      "x": 0, "y": 0, "width": 64, "height": 64,
      "uv": { "minX": 0.0, "minY": 0.0, "sizeX": 0.03125, "sizeY": 0.03125 },
      "renderSize": { "width": 64, "height": 64 },
      "layout": { "cropInsetLeft": 0, "cropInsetTop": 0, "endAnchorX": 1, "endAnchorY": 1,
                  "startAnchorX": 0, "startAnchorY": 0, "scaleRatioX": 1, "scaleRatioY": 1 },
      "file": "sprites/rui/….png" }
  ]
}
```
  - `atlas.file` / `sprite.file` are `null` when not written (the texture is missing → warning, JSON still written, the way `matl.json` handles a missing texture). Exact float values (serde), no `to_string` rounding.
- No new capability is needed (Bitmap + Metadata + BitmapBuf exist).

## rsx quirks
- **Sprite order lost:** `imageArray` is `std::sort`ed by width (`:100`). The sort isn't stable, and the JSON and file export follow that order. **Drop:** keep the stored order and emit `index`.
- **Root pseudo-image:** rsx appends a whole-atlas entry to `imageArray` (`:103-111`). Exporters skip it with `rbegin()+1` and with the `!offsets` check plus a hack that pops the last comma off the JSON string (`:624-633`). **Drop:** the atlas is a separate output.
- Hand-built JSON with no escaping of `path` (`:638`). **Drop:** use serde.
- `posX/posY` are truncated while width/height are rounded with `+0.5` (`:56-62`; rsx's own TODO asks whether this is right). **Keep** it for parity with rsx output, and note it in the review.
- `CreateTextureForImage` compares `uiImage->height == uiImage->height` (always true, `:267`), so any sprite as wide as the atlas previews as the whole atlas. Preview only; drop.
- Unknown version: assert, then a null dereference (`:28-33`). → `UnsupportedVersion`.
- Missing atlas texture: assert, then a crash (`:125-128`). → `MissingDependency` / warning.
- `textureNames + pathTableOffset` is computed even when `textureNames` is null; the null check comes after (`:74, :89`). Fine in respak, which checks first.
- Sprite export path: `currentPath = exportPath/<stem>` + the sprite path's parent, then `.parent_path()` is taken to strip a trailing slash. When the sprite path has no parent, that also drops `<stem>`, so sprite files land next to the atlas. Its "files on disk" layout isn't reliable. **Drop:** use the respak layout above.
- `printf("Format = %d")` debug output in the DDS loop (`:564`). Drop.
- Global settings: `g_rsxSettings.exportPathsFull` (path layout), `disableCachedNames` (name fallback). Not ported.
- Sprite DDS files are RGBA8, not the source BC format.

## Reuse in respak
- `rpak.asset` (header, `data_mem_offset`, `volume`, `MEM`), `txtr::read_name` for bounded C-string reads, the null-pointer convention from `TxtrHeader::parse`.
- `GuidRef` + `identify` (`matl_json.rs::locate`) to get the atlas `txtr`. `TextureTarget` for the image option. Bundle output like `uiia_json.rs`.
- To slice, decode the atlas mip 0 to RGBA8 with `respak_formats::decode_rgba8` (as `uiia.rs` does), crop, and wrap the crop in `BitmapBuf`.
- New: `uimg.rs` (header, sprite table), `uimg_json.rs` (encoder + slicer).

## Tests
- Synthetic v10 and v11 assets: a 0x40 header, page data with 2–3 sprites (offsets, dims, hashes with the right stride, a names blob), CPU bounds. One fixture with `textureNames` null → `0x<hash>` paths. One sprite of size 0 → skipped with a warning. One v12 fixture → `UnsupportedVersion`. A sprite path containing `..` → sanitised.
- Rounding: bounds `0.5/64` style values, checked against the formulas above.
- A synthetic RGBA8 `txtr` (guid matching `atlasGUID`) in the same test pak, with checks on the cropped pixels. A missing atlas → JSON with `file: null` and a warning.
- Real files (no Steam runs without permission): Apex `common.rpak` / `ui*.rpak` atlases; compare with rsx "Metadata (JSON)" and "PNG (Textures)" output, allowing for the reordering.

## Open questions
1. Which Apex seasons use v10 and which use v11? Does R2/Titanfall have `uimg`? (Not visible in rsx.)
2. Is `headerSize` always 0x40 for both versions? rsx has no validation entry for `uimg`.
3. How is `pathHash` computed? Can it be checked against the stored path (and used for name recovery)?
4. Can the header `width/height` differ from the atlas texture's mip-0 size (e.g. when streamed mips are missing)? Which one do the rectangles use then?
5. Naming: is `ui_image_atlas/<txtr name>.rpak` really the asset's name (does it hash to the guid)? Should the read edge resolve it, or leave it to the cache?
6. Meaning of `unk` (unkCount × 32 bytes), `unk_4` (v10) and `unk_8` (v11): dump as hex in JSON, or skip?
7. Default for slicing sprites: off (atlas + JSON) or on? Should sprites keep their render size (`textureDimensions`) via resizing, or always the raw atlas pixels (rsx: raw)?
8. Should `uimg` provide `Bitmap` for the atlas directly (meaning a dependency is resolved at read time), so `uimg -> png` works?
