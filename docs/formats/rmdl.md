# mdl_ · Respawn rpak model (rmdl)
**rsx sources:** `game/rtech/assets/model.h` (asset headers `ModelAssetHeader_v8/v9/v12_1/v13/v16_t`, `ModelAssetCPU_v16_t`, `eMDLVersion`, mapping tables), `game/rtech/assets/model.cpp` (`LoadModelAsset` :21, `PostLoadModelAsset` :208, `ExportModelStreamedData` :463, `ExportRawModelAsset` :555, `ExportModelAsset` :819, `InitModelAssetType` :927, `GetModelVersionFromAsset` :946), `game/rtech/utils/studio/studio_r5.h` (studiohdr v8, bone v8, mesh/model v8, texture v8), `studio_r5_v12.h` (v12.1 through v15 headers, bone/mesh/model/hw-group), `studio_r5_v16.h` (v16 through v20 headers, bone/mesh/model/bodypart, hw-group), `studio.h` (VG `vg::rev1..rev4`, `vvw`, vertex flags, `FIX_OFFSET`, `UNPACKWEIGHT`, header flags), `core/math/compressedvector.h` (`Vector64`, `Normal32`), `core/mdl/modeldata.{h,cpp}` (vertex parse, LOD/mesh assembly, materials, Cast export), `core/mdl/cast.{h,cpp}` (Cast writer), `game/rtech/utils/utils.{h,cpp}` (`eCompressionType`, `DecompressStreamedBuffer`), `game/rtech/cpakfile.h` (starpak offsets) · **Games / versions:** Apex Legends only (rpak asset versions 8 to 20, with sub-variants 12.1-12.5, 13.1, 14.1, 19.1-19.3). Titanfall 1/2 models are loose Source files, not this asset (see `source-mdl`) · **Priority hint:** high (every prop, weapon, character; the base for animation), **L**. First slice should be rmdl -> Cast (mesh + skeleton + material refs), versions 16 to 20 first.

