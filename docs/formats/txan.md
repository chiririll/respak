# txan · Texture animation
> **Verified against** Apex `paks/Win64/common.rpak` (111 assets) and `mp_lobby.rpak` (4): version 1, header size 24, 12-byte layer stride. **Corrects rsx:** the layer's two u16 are `{first slot, count}` and the slot array length is `sum(count)`.
**rsx sources:** `game/rtech/assets/texture_anim.{h,cpp}` (`TextureAnimAssetHeader_v1_t`, `TextureAnimLayer_t`, `LoadTextureAnimationAsset`, `GetSlotCount`, `ExportTextureAnimationAsset`), `docs/assets/rpak/TextureAnimation.md` · **Games / versions:** Apex Legends, asset version 1 only (still 1 in retail per the rsx comment, build `R5pc_r5-230_J34_CL8040141`) · **Priority hint:** low, S (tiny header, no decoding; the meaning of most fields is unknown)

## Summary
A small table that tells the engine how to cycle a material's texture slots over time: a list of layers, each covering a run of entries (first index + count) of a byte array of texture-slot indices. rsx does not interpret it; it only dumps the raw tables to a custom `.txan` file (rsx's own format, which is also read by repak). A user would convert it to JSON (layers + slots) or keep the raw rsx `.txan` for repak.

## Where it lives
rpak asset, FourCC `txan` (rsx binding `'naxt'`, i.e. the little-endian value of the ASCII `txan`), rsx output directory `texture_anim` (`s_AssetTypePaths`, `game/asset.h:184`). Header alignment 8. The asset has no name field: rsx calls `SetAssetNameFromCache()` (name comes from rsx's external guid-to-name cache, otherwise unnamed). Name in the volume would be `<guid>.txan` unless a name is supplied by the user. The real name hashing is unknown (not verified in the source).

## Versions and detection
Only asset version 1; `LoadTextureAnimationAsset` does not even switch on it (any version is treated as v1). Proposed: score = 10 for kind `txan`, `Version { major: asset.version, minor: 0 }`; version 1 accepted, anything else `UnsupportedVersion { txan, <v> }`. Also require the header struct size of 0x18 if the rpak exposes it (not checked by rsx).

## Layout
Little-endian. Asset header `TextureAnimAssetHeader_v1_t`, 0x18 bytes:

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | page ptr | `layers` | array of `TextureAnimLayer_t`, `layerCount` entries. The rsx comment says it "starts with TextureAnimDataHeader_t"; that type does not exist in the source and the export treats it as a plain layer array (stale comment) |
| 0x08 | 8 | page ptr | `slots` | `u8` array of slot indices; size not stored |
| 0x10 | 4 | i32 | `layerCount` | confirmed 32-bit by a disassembly note |
| 0x14 | 4 | i32 | `padding` | always 0 |

`TextureAnimLayer_t`, 12 bytes (array stride 12):

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 2 | u16 | `startSlot` | **index of the layer's first entry in the `slots` array** (verified; layers are contiguous) |
| 0x02 | 2 | u16 | `endSlot` | **number of entries** (frames) of the layer, not an end index (verified) |
| 0x04 | 4 | f32 | `unk2` | "some scale used to switch between textures, cross-fading perhaps?" Often equals the frame count, also 1.0, 30.0, 128.0, 0.95: looks like a playback rate |
| 0x08 | 2 | u16 | `flags` | unknown flags |
| 0x0A | 2 | u16 | `unk5` | probably padding, unused by the engine functions rsx reverse engineered |

Slot count: not in the asset. rsx's `GetSlotCount` = maximum over layers of `max(startSlot, endSlot)`, which is **only correct for single-layer assets**. The real length of the `slots` array is `sum(endSlot)` over the layers = `last.startSlot + last.endSlot` (verified on 111 assets; see *Verified on real data*).

rsx's export file (`TextureAnimFileHeader_t`, 16 bytes, "not an official format"):

| Off | Type | Field |
|---|---|---|
| 0x00 | i32 | `magic` = `('N'<<24 \| 'A'<<16 \| 'X'<<8 \| 'T')`, stored bytes `54 58 41 4E` = ASCII `TXAN` |
| 0x04 | u16 | `fileVersion` = 1 (`TXAN_FILE_VERSION`) |
| 0x06 | u16 | `assetVersion` (`pakAsset->data()->version`) |
| 0x08 | u32 | `layerCount` |
| 0x0C | u32 | `slotCount` (from `GetSlotCount`) |

Followed by `layerCount * 12` bytes of layers and `slotCount` bytes of slots.

## Verified on real data
`mp_lobby.rpak` (4 assets) and `common.rpak` (all 111 assets; one more in `startup.rpak` was not read): version 1, header size 0x18, padding 0, in every one.
- **The two u16 fields of a layer are `{first slot index, number of slots}`, not `{start, end}`.** In all 111 `common.rpak` assets the layers are contiguous: the first layer starts at 0 and `layer[i+1].first == layer[i].first + layer[i].count`. Examples: `(0,4),(4,1),(5,1),(6,1)`: one 4-frame animation and three single-slot layers; `(0,16),(16,16),(32,16),...` for a 15-layer 16-frame flipbook; `(6,2),(8,2),(10,2),(12,2)`. 31 of 111 assets have a second field smaller than the first, which only makes sense as a count. This is why rsx's comment says the start "may be greater than the end".
- The layer array is immediately followed (`slots == layers + 12 * layerCount` in 111/111) by the slot array, which holds the texture-slot indices for the frames of every layer in order (e.g. `[0,1,2,3, 0, 1, 2]` for the first example: the 4 frames of layer 0 are slots 0..3, then layers 1-3 are single frames showing slots 0, 1, 2).
- **The slot array has `sum(count) = last.first + last.count` bytes.** In 28 of 28 checkable cases (the next object follows directly) the next asset's layers start exactly there. rsx's `GetSlotCount` (`max(startSlot, endSlot)`) is **wrong whenever it differs**: 31 of 111 assets (all with more than one layer) have `sum(count) != GetSlotCount`, so rsx's own `.txan` export drops the last byte(s) of the slots for them. For single-layer assets starting at 0 the two agree (`mp_lobby`: counts 60, 64, 32, 64).
- `layerCount` seen: 1 (79 assets), 2, 3, 4 (14), 5, 7, 9, 10, 15, 16, 17, 24. The f32 (`unk2`) equals the layer's count in 186 of 277 layers and is otherwise values such as 1.0, 30.0, 128.0, 0.95: it looks like a playback rate (frames per second or per loop). `flags` is 2 (140 layers) or 3 (137); the trailing u16 is 0 in all 277. Slot contents are mostly identity runs but also `[0,1,3,4,5,2,...]`, `[0,1,2,3,2,1]`, `[0,1,2,3,1,2,3]`.

## Data outside the header
Everything is in pages: the layer array and the slot array are page-pointer targets in the rpak (`.mem`). No starpak data, no compression, no embedded blobs.

## Dependencies
None known. Materials presumably reference the txan by guid (unverified in rsx); rsx never follows a txan to a texture, and the slot indices refer to texture slots of the material that uses it.

## Decoding
1. Read the 0x18-byte header; follow `layers` and `slots` page pointers.
2. Reject `layerCount < 0`; bound check `layerCount * 12` against the page.
3. Read `layerCount` layers.
4. The slot array length is not stored: use `sum(layer.endSlot)` (= `last.startSlot + last.endSlot`) bytes, **not** rsx's `maxSlot` (verified: that loses bytes on multi-layer assets). Never read beyond the page.

## Output
rsx: one `<export dir>/texture_anim/<name>.txan` in the format above (no options, `settings` empty). Proposed respak: `Metadata` (kind `txan`, guid) and a JSON edge `txan -> txan.json`:
```json
{ "version": 1, "layers": [ { "firstSlot": 0, "count": 4, "unk2": 4.0, "flags": 3, "unk5": 0 } ], "slots": [0, 1, 2, 3] }
```
Optionally also write rsx's binary `.txan` for repak compatibility.

## rsx quirks
- No name field; named only through the external cache.
- `GetSlotCount` is wrong for multi-layer assets (verified): it takes `max(startSlot, endSlot)` while `endSlot` is really a count, so the exported `slotCount` is too small whenever the last layer does not start at 0, and the `.txan` export is truncated. Do not copy it.
- Load function ignores the asset version.
- `unk2`, `flags`, `unk5` are unreverse-engineered; keep them as raw values.
- A repak-side reader exists for the export format; the file magic is written as a 32-bit int, so its in-file bytes read `TXAN`.

## Reuse in respak
`rpak.asset` (`detect_kind`, `rpak_asset`, page-pointer reading); JSON writer used by `dtbl`/`stgs`. New: header + layer parser (about 40 lines).

## Tests
Synthetic: build a 0x18 header with 2 layers (`first 0, count 4`, `first 4, count 1`) and slots `[0,1,2,3,0]`; check the slot array length is 5 (rsx's `maxSlot` would say 4). Edge cases: `layerCount` 0 (empty arrays, null `slots`), `layerCount` negative, pointer past the page, version != 1. Real files: `paks/Win64/common.rpak` (111 `txan`, 79 single-layer, 32 multi-layer), `paks/Win64/mp_lobby.rpak` (4); also `startup.rpak` (1) and `common_mp.rpak` (5), not read.

## Open questions
1. What `unk2` exactly is (playback rate?) and what `flags` 2 vs 3 mean.
2. Is the txan worth a slice at all (nothing downstream consumes the data)? Suggest raw JSON dump only.
3. Where the guid-to-name comes from (cache only?).
4. How a material refers to a txan and how the slot indices map to the material's texture slots (no material was followed).