> **Scope of what was read.** Everything below marked "read" comes from the rsx files listed above, read directly in this run. Not read: `ParseModelVertexData_VTX` (the v8 path through loose VTX/VVD), `animdata.*`, `qc.*`, `rmax.*`, `smd.cpp`, the r1/r2 studio structs, `ParseMaterialTextureExportInfo`/`MaterialAsset` (the matl side), animation/sequence structs (only named). Those are marked unknown. No game file was opened, so no layout below is checked against real bytes; the offsets were computed by hand from the struct definitions (and cross-checked against rsx's own `static_assert`s and offset comments where they exist).

## Summary
An `mdl_` asset is one compiled Respawn model: a Source-style `studiohdr` (bones, bodyparts, per-bodypart models, per-model meshes, material references, skins, hitboxes, attachments, sequences, IK, pose parameters), plus the baked vertex data ("VG", a hardware-ready vertex/index buffer set) that lives in a mandatory starpak (or, for small/static models, in the asset's own pages), plus optional physics data and references (by guid) to animation rigs (`arig`) and sequences (`aseq`). A user wants: the mesh per LOD and body part, the skeleton, and the materials (which in turn point to `matl` and `txtr` assets) written as a Cast model that Blender and other tools import. Later: hitboxes, physics collision, sequences/animations.

## Where it lives
- Container: rpak. Kind FourCC as stored (file order): `mdl_`. rsx writes it reversed, as the multi-char literal `'_ldm'` (`model.cpp:932`, also `s_postLoadOrderOverrides` in `asset.h:252`). Header alignment 8 (`:933`). Extension in the rpak volume: `mdl_` is what `kind_string()` would give (`<path>.mdl_`). rsx's export folder prefix is `mdl` (`s_AssetTypePaths`, `asset.h:171`); the extension respak should use is an open question.
- Name: **stored.** The asset header has a `name` page pointer to a NUL-terminated path such as `mdl/.../x.rmdl` (`LoadModelAsset` :203-204, `SetAssetName`). The studiohdr also holds a name (`sznameindex` or `name[]`), which `ModelParsedData_t::name` uses for the Cast model guid. The asset guid is in the pak asset entry (`StringToGuid(name)` presumably; not checked).
- Streamed data: the **mandatory starpak** entry of the asset (VG buffer). rsx reads only the non-optional starpak (`getStarPakStreamEntry(false)`, comment "vertex data is never opt streamed (I hope)" `:28`). Models can also carry the VG inline (`staticStreamingData`), mostly static props.
- Order: `mdl_` is post-loaded after `matl` and `arig`, before `aseq` (`asset.h:239-257`), because it looks up materials by guid and sequences borrow its skeleton. In respak this is just guid resolution at convert time.

## Versions and detection
rsx's `eMDLVersion` ("more accurate than asset version, Respawn made changes without bumping the version", `model.h:164`). The pak asset entry's `version` selects the major; `GetModelVersionFromAsset` (:946) refines it. Mapping to `Version { major, minor }` (the `s_mdlVersionToPak_*` tables, `model.h:216-268`), season labels are rsx comments on the VG revisions (`studio.h:403-657`):

| asset version | detected variants (minor) | asset header struct (size) | studiohdr struct | VG rev | notes |
|---|---|---|---|---|---|
| 8 | 8.0 | `ModelAssetHeader_v8_t` (0x50, computed, no assert) | `studiohdr_v8_t` (0x224 = 548) | none (VTX/VVD loose path) | no VG; geometry comes from `vertexComponentData` as VTX/VVD. **Not read, defer.** |
| 9, 10, 11, 12 | 9.0, 10.0, 11.0, 12.0 | `_v9_t` (120) | `studiohdr_v8_t` | rev1 (seasons 0-6) | rev1 VG |
| 12 | 12.1, 12.2, 12.4, 12.5 (12.3 exists in the enum but the detector never returns it) | `_v12_1_t` (104) | v12.1: 520 bytes, v12.2: 524, v12.4: 532, v12.5: 536 | rev2 (seasons 7-13) | 12.3 only matters for aseq events |
| 13 | 13.0, 13.1 | `_v13_t` (128) | `studiohdr_v12_5_t` | rev2 | 13.1 differs by `mstudiomodel_t` stride |
| 14 | 14.0, 14.1 | `_v13_t` (128) | `studiohdr_v14_t` (544) | rev3 | 14.1 differs in string offsets of `localnodename` |
| 15 | 15.0 | `_v13_t` (128) | `studiohdr_v14_t` | rev3 | new `mstudiobodyparts_v15_t` (24 bytes) |
| 16 | 16.0 | `ModelAssetHeader_v16_t` (96) + `ModelAssetCPU_v16_t` | `studiohdr_v16_t` (224) | rev4 (seasons 15-now) | all u16 fields, `FIX_OFFSET` |
| 17, 18 | 17.0, 18.0 | `_v16_t` | `studiohdr_v17_t` (228) | rev4 | v18 only changes sequences |
| 19 | 19.0, 19.1, 19.2, 19.3 | `_v16_t` | 19.0/19.1: `studiohdr_v17_t` (228). 19.2/19.3: `studiohdr_v19_2_t` (244) | rev4 | new bone layout (`linearbone` carries pos/rot/scale); 19.2+ has 1024-bone vertex indices |
| 20 | 20.0 | `_v16_t` | `studiohdr_v19_2_t` | rev4 | like 19.2/19.3 |

Detection, in order (`model.cpp:946-1049`, read):
1. `major` = asset entry version. Not in {8..20} -> unknown (rsx `assert(false)`, then `return`; respak: `UnsupportedVersion`).
2. **12**: the `studiohdr` is `*(int*)hdr` (first field of the asset header is the `data` pointer). Index `i` below is a 32-bit word of the studiohdr:
   - `(w[97] == 548 || w[41] == 548) && assetHdrSize == 120` -> 12.0. (`w[41]` is `boneindex`: "bones directly follow the header".)
   - `(w[101] == 520 || w[41] == 548 [sic, v8 size]) && assetHdrSize == 104` -> 12.1.
   - `(w[102] == 524 || w[41] == 524) && assetHdrSize == 104` -> 12.2. Same with 532 -> 12.4, 536 -> 12.5. Anything else -> unknown.
   - `w[101]`/`w[102]` is `sourceFilenameOffset` (the "text starts right after the header" trick). The 12.1 branch's alternative compares `w[41]` with the **v8** size (a typo; harmless because of the `||`).
3. **13**: `numbodyparts == 0` -> 13.0. Else take model 0 of bodypart 0 as `mstudiomodel_v12_1_t`; if its `meshindex == 0` -> 13.0 (assert). `modelSizeSingle = (model0 + meshindex - model0) / totalModelCount` (assumes the mesh array immediately follows all model structs). `== 116` (`sizeof(mstudiomodel_v13_1_t)`) -> 13.1 else 13.0.
4. **14**: `numlocalnodes == 0` -> 14.0. Else read `i32` at `hdr + localnodenameindex`; if `value + localnodenameindex < length` -> 14.1 else 14.0.
5. **19**: pak header `createdTime` (u64 at pak header offset 0x08, all pak versions; FILETIME, 100 ns since 1601) `>= 0x01DD1EED32D6C000` (rsx comment "07/29/2026") -> 19.3; else `sourceFilenameOffset == 244` (the u16 at studiohdr+216 in the v19_2 layout) -> 19.2; else `createdTime >= 0x01DC1DF805C28000` ("09/05/2025") -> 19.1; else 19.0.
6. Everything else: the major as is, minor 0.

Proposed respak: score function on `rpak.asset` with `kind == "mdl_"` -> score 10 (the container-provided kind), `Version { major: entry.version, minor }` from the table above. Needs: `entry.header_size` (asset header size), and the pak header `createdTime` (**not exposed by `respak-respawn` today**, `PakHeader` has no such field; needs adding for 19.x). Unknown major/minor combination -> `UnsupportedVersion`.
- Dead code: `GetModelPakVersion` (`model.h:287`) is never called (checked by grep); its `pHdrNew[54]` check for v19.2 does not match any field. Drop.
- rsx quirk: the detection reads the studiohdr from the live pointer with no bounds check (`pHdr->pBodypart(0)` etc.). Respak must do bounded reads and return `Corrupt`.

## Layout
Little-endian. "page ptr" = 8-byte pointer in the asset header, already relocated by the rpak volume to a `.mem` offset (null = 0 or `u64::MAX`, as in `txtr.rs`). Pointers *inside* `.mem` (page data, CPU data) are stored `{u32 page, u32 offset}` and need `RpakAsset::mem_offset_of`. The studiohdr itself uses **relative offsets only** (relative to the struct that holds them, or to the studiohdr base), no pointers, so once you know its `.mem` offset you can range-read it.

### Asset header (all offsets computed assuming natural alignment; sizes match rsx's static_asserts)
| field | v8 | v9 (9-12.0) | v12.1 (12.1-12.5) | v13 (13-15) | v16 (16-20) |
|---|---|---|---|---|---|
| `data` (page ptr, studiohdr + rmdl buffer) | 0x00 | 0x00 | 0x00 | 0x00 | 0x00 |
| `info` (runtime, 0 on disk) | - | 0x08 | 0x08 | 0x08 | - |
| `name` (page ptr, C string) | 0x08 | 0x10 | 0x10 | 0x10 | 0x08 |
| `physics` (page ptr, phy blob) | 0x18 | 0x20 | 0x20 | 0x20 | in CPU data |
| `vertexComponentData` (page ptr, vtx/vvd/vvc/vvw blob) | 0x20 | 0x28 | 0x28 | 0x28 | - |
| `staticStreamingData` (page ptr, inline VG) | - | 0x30 | 0x30 | 0x30 | 0x18 |
| `animRigs` (page ptr -> `u64 guid[numAnimRigs]`) | 0x28 | 0x38 | 0x38 | 0x38 | 0x20 |
| `numAnimRigs` u32 | 0x30 | 0x40 | 0x40 | 0x40 | 0x28 |
| `componentDataSize` u32 | 0x34 | 0x44 | 0x44 | 0x44 | - |
| `streamingDataSize` u32 (VG size after baking, page-aligned, may be oversized) | - | 0x48 | 0x48 | 0x48 | 0x2C |
| bbox min / max (2 x 3 f32) | - | - | - | 0x4C / 0x58 | 0x30 / 0x3C |
| `numAnimSeqs` | u32 @0x3C | u32 @0x64 | u32 @0x54 | u32 @0x6C | **u16 @0x4A** |
| `animSeqs` (page ptr -> `u64 guid[]`) | 0x40 | 0x68 | 0x58 | 0x70 | 0x50 |
| total | 0x50 | 120 | 104 | 128 | 96 |
Notes: the `gap_*`/`unk_*` names in rsx's v9 struct are wrong in position (alignment inserts 4 bytes before `unk_4C`); the offsets above are the real ones. Other fields are unknown. The `v8` column was not asserted by rsx.

### CPU data (v16-v20 only) `ModelAssetCPU_v16_t`, 16 bytes at `RpakAsset::data_mem_offset`
| off | type | field |
|---|---|---|
| 0x00 | `{u32 page, u32 offset}` | `physics` (pointer to phy blob in pages) |
| 0x08 | i32 | `dataSizePhys` (bytes) |
| 0x0C | i32 | `dataSizeModel` (bytes of the studio blob; v16+ has no `length` field in the studiohdr) |
For v8-v15 the sizes are in the studiohdr (`length` at 0x50, `phySize`, ...).

### studiohdr: the fields a mesh+skeleton+material export needs
Offsets within the studiohdr. `T` = type. v8-v15 headers are `pack(4)` all-i32; v16+ is `pack(2)` and mostly u16.

**v8 / v9-12.0 (`studiohdr_v8_t`, 548 bytes):** `id` 0, `version` 4, `checksum` 8, `sznameindex` 12, `name[64]` 16, `length` 80 (0x50), eyeposition 84, illumposition 96, hull min/max 108/120, view bb 132/144, `flags` 156 (0x9C), `numbones` 160, `boneindex` 164, `numhitboxsets` 176, `hitboxsetindex` 180, `numlocalseq` 192, `localseqindex` 196, `materialtypesindex` 204, `numtextures` 208, `textureindex` 212, `numcdtextures` 216, `cdtextureindex` 220, `numskinref` 224, `numskinfamilies` 228, `skinindex` 232, `numbodyparts` 236, `bodypartindex` 240, `numlocalattachments` 244, `localattachmentindex` 248, `meshOffset` 272, `surfacepropindex` 316, `mass` 336, `contents` 340, `sourceFilenameOffset` 388 (= `w[97]`), `vtxOffset/vvdOffset/vvcOffset/phyOffset` 448/452/456/460, `vtxSize/vvdSize/vvcSize/phySize` 464/468/472/476, `bvhOffset` 532, `vvwOffset/vvwSize` 540/544.

**v12.1-v13 (`studiohdr_v12_1_t` and v12.2/12.4/12.5 share the front part):** as v8 up to `localNodeDataOffset` 268; then `numikchains` 272 (no `meshOffset`/flex block), `uiPanelCount` 280, `numlocalposeparameters` 288, `surfacepropindex` 296, `mass` 316, `contents` 320, `bonetablebynameindex` 336, `meshCount` 340 (meshes per LOD), `meshOffset` 344, `boneStateOffset` 348 (relative to this field), `boneStateCount` 352, `hwDataSize` 360, `vgUnk` 364 (i16), `vgLODCount` 366 (i16), `lodMap` 368, `groupHeaderOffset` 372 (relative to this field), `groupHeaderCount` 376, `lodOffset` 380 (relative to this field), `lodCount` 384, `fadeDistance` 388, `gatherSize` 392, `sourceFilenameOffset` 404 (12.1) / 408 (12.2+, because of `unk_194` at 404). Offsets 0..340 are identical in all of 12.1-13; from `sourceFilenameOffset` on 12.1 and 12.2+ differ by 4.

**v14, v15 (`studiohdr_v14_t`, 544 bytes):** as v12.5 plus `unk_C8[2]` at 200: every field after `localseqindex` moves by +8: `activitylistversion` 208, `numtextures` 216, `textureindex` 220, `numskinref` 232, `numskinfamilies` 236, `skinindex` 240, `numbodyparts` 244, `bodypartindex` 248, `meshCount` 348, `meshOffset` 352, `boneStateOffset` 356, `boneStateCount` 360, `hwDataSize` 368, `lodMap` 376, `groupHeaderOffset` 380, `groupHeaderCount` 384, `lodOffset` 388, `lodCount` 392, `sourceFilenameOffset` 416 (= `w[104]`). `numlocalnodes` and `localnodenameindex` (used by the 14.1 check) also move by +8 (260, 268).

**v16 (`studiohdr_v16_t`, 224 bytes, `pack(2)`):**
| off | T | field |
|---|---|---|
| 0x00 | i32 | `flags` |
| 0x04 | i32 | `checksum` |
| 0x08 | u16 | `sznameindex` (`FIX_OFFSET`, relative to hdr) |
| 0x0A | char[33] | `name` |
| 0x2B | u8 | `surfacepropLookup` |
| 0x2C | f32 | `mass` |
| 0x30 | i32 | `contents` |
| 0x34 | u16 | `hitboxsetindex` |
| 0x36 | u8 | `numhitboxsets` |
| 0x37 | u8 | `illumpositionattachmentindex` |
| 0x38 | 5 x Vec3 | illumposition, hull_min, hull_max, view_bbmin, view_bbmax |
| 0x74 | u16 | `boneCount` |
| 0x76 | u16 | `boneHdrOffset` (array of `mstudiobonehdr_v16_t`) |
| 0x78 | u16 | `boneDataOffset` (array of `mstudiobonedata_v16_t`) |
| 0x7A | u16 | `numlocalseq` |
| 0x7C | u16 | `localseqindex` |
| 0x7E | u16[2] | unk |
| 0x82 | u8 / u8 | `activitylistversion` (char) / `numlocalattachments` |
| 0x84 | u16 | `localattachmentindex` |
| 0x86-0x8E | u16 x5 | numlocalnodes, localnodenameindex, localNodeDataOffset, numikchains, ikchainindex |
| 0x90 | u16 | `numtextures` |
| 0x92 | u16 | `textureindex` (array of u64 material guids) |
| 0x94 | u16 | `numskinref` |
| 0x96 | u16 | `numskinfamilies` |
| 0x98 | u16 | `skinindex` |
| 0x9A | u16 | `numbodyparts` |
| 0x9C | u16 | `bodypartindex` |
| 0x9E-0xA4 | u16 x4 | uiPanelCount/Offset, numlocalposeparameters, localposeparamindex |
| 0xA6 | u16 | `surfacepropindex` |
| 0xA8 | u16 | `keyvalueindex` |
| 0xAA | u16 | `virtualModel` |
| 0xAC | u16 | `meshCount` (meshes per LOD) |
| 0xAE | u16 | `bonetablebynameindex` |
| 0xB0 | u16 | `boneStateOffset` (relative to this field; `u8` map) |
| 0xB2 | u16 | `boneStateCount` |
| 0xB4 | u16 | `groupHeaderOffset` (relative to this field, `FIX_OFFSET`) |
| 0xB6 | u16 | `groupHeaderCount` |
| 0xB8 | u16 | `lodOffset` (relative to this field; `f32 switchPoint[lodCount]`) |
| 0xBA | u16 | `lodCount` |
| 0xBC | f32 | `fadeDistance` |
| 0xC0 | f32 | `gatherSize` |
| 0xC4 | u16 x2 | numsrcbonetransform, srcbonetransformindex |
| 0xC8 | u16 | `sourceFilenameOffset` (= 224 = header size, the v16 detector) |
| 0xCA | u16 | `linearboneindex` |
| 0xCC-0xD0 | u16 x3 | procBoneCount, procBoneOffset, linearProcBoneOffset |
| 0xD2-0xD4 | u16 x2 | boneFollowerCount, boneFollowerOffset |
| 0xD6 | u16 | `bvhOffset` |
| 0xD8 | char[2] | bvhUnk |
| 0xDA-0xDE | u16 x3 | unkDataCount, unkDataOffset, unkStrcOffset |
| 0xE0 | i32 | (v17+) `unk_E0`; v17 is 228 bytes |
**v19.2 / v20 (`studiohdr_v19_2_t`, 244 bytes):** as v17 up to `boneStateCount` 0xB2; then `boneStatePerLOD[8]` (u16) at 0xB4; every later field moves by +16: `groupHeaderOffset` 0xC4 (rsx asserts this), `groupHeaderCount` 0xC6, `lodOffset` 0xC8, `lodCount` 0xCA, `fadeDistance` 0xCC, ..., `sourceFilenameOffset` 0xD8 (216, compared with 244), `linearboneindex` 0xDA. The bone state map (`boneStateOffset`) is `u16` instead of `u8`.

`FIX_OFFSET(v)` for u16 offsets (`studio.h:6`): `(int)(v & 0xFFFE) << (4 * (v & 1))`. If bit 0 is set the offset is `(v & 0xFFFE) * 16`; otherwise it is `v`. Used on `sznameindex`, bone/material/bodypart/model/mesh offsets, etc. Some fields skip it (srcbonetransformindex), see rsx. Offsets are relative to the struct that holds the field unless stated, except `boneHdrOffset`/`boneDataOffset`/`bodypartindex`/`textureindex`/`skinindex`/`hitboxsetindex`/`localattachmentindex`, which are relative to the studiohdr base.

### Bones
- **v8, v9-12:** `mstudiobone_v8_t`, 184 bytes: `sznameindex` i32 0 (relative to the bone), `parent` i32 4 (-1 = root), `bonecontroller[6]` 8, `pos` Vec3 32, `quat` (x,y,z,w) 44, `rot` (RadianEuler) 60, `scale` Vec3 72, `poseToBone` 3x4 f32 84, `qAlignment` 132, `flags` 148, `proctype` 152, `procindex` 156, `physicsbone` 160, `surfacepropidx` 164, `contents` 168, `surfacepropLookup` 172, `unk_B0` 176, `collisionIndex` 180.
- **v12.1-v18:** `mstudiobone_v12_1_t`, 0xB4 (180) bytes: same, with `collisionIndex` (u8) at 176 and 3 pad bytes.
- **v16-v18 split form** (arrays at `boneHdrOffset` and `boneDataOffset`, index `i` in both):
  - `mstudiobonehdr_v16_t` 12 bytes: `contents` i32 0, `unk_4` u8 4, `surfacepropLookup` u8 5, `surfacepropidx` u16 6, `physicsbone` u16 8, `sznameindex` u16 10 (relative to the bonehdr, `FIX_OFFSET`).
  - `mstudiobonedata_v16_t` 128 bytes: `poseToBone` 3x4 f32 at 0, `qAlignment` 48, `pos` 64, `quat` 76, `rot` 92, `scale` 104, `parent` i16 116, `unk_76` u16 118, `flags` i32 120, `collisionIndex` u8 124, `proctype` u8 125, `procindex` u16 126.
- **v19+:** `mstudiobonehdr_v16_t` as above, plus `mstudiobonedata_v19_t` 16 bytes: `parent` i16 0, `unk_76` u16 2, `flags` i32 4, `collisionIndex` u8 8, `proctype` u8 9, `procindex` u16 10, `unk_C` i32 12. The default pose lives in the **linear bone** block at `linearboneindex` (FIX_OFFSET'd, relative to hdr; must be non-zero, rsx dereferences it unchecked): `mstudiolinearbone_v19_t` = nine u16 offsets (`numbones` at 0, then `flagsindex` 2, `parentindex` 4, `posindex` 6, `quatindex` 8, `rotindex` 10, `posetoboneindex` 12, `qalignmentindex` 14, `scaleindex` 16; the array offsets are relative to the linearbone struct, `FIX_OFFSET`), each pointing to a packed array of `numbones` elements (`i32 flags`, `i32 parent`, `Vec3 pos`, `Quat quat`, `RadianEuler rot`, `3x4 poseToBone`, `Quat qAlign`, `Vec3 scale`). rsx takes `parent` and `flags` from `bonedata`, the rest from the linearbone arrays.
- rsx's `ModelBone_t` keeps: name, parent, flags, physicsbone, surfaceprop, contents, pos, quat, rot, scale, poseToBone, proctype/proc. `pos`/`quat` are **parent-local bind pose**.

### Bodyparts, models, meshes
- **Bodypart:** v8-v14: `mstudiobodyparts_t`, 16 bytes: `sznameindex` 0 (relative), `nummodels` 4, `base` 8, `modelindex` 12 (relative to the bodypart). v15: `mstudiobodyparts_v15_t`, 24 bytes: `sznameindex`, `nummodels`, `base`, `modelindex`, `unk_10`, `meshOffset`. v16+: `mstudiobodyparts_v16_t`, 16 bytes: `sznameindex` u16 0, `modelindex` u16 2, `base` i32 4, `nummodels` i32 8, `meshOffset` i32 12.
- **Model** (strides matter, rsx indexes arrays of them):
  - v8: `mstudiomodel_v8_t` 136 bytes: `name[64]` 0, `unkStringOffset` 64, `type` 68, `boundingradius` 72, `nummeshes` 76, `meshindex` 80 (relative to the model), `numvertices` 84, `vertexindex` 88, `tangentsindex` 92, `numattachments` 96, `attachmentindex` 100, ..., `colorindex` 128, `uv2index` 132.
  - v9-v13.0 (`mstudiomodel_v12_1_t`) 112 bytes: same up to `tangentsindex`, then `numattachments` 96, `attachmentindex` 100, `colorindex` 104, `uv2index` 108. v13.1 (`_v13_1_t`) 116 bytes (+`uv3index`).
  - v14-v15 (`mstudiomodel_v14_t`) 124 bytes: `name[64]`, `unkStringOffset` 64, `type` 68, `boundingradius` 72, `meshCountTotal` 76, `meshCountBase` 80, `meshCountBlend` 84, `meshOffset` 88 (relative to the model), then vertex-cache leftovers up to `uv3index` 120.
  - v16+ (`mstudiomodel_v16_t`) 10 bytes: `unkStringOffset` u16 0, `meshCountTotal` u16 2, `meshCountBase` u16 4, `meshCountBlend` u16 6, `meshOffset` u16 8 (relative, `FIX_OFFSET`).
- **Mesh** (the fields that matter): material index and `meshid`.
  - v8 (`mstudiomesh_v8_t`, 0x5C): `material` i32 0, `modelindex` 4, `numvertices` 8, `vertexoffset` 12, `meshid` 32, `center` Vec3 36, `vertexloddata` 48 (`numLODVertexes[8]` after a pad int), ptr 84.
  - v9-v13 (`_v12_1_t`, 0x4C): `material` i32 0, `modelindex` 4, `numvertices` 8, `vertexoffset` 12, `meshid` 16, `center` 20, `vertexloddata` 32, ptr 68.
  - v14-v15 (`_v14_t`, 0x4C): same as v12.1 but `material` is u16 at 0 and `unk_2` u16 at 2.
  - v16+ (`_v16_t`, 20 bytes): `material` u16 0, `meshid` u16 2, `unk_4[4]`, `center` Vec3 8.
- `meshid` is the key into the VG: it indexes the LOD's mesh array (rev1: `LOD.meshIndex + meshid`; rev2+: `meshid` directly). A mesh with VG `flags == 0` in a LOD is empty and skipped (rsx drops it from the model's count).
- Only `meshCountBase` meshes are read from v14+ models; the `meshCountBlend` blend-shape meshes are **not** handled by rsx ("we do not handle blendstates currently", `:1784`).

### Materials and skins
- **v8-v15 `mstudiotexture_v8_t`, 12 bytes (`pack(4)`):** `sznameindex` i32 0 (relative to the entry; the material's short path as the studio compiler wrote it), `texture` u64 4 (**guid of the `matl` asset**). Array at `textureindex`, `numtextures` entries. `texture == 0` means none (rsx skips lookup).
- **v16+:** `textureindex` is a plain array of `numtextures` x `u64` guids. No names; rsx synthesizes `0x%llX` (`modeldata.cpp:1311`).
- **Skins:** `skinindex` -> `i16 skinref[numskinfamilies][numtextures]` followed by `numskinfamilies - 1` name offsets (`i32` v8-v15 / `u16` v16+, `FIX_OFFSET`'d relative to the studiohdr, only for families 1..). Family 0 = "default" (name not stored). The mesh's `material` is an index into family 0 first, then `skinref[family][material]` gives the index into the material table that the mesh actually uses. rsx's stride is `numMaterials` (`numtextures`), not the header's `numskinref` (`modeldata.h:958`); are they ever different? Unknown (open question).
- `cdtextures` (v8-v15 only, `cdtextureindex`/`numcdtextures`): material search directories. Not used by the Cast writer.

### Hitboxes, attachments (brief, deferrable)
- Hitbox set v8: `{i32 sznameindex, i32 numhitboxes, i32 hitboxindex}` 12 bytes; `mstudiobbox_v8_t` 44 bytes: `bone` 0, `group` 4, `bbmin` 8, `bbmax` 20, `szhitboxnameindex` 32, `forceCritPoint` 36, `hitdataGroupOffset` 40. v16+: set `{u16 sznameindex, u16 numhitboxes, u16 hitboxindex}` 6 bytes; `mstudiobbox_v16_t` 32 bytes: `bone` u16 0, `group` u16 2, `bbmin` 4, `bbmax` 16, `szhitboxnameindex` u16 28, `hitdataGroupOffset` u16 30. rsx exports them as OBJ cuboids (`ExportModelHitboxes` :761, one `o <set>_<i>_<name>` object per hitbox, 8 vertices and 6 quads each).
- Attachments v8: `{i32 sznameindex, i32 flags, i32 localbone, 3x4 local}`; v16+: `{u16 sznameindex, u16 localbone, i32 flags, 3x4 local}` (56 bytes). Not needed for the first slice.

### Hardware (VG) groups and LODs in the studiohdr
- **v9-v12.0 (rev1):** no group table; the whole VG is one group (`lodMap = 0xFF >> (8 - lodCount)`). LOD count and the bone map come from the VG header itself.
- **v12.1-v15 (`studio_hw_groupdata_v12_1_t`, 16 bytes, `pack(4)`):** `dataOffset` i32 0 (offset of the group in the VG blob), `dataSize` i32 4, `lodIndex` i16 8, `lodCount` i16 10, `lodMap` i16 12, 2 bytes pad. Never compressed. Table at `groupHeaderOffset` (relative to that field), `groupHeaderCount` entries. `lodCount` (studiohdr) is the number of LODs; switch points come from the VG LOD headers (rev2/3).
- **v16+ (`studio_hw_groupdata_v16_t`, 16 bytes, `pack(2)`):** `dataOffset` i32 0, `dataSizeCompressed` i32 4, `dataSizeDecompressed` i32 8, `dataCompression` u8 12 (`eCompressionType`: 0 NONE, 1 PAKFILE (the RTech/"rpak" LZ), 2 SNOWFLAKE, 3 OODLE; `utils.h:148`), `lodIndex` u8 13, `lodCount` u8 14, `lodMap` u8 15. LOD switch points: `f32[lodCount]` at `lodOffset` (studiohdr, relative to that field).
- Bone remap ("bone states"): `u8[boneStateCount]` at `boneStateOffset` (relative to that field) for v12.1-v19.1; `u16[]` for v19.2+; for rev1 it is inside the VG header (`boneStateChangeOffset/Count`, 1-byte entries). Maps the *vertex-local* bone index in the blend data to the global bone index. v19.2+ also has `boneStatePerLOD[8]`; rsx never reads it.

### Streamed vertex data: VG ("0tVG", `0x47567430`)
Four revisions. Blob layouts, all little-endian, native alignment. In rev2-rev4 every offset field is **relative to the address of that field itself** (`offsetof` + value), not to the header start.

**rev1 (v9-v12.0), `VertexGroupHeader_t`, 224 bytes, offsets relative to the header start:** `id` 0 (`0tVG`), `version` 4 (1), `unk` 8, `dataSize` 12, then (i64 offset, i64 count) pairs: bone map @16 (u8 entries), mesh headers @32, indices @48 (u16), vertices @64 (count = bytes), extra bone weights @80 (bytes), unknown @96, LODs @112 (8-byte `ModelLODHeader_t`), legacy weights @128, strips @144, `unused[8]` @160.
- `ModelLODHeader_t` 8 bytes: `meshIndex` u16 0, `meshCount` u16 2, `switchPoint` f32 4.
- `MeshHeader_t` 0x48 bytes: `flags` i64 0, `vertOffset` u32 8 (byte offset into the vertex buffer), `vertCacheSize` u32 12 (**vertex stride in bytes**), `vertCount` u32 16, `unk_14` 20, `extraBoneWeightOffset` i32 24 (bytes), `extraBoneWeightSize` i32 28 (bytes), `indexOffset` i32 32 (**element** offset into the u16 index array), `indexCount` i32 36, `legacyWeightOffset/Count` 40/44, `stripOffset/Count` 48/52, `unk_38[4]` 56.

**rev2 (v12.1-v13):** `VertexGroupHeader_t`, 32 bytes: `id` 0, `version` 4, `lodIndex` 8, `lodCount` 12, `groupIndex` 16, `lodMap` 20, `lodOffset` i64 24 (LOD array at `hdr + 24 + lodOffset`). `ModelLODHeader_t`, 24 bytes: `dataOffset` 0, `dataSize` 4, `meshCount` u8 8, `meshIndex` u8 9, `lodLevel` u8 10, `groupIndex` u8 11, `switchPoint` f32 12, `meshOffset` i64 16 (mesh array at `lod + 16 + meshOffset`). `MeshHeader_t`, 96 bytes: `flags` i64 0, `vertCacheSize` i32 8 (stride), `vertCount` i32 12, `indexOffset` i64 16 (bytes, relative to this field), packed i64 @24 (`indexCount` = low 56 bits, `indexType` = high 8 bits; "tri/quad", from 12.2), `vertOffset` i64 32 (relative to this field), `vertBufferSize` i64 40, `extraBoneWeightOffset` i64 48 (relative), `extraBoneWeightSize` i64 56 (bytes), legacy weights 64/72, strips 80/88.

**rev3 (v14-v15):** group and LOD headers same as rev2. `MeshHeader_t`, 112 bytes: as rev2 but `flags` u64, `vertCacheSize`/`vertCount` u32, the four `legacy`/`strip` fields at 64..96 are `deprecated_*`, `blendShapeVertOffset` i64 96, `blendShapeVertBufferSize` i64 104.

**rev4 (v16-v20):** `VertexGroupHeader_t`, 8 bytes: `lodIndex` u8 0, `lodCount` u8 1, `groupIndex` u8 2, `lodMap` u8 3, `lodOffset` u32 4 (LOD array at `hdr + 4 + lodOffset`; there is no `id`/`version`, the "0tVG" magic is gone). `ModelLODHeader_t`, 8 bytes: `meshCount` u8 0, `meshIndex` u8 1, `lodLevel` u8 2, `groupIndex` u8 3, `meshOffset` u32 4 (mesh array at `lod + 4 + meshOffset`). `MeshHeader_t`, 48 bytes: `flags` u64 0, `vertCount` u32 8, `vertCacheSize` u16 12 (stride), `vertBoneCount` u16 14, `indexOffset` u32 16 (bytes, relative to this field), packed u32 @20 (`indexCount` low 28 bits, `indexType` high 4), `vertOffset` u32 24 (relative), `vertBufferSize` u32 28, `extraBoneWeightOffset` u32 32 (relative), `extraBoneWeightSize` u32 36 (bytes), `blendShapeVertOffset` u32 40, `blendShapeVertBufferSize` u32 44.

How rsx walks it (read; `ParseModelVertexData_v16` :1830 is the template, the others differ only in header types):
```
groupBuf[g]  = decompress(vg blob [group.dataOffset .. +dataSizeCompressed], group.dataCompression -> dataSizeDecompressed)   // v12.1-15: raw copy of dataSize
for each group g, for lodLevel in 0..studiohdr.lodCount:
    if lodIdx == group.lodCount: break
    if (group.lodMap & (1 << lodLevel)) == 0: continue          // this LOD is in a different group
    lod = group.pLod(lodIdx++)
    for each bodypart bp, each model m in bp:                 // lists in studiohdr order
        for each of the model's (base) meshes sm:
            mh = lod.pMesh(sm.meshid)
            if mh.flags == 0: skip                              // empty in this LOD
            vertices  = mh.pVertices()   [mh.vertCount x mh.vertCacheSize]
            indices   = mh.pIndices()    [mh.indexCount x u16, triangle list]
            extraW    = mh.pBoneWeights() [mh.extraBoneWeightSize bytes of {i16 weight, i16 bone}]
            material  = sm.material
```
The result is **one parsed LOD per `lodLevel`**, each with `numModels` models (`GenerateName` -> `"<bodypart>_<modelIndexInBodypart>_LOD<lodLevel>"`), each model with its list of meshes. LODs are *independent* (separate vertices/indices), not index ranges of one buffer.

### Vertex layout (`mh.flags`, `vertCacheSize`), read `ParseVertexFromVG` (`modeldata.cpp:197`)
`flags` are the shader-input bits (`studio.h:287-317`); the *effective* layout rsx parses, in this order for each vertex at `vertBase + i * vertCacheSize`:
1. **Position**, by `flags & 3` (a value, not bit flags, since 13.1): 0 none (no bytes), 1 `Vec3` f32 (12 bytes), 2 `Vector64` (8 bytes), 3 `Vector48` (6 bytes; rsx writes (0,0,0) and skips 6 bytes, the format is unknown).
   - `Vector64`: three bitfields in one u64, LSB first: `x` 21 bits, `y` 21 bits, `z` 22 bits. `pos.x = x * 0.0009765625 - 1024`, `pos.y = y * 0.0009765625 - 1024`, `pos.z = z * 0.0009765625 - 2048` (steps of 1/1024; X,Y in [-1024, 1024), Z in [-2048, 2048)).
2. **Blend** if `flags & (0x1000 BLENDINDICES | 0x4000 BLENDWEIGHTS_PACKED)`: 8 bytes, see Decoding. Else default: one weight 1.0 on bone 0 (comment: "can only happen when a model has one bone"). `VERT_BLENDWEIGHTS_UNPACKED (0x2000)` is asserted absent.
3. **Normal+tangent**, always 4 bytes (`Normal32`), **regardless of the `VERT_NORMAL_*` / `VERT_TANGENT_*` flag bits** (they are never tested).
4. **Color** if `flags & 0x10`: 4 bytes `Color32` (r,g,b,a bytes in file order, as rsx copies them raw).
5. **Texcoord 0** if `flags & (0xF << 24)`: `Vec2` f32 (8 bytes).
6. **Extra texcoords**: for each further texcoord counted in `texcoordCount`: `Vec2` f32. Texcoord slot `n` has a 4-bit format at bits `24 + 4n`. `ParseTexcoords` (:422) counts the non-zero nibbles and asserts each is 2 (float2) or 0. Slots seen in comments: 0 (uv0), 2 (uv1, "second uv"), 3 ("wind/foliage" models).
7. `offset == vertCacheSize` is asserted at the end (debug only). In respak this must be a hard check; a mismatch means a layout rsx doesn't know.
Other sizes (`VertexSizeFromFlags_V13`, `studio.cpp:269`, used by the preview): position 0/12/8/6 by the 2-bit enum, color 4, `VERT_UNK (0x40)` 8, normals and tangents by a LUT, blend 8 or 12, texcoord formats 0,4,8,12,16,4,8,4,8,8 bytes for formats 0..9. rsx's parser doesn't use this function. When it disagrees with the actual stride, trust `vertCacheSize` and report.

`Normal32` (read, `compressedvector.h:400`): bit 31 = binormal sign, bits 30-29 = index `idx1` of the *dropped* (largest) component (0..2), bit 28 = sign of that component, bits 27-19 = `v2` (9 bits), bits 18-10 = `v3` (9 bits), bits 9-0 = tangent angle. Unpack: `v1 = sign ? -255 : 255`, `v2 = ((p >> 19) & 0x1FF) - 256`, `v3 = ((p >> 10) & 0x1FF) - 256`, `idx2 = (0x124 >> (2*idx1 + 2)) & 3`, `idx3 = (0x124 >> (2*idx1 + 4)) & 3`, `n[idx1] = v1/len`, `n[idx2] = v2/len`, `n[idx3] = v3/len` with `len = sqrt(255^2 + v2^2 + v3^2)`. (rsx uses `FastRSqrtFast`, an approximate reciprocal square root; use an exact one and accept the tiny difference.) Tangent (not emitted by the Cast writer, rsx's unpack leaves it commented out): `angle = (p & 0x3FF) * 0.00614192151`; basis from the normal: if `n.z < -0.9999`: `a = (0,-1,0)`, `b = (-1,0,0)` else `k = -1/(1+n.z)`, `a = (k*nx*nx + 1, k*nx*ny, -nx)`, `b = (k*nx*ny, k*ny*ny + 1, -ny)`; `tangent = a*cos(angle) + b*sin(angle)`; `tangent.w = (bit31) ? -1 : 1` (from `PackNormal`: `binormSign = tangent.w < 1 ? 1 : 0`).

## Data outside the header
- **studiohdr + rmdl buffer:** at `data` (the asset header's first pointer), in page data. Length: studiohdr `length` (v8-v15) or CPU `dataSizeModel` (v16+). The buffer holds bones, names, bodyparts, ..., the string table, and for v8-v15 up to `phySize`/etc. refer to `vertexComponentData`.
- **VG / hardware vertex data:** either (a) the asset's mandatory starpak entry, size = the starpak TOC entry for that offset (rsx takes the size from `parsedOffsets` in the starpak, not from `streamingDataSize`, which is rounded up to 4096), or (b) `staticStreamingData` in page data when there is no starpak reference (rsx: `vertexBuffer = streamed ? streamed : staticStreamingData`; `streamingDataSize` bytes, which may be oversized; use the group table's offsets/sizes). Optional starpak: not used. A model with neither loads "with no vertex data" (rsx logs and continues; respak: mesh output is empty, skeleton still written).
- **Compression** (v16+ per group): `eCompressionType` 0 NONE, 1 PAKFILE, 2 SNOWFLAKE, 3 OODLE. Handled by `RTech::DecompressStreamedBuffer(in, out, bufSize, type)` (`utils.cpp:2345`). respak has `rtech::decompress`, `snowflake::decompress`, `oodle::decompress(input, decoded_len)`. OODLE quirk: if the first decode call fails, rsx treats the input as uncompressed *without copying it to `out`* (`out` stays unwritten); respak should copy or error.
- **Component data** (`vertexComponentData`, v8-v15): non-streamed `vtx`/`vvd`/`vvc`/`vvw` blobs for the old Source-style path (v8 geometry; later versions only for raw export). Offsets/sizes in the studiohdr (`vtxOffset`, `vvdOffset`, ...). Not needed for v9+ meshes.
- **Extra bone weights** (`vvw::mstudioboneweightextra_t`, 4 bytes: `i16 weight`, `i16 bone`): per mesh, in the VG (bytes `extraBoneWeightSize` at `extraBoneWeightOffset`). Used when the studiohdr has `STUDIOHDR_FLAGS_USES_EXTRA_BONE_WEIGHTS` (`flags & 0x4000`).
- **Physics:** `physics` blob (phy / "IVPS"), size `phySize`/`dataSizePhys`; plus a BVH4 tree at `bvhOffset`. Deferrable; rsx exports STL (`ExportPhysicsModelPhy` :641, `ExportPhysicsModelBVH` :702). Not read in detail.
- **Sequences:** `numlocalseq`/`localseqindex` in the studiohdr, `animRigs`/`animSeqs` guid arrays in the asset header (external `arig`/`aseq` assets). Deferrable.

## Dependencies
- **`matl` assets** by guid, from the studiohdr material table (`mstudiotexture_v8_t.texture` / v16+ `u64`). rsx resolves with `g_assetData.FindAssetByGUID` across all loaded paks. The material gives the texture bindings; for Cast, `ParseMaterialIntoCast` (:2816) writes the textures through the `matl` -> `txtr` chain (`albedoTexture`, `diffuseTexture`, `normalTexture`, `glossTexture`, `roughnessTexture`, `specTexture`, `emissiveTexture`, `aoTexture`, `cavityTexture` resource names -> Cast material slots). If the `matl` is not loaded, rsx writes a stub material with only the name.
- **`arig`** (animation rigs) and **`aseq`** (sequences), by guid arrays in the asset header; only to be listed for now.
- In respak: `Metadata::dependencies()` = the `matl` guids (via `GuidRef`/`rpak.guid`), then rigs and seqs. `rpak.asset` already carries the pak's dependency guids; the guids used by the studiohdr should be a subset (not verified). Material resolution happens in the Cast encoder (project.md §5.5), not in the read edge.
- Skeleton for sequences: an `aseq` takes its skeleton from the model/rig; out of scope here.

## Decoding
Pseudocode for the first slice (versions 16-20, then the rest with the per-version tables above):
```
asset  = rpak.asset;  ver = detect(asset)
hdr    = read(asset.header, size per ver)                 // page ptrs relocated
cpu    = ver >= 16 ? read(asset.data_mem_offset, 16) : none
sh     = studiohdr at hdr.data (.mem), length = ver >= 16 ? cpu.dataSizeModel : sh.length
// skeleton
for i in 0..sh.boneCount:
    v16-18: hdr_i = bonehdr[i]; data_i = bonedata[i]; name = cstr(bonehdr_i + FIX_OFFSET(sznameindex)); parent = data_i.parent; pos/quat/rot/scale = data_i
    v19+:   same names; parent/flags from bonedata_v19[i]; pos/quat/rot/scale/poseToBone from linearbone arrays
    v8-v15: mstudiobone[i] (184 or 180 bytes)
// materials
materials[i] = u64 guid (v16+)   or  {name, guid} (v8-v15)
skin0[i]     = skinref[0][i]                              // i16
// VG
blob = streamed starpak data or staticStreamingData
for g in groups: decompress per group
for each LOD/model/mesh (loop in Layout):
    decode vertices per "Vertex layout"; decode blend per below
    indices: copy u16 triangle list
// blend (8 bytes at the vertex's blend offset)
packed = { u16 w0, u16 w1_or_extraIdx }; idx = 4 bytes after it
if ex (studiohdr.flags & 0x4000):
    first, last, unk, count = idx (256: u8 each; 1024 (v19.2+): bits 0-9 first, 10-19 last, 20-23 unk, 24-31 count)
    out[0] = (bone: map[first], weight: unpack(w0))          ; remaining = 32767 - w0
    for k in 1 .. count-1: e = extra[w1 + (k-1)]; out[k] = (map[e.bone], unpack_extra(e.weight)); remaining -= e.weight
    if count > 0: out[count] = (map[last], unpack(remaining))
else:
    count = idx[3]                   // 0, 1 or 2; count+1 weights (<=3)
    for k in 0..count-1: out[k] = (map[idx[k]], unpack(w[k])); remaining -= w[k]     // w has only 2 entries
    out[count] = (map[idx[count]], unpack(remaining))
unpack(w) = (w + 1) / 32768.0                                // UNPACKWEIGHT, so 32767 -> 1.0
map = bone state array (u8 or u16 by version), identity when boneStateCount == 0
```
Notes:
- `remaining` is a u16 starting at 32767; it is the weight left for the last bone, so weights sum to ~1.
- Triangle winding: indices are triangle lists, front-face order of the engine. Cast needs the order reversed (see Output). `indexCount` must be a multiple of 3.
- `numLODs == 0 || numBodyparts == 0` -> no geometry (rsx asserts, then writes the skeleton only).
- A mesh whose `vertCount == 0` is not exported (rsx skips it in `ModelExportInfo`, and in `HandleModelMaterials`, but not in the Cast mesh loop; see quirks).
- All `FIX_OFFSET` offsets must be bounds-checked against the studiohdr length.

## Output
**rsx** (`ExportModelAsset` :819, `setting`): Cast (default), RMAX, SMD, raw `.rmdl` (+`.phy`, `.rson` dependency manifest, `.vg`, `.vg_static`, `.vvd`/`.vvc`/`.vvw`/`.dx11.vtx`), STL (Valve physics `.phy`, Respawn BVH physics), hitboxes OBJ. A QC file is always written alongside 3D models (`exportQC = true`, hard-coded, `ExportModelMeshes` :3231). LODs are exported as **separate files per (LOD, model)**, not combined (`lodAsModel = false`).

**rsx Cast output** (`ExportModelSetupCAST` :2558, `ExportModelCAST` :2877), file `<dir>/<stem>/<bodypart>_<i>_LOD<n>.cast` (one per LOD and per bodypart model), or `<stem>.cast` with only the skeleton when `LODCount() == 0`:
- Header: `u32 magic = 0x74736163 ("cast")`, `u32 version = 1`, `u32 rootNodes`, `u32 flags = 0` (16 bytes).
- `CastNodeHeader` (24 bytes): `u32 id` (FourCC in file order: `root`, `modl`, `mesh`, `skel`, `bone`, `matl`, `file`, ...), `u32 nodeSize` (**rsx writes the size including the 24-byte header and all properties and children**; the Cast spec comment says "following the node"; unverified which one loaders need), `u64 hash`, `u32 propertyCount`, `u32 childCount`. Children follow the properties, depth-first.
- Property: `u16 id`, `u16 nameSize`, `u32 arrayLength`, name bytes (no NUL), then data. Type ids: `b` u8, `h` u16, `i` u32, `l` u64, `f` f32, `d` f64, `s` NUL-terminated string (written with `strnlen(..., MAX_PATH)`, so strings are capped at 260 bytes), `v2`, `v3`, `v4` as the u16 multi-char constants (`'v2'` etc., written by the C++ compiler as `0x7632` etc.; not verified against a real file). Strings: `arrayLength` is 1.
- Node tree: `root` -> `modl` (hash = `StringToGuid(model name)`, no properties) -> children: `skel` (hash `StringToGuid(stem)`) with `bone` children, then `matl` nodes, then `mesh` nodes.
- **Bone** (hash 0): props `n` (string name), `p` (i32 parent index, -1 for roots), `lp` (v3, local position), `lr` (v4, local rotation x,y,z,w). Parent-local bind pose from `pos`/`quat`. rsx does not write `s` (scale) or `ssc` here (a scale overload exists but is unused for models).
- **Material** (`matl`, hash = material guid): `n` (string, the material name's part after the last `/`; for a missing `matl`, the studio name's last component, which for v16+ is `0x<GUID>`), `t` = `"pbr"`; for each bound texture with a name, a `l` property keyed `albedo`, `diffuse`, `normal`, `specular`, `emissive`, `gloss`, `roughness`, `ao`, `cavity` (value = the texture guid) and a child `file` node (hash = texture guid) with `p` = relative path to the exported PNG (`<info.exportPath>/<name>.png`). Textures are always converted to PNG (the code says Blender's DDS support is bad).
- **Mesh** (`mesh`, hash = `StringToGuid("<model>_<material>")`; `<material>` is the matl name after the last slash or the **decimal** guid if the matl isn't loaded): props `n`, `vp` (v3 positions), `vn` (v3 normals), `vc` (`i` u32 colors, only if `flags & 0x10`; raw RGBA bytes), `u0`, `u1`, ... (v2 UV buffers, one per texcoord slot present), `wb` (bone indices, `b` or `h` by bone count: `h` if `boneCount > 256`), `wv` (f32 weights), `f` (`h` u16 triangle indices, winding reversed: `(i0,i1,i2)` -> `(i2,i1,i0)`), `ul` (`h` UV layer count), `mi` (`h` max influence = mesh's `weightsPerVert`), `m` (`l` material guid). No `vt` (tangent) buffer and no `sm` (skinning method) are written.
- Skin: rsx exports **skin 0** unless the global setting `exportModelSkin` selects another (`g_rsxSettings.previewedSkinIndex`); the mesh's material = `skin.indices[mesh.materialId]`.
- UVs are written **as stored** (no V flip); SMD flips (`1 - v`). Whether Blender's Cast importer expects flipped V is not verified (open question).
- Static props (`flags & 0x10`) get a coordinate flip in SMD/QC only (`StaticPropFlipFlop`: `(x, y) -> (y, -x)`); the Cast writer does not.

**Proposed respak targets:**
- A new `rmdl` format: read edge `rpak.asset (mdl_) -> rmdl` that parses the asset header, studiohdr tables, and the VG lazily (a `RmdlAsset` with `Metadata` (name, guid, dependencies), and the new `Mesh` and `Skeleton` capabilities: these two are empty traits in `respak-core` today, so this slice defines them).
- `Skeleton`: ordered bones `{ name, parent: Option<u32>, local_pos: [f32;3], local_rot: [f32;4] (x,y,z,w), scale: [f32;3], world_bind: Option<[f32;12]> }`.
- `Mesh` (per LOD): groups `{ name, bodypart, model_index, lod, material: u64 guid, positions, normals, optional tangents, colors, uv layers, bone weights (per vertex: up to N (bone, weight)), triangle indices (u16 or u32) }`. Keep the stored order; document the winding convention in the trait and convert at the encoder.
- `rmdl -> cast` convert edge (a composite output: folder `<stem>/`, `<stem>.cast`, `textures/*.png`, per project.md §5.5/§12 composite-output rules). Options: `{ "lods": "0" | "all" | [..], "skin": 0, "textures": true, "image": "png" }`. Suggested default: **LOD 0 only, all bodyparts, one `.cast` file** (rsx's per-model file split is an artefact of its exporter structure; a single `.cast` with one `mesh` node per mesh is valid Cast). Tell the user (open question).
- Cast encoder: the encoder asks for each material through the mediator (`matl` -> `Metadata`/`Bitmap` pipeline already in respak), converts textures to PNG and writes them via `cx.write`; no new capability.
- Later targets, not in the first slice: `rmdl -> hitboxes.obj`, `rmdl -> collision.stl`, `rmdl -> smd/qc`, animations through `aseq`/`arig`.

## rsx quirks
Keep / drop / fix, listed with the behaviour change against rsx (project.md §12):
- **Detection by pointer reads with no bounds checks**, version heuristics using `sourceFilenameOffset == header size`, a model-stride division, and the pak file's `createdTime` (19.x). Keep the rules; make all reads bounded; `createdTime` rules are date-based guesses, document them as such.
- `GetModelVersionFromAsset` never returns `12_3`; `GetModelPakVersion` is dead code. Drop the second; keep 12.3 as a name only.
- The 12.1 branch alternative `pMDL[41] == sizeof(studiohdr_v8_t)` is a copy-paste typo; no effect.
- Header `vvcOffset` is initialised from `vvdOffset` (v12.1 and v12.2 constructors, `modeldata.cpp` ~:660-760; other versions not checked), so raw `.vvc` export would use the wrong offset. Only matters for raw export.
- **`FIX_OFFSET`** on 16-bit offsets: bit 0 is a "multiply by 16" flag. Keep exactly; some u16 offsets do not go through it (e.g. `srcbonetransformindex`).
- **Bone state default map is 8 entries** (`s_DefaultBoneStates_*`). With no bone states and a bone index >= 8, rsx reads out of bounds. Respak: identity map without a table (behaviour change).
- **Texcoord loop bug:** the extra-texcoord loop tests `VERT_TEXCOORDn(localIdx)` (a constant) instead of the mesh's flags, so the "skip empty slot" check never fires; it reads the next `texcoordCount - 1` Vec2s sequentially. Equivalent for contiguous slots only. Respak should check the real nibbles and error if slots are non-contiguous.
- **Normal flags ignored:** the parser always reads a 4-byte `Normal32`; `VERT_NORMAL_*`/`VERT_TANGENT_*` bits are never inspected. Any mesh with another normal layout would be misparsed (only debug-asserted by `offset == vertCacheSize`). Respak: check the stride, error on mismatch.
- **`Vector48` positions** (position enum 3, Switch) are returned as zero; the format is unknown. Respak: unsupported -> warning/`UnsupportedVersion`.
- **Cast bone weight buffer indexing:** in `ExportModelCAST` the per-vertex write index is `baseWeightIndex = ((weightsPerVertMax * curIndex) + (mesh.weightsPerVert * vertIdx)) * boneIndiceWidth`, used as an *element* index into typed arrays; for `uint16` indices (more than 256 bones) the weight array and bone index array get indexed at twice the position the property pointers use, so the output is wrong for models with more than 256 bones. Does not matter for typical models. Do not port; write per-mesh buffers.
- **Per-mesh duplicate Cast `matl` nodes:** `ParseMaterialIntoCast` is called once per mesh and does not dedupe, so a material used by N meshes gets N identical nodes in the file. Dedupe by guid.
- Mesh name uses the **decimal** guid when the `matl` isn't loaded, hex (`0x...`) elsewhere. Pick one.
- The weight `UNPACKWEIGHT` formula adds 1 (`(w + 1) / 32768`), so a lone `32767` weight is exactly 1.0 and a zero weight is `1/32768`, not 0. Keep.
- The `FastRSqrtFast` (approximate) normal normalisation. Use exact.
- `numvertices`/`vertexoffset`/`vertexloddata` in the studio mesh and model are old Source vertex-cache fields; they are not used by the VG path. Ignore.
- LOD thresholds: rev2/3 take `switchPoint` from the VG LOD header, rev4 from the studiohdr's `f32[]`; rev1 from the VG LOD header. Keep.
- Empty-mesh handling: a mesh with VG `flags == 0` is dropped from its model; the *model* keeps existing, possibly with zero meshes. The Cast loop still writes a file for it unless `meshCount` is 0 (it does not skip those, `ExportModelMeshes` :3287). Respak: skip models with no meshes.
- Skin handling: `HandleModelMaterials` runs once per export for LOD 0 meshes only and builds the material table; a mesh in a lower LOD with a material not used in LOD 0 would fail `materials->contains` (assert). Respak: build the table from all exported meshes.
- Global state: `g_rsxSettings.exportModelSkin`, `previewedSkinIndex`, `exportMaterialTextures`, `exportModelMatsTruncated`, `exportPathsFull` drive the output. Not ported; they become target options.
- Material textures are exported to PNG directly inside `HandleModelMaterials` (a side effect while "preparing" a model), possibly concurrently for models sharing materials. Respak linking dedupes per project.md §5.5.
- `ExportModelStreamedData` for v16+ concatenates decompressed groups back to back in `outBuf` regardless of each group's `dataOffset`, producing a `.vg` whose layout differs from the stored one. Raw export only; drop.

## Reuse in respak
- `rpak.asset` (`header_path`, `data_mem_offset`, `stream`, `volume`, `mem_offset_of`, `read_mem`), the `txtr.rs` null-pointer convention and bounded C-string reads, `starpak::split_reference` + `Starpak::entry(offset)` for the VG slice, `rtech.rs`/`snowflake.rs`/`oodle.rs` decompressors, `guid::string_to_guid` for Cast hashes, `GuidRef` + `identify` for `matl` (as in `matl_json.rs::locate`) and the existing `matl`/`txtr` pipeline for textures.
- New: `rmdl.rs` (asset/CPU headers, studiohdr tables per version, bones, bodyparts/models/meshes, skins, hw groups, VG parser for rev1..rev4, vertex decoder), `skeleton.rs`/`mesh.rs` capability fill-ins in `respak-core`, a Cast writer (`respak-formats` or a new module; the node/property structs above are the whole format), `rmdl_cast.rs` (encoder), and `createdTime` in `PakHeader`.
- The existing rpak `header()` does not expose `createdTime` (pak header 0x08). Needed for v19.1/19.3.

## Tests
- Synthetic: build a minimal v16 asset in bytes: asset header (96 bytes) + CPU data (16 bytes) + a studiohdr (224 bytes, `pack(2)`) with 2 bones (v16 split form), 1 bodypart, 1 model, 2 meshes, 2 materials (guids), 1 skin family; a VG blob (rev4) with one group, one LOD, two meshes: one with position enum 1 (Vec3) + packed blend (non-extra) + `Normal32` + 1 texcoord, one with `Vector64` positions + color + 2 texcoords + extra weights. Check: bone name/parent decode, `FIX_OFFSET` both parities, `Vector64` unpack of known values (e.g. `x = 0` -> -1024), `Normal32` unpack of axis-aligned normals (idx1 0/1/2, both signs), weights sum to 1, triangle winding reversed in the Cast output, Cast property/node sizes.
- Detection fixtures: header size 120 vs 104 (12.0 vs 12.x), `sourceFilenameOffset` 520/524/532/536, 13.1 by model stride 116 vs 112, 14.1 by the local-node-name test, 19.x by `createdTime` and `sourceFilenameOffset == 244`, unknown major (21) -> `UnsupportedVersion`.
- Edge cases: model with no LODs (skeleton only), missing starpak entry (mesh empty + `MissingDependency` warning), `staticStreamingData` path (no starpak), compressed group per codec (needs a small OODLE/PAKFILE sample), more than 256 bones (`h` bone index), missing `matl` (stub material), a `vertCacheSize` that does not match the layout (error), non-contiguous texcoord nibbles, truncated studiohdr (bounded read -> `Corrupt`).
- Real files (no Steam runs without permission): any Apex `common.rpak` model for v16+ (a static prop, e.g. under `mdl/.../*.rmdl` with `.starpak`), a character with extra bone weights, a weapon with several bodyparts. Compare vertex/index counts and bone lists against rsx's Cast export (allowing for single-file vs per-model output and for the dropped quirks above).

## Open questions
1. Which versions are in scope for the first slice? Proposal: **16-20** (current Apex, rev4 VG, all u16 offsets), reading both v16/17/18/19.0/19.1 (studiohdr_v17/v16) and 19.2+ (v19_2). v12.1-v15 (rev2/rev3) and v9-v12.0 (rev1) as a follow-up, v8 never (needs the VTX/VVD path)? Which games/seasons does the user actually have?
2. Cast output granularity: rsx writes one `.cast` per (LOD, bodypart model). Proposed: one `.cast` per rmdl, LOD 0 by default, with a `lods` option. OK?
3. File extension for the asset in the pak volume: `.mdl_` (stored FourCC) or `.rmdl` (what the model's own name ends with)? The name inside is `mdl/.../x.rmdl`.
4. `Mesh` / `Skeleton` capabilities: define them in this slice (proposed shapes above) with the Cast encoder as the first consumer, or a narrower internal type first?
5. Cast details to verify against a real importer (Blender cast plugin, Maya): UV V orientation (rsx writes raw), `nodeSize` convention (rsx writes the node's total size including its header), and the `v2`/`v3`/`v4` property id byte order. Is there a reference Cast file available to compare?
6. Skins: export only skin 0 or all families as separate material variants? rsx exports one skin chosen by a global setting.
7. Bone weights beyond 3 per vertex, and meshes above 65535 vertices: keep `u8/u16` Cast typing like rsx (and `h` face indices), or widen? (`ul`/`mi` types are `h` in rsx.)
8. Blend shapes (`meshCountBlend`, VG `blendShapeVert*`): rsx ignores them. Park in the backlog?
9. Does `numskinref` ever differ from `numtextures` (rsx uses `numtextures` as the skin row stride)? Needs a real-file check.
10. `Vector48` positions (Switch) and `VERT_UNK (0x40)` (8 bytes, "gets ignored") are unknown. Out of scope?
11. Whether `streamingDataSize` is always 4096-aligned and whether a model can have **both** a starpak stream and `staticStreamingData` (rsx prefers the stream; the raw exporter writes both).
12. Should the Cast writer live in `respak-formats` (like `png`/`dds`) so other converters can reuse it (animations via `aseq`)?
