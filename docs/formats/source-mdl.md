# mdl, seq · Source-engine models (Titanfall 1/2 loose `.mdl`) and their sequences
**rsx sources:** `core/filehandling/mdl.cpp` (`HandleMDLLoad` :11), `core/filehandling/load.cpp` (`GroupPathByExtension` :15), `game/model/sourcemodel.{h,cpp}` (`CSourceModelAsset`, `CSourceSequenceAsset`, `LoadSourceModelAsset` :194, `ParseSourceModelTextureData` :48, `ExportSourceModelAsset` :309, `LoadSourceSequenceAsset` :406, `PostLoadSourceSequenceAsset` :436, `ExportSourceSequenceAsset` :485), `game/rtech/utils/studio/studio.{h,cpp}` (`studiohdr_short_t` :1106, `StudioLooseData_t` :1125 / `.cpp` :7, :120, `VerifyFileIntegrity` :163, `vvd`/`vvc`/`vvw` structs :50-281, `s_StudioLooseDataExtensions` :1197, flags, `mstudio*` generic structs), `studio_r1.{h,cpp}` (`r1::studiohdr_t`, `studiohdr2_t`, `mstudiobone_t`, `mstudiomesh_t`, `mstudiomodel_t`, `mstudiotexture_t`, `ExtractAnimValue`), `studio_r2.{h,cpp}` (`r2::studiohdr_t`, `mstudiobone_t`, `mstudioseqdesc_t`, `mstudioanimdesc_t`, `mstudio_rle_anim_t`, `CalcBone*`), `optimize.h` (VTX), `phyfile.h` (PHY), `core/mdl/modeldata.{h,cpp}` (`ParseModelVertexData_VTX` h:1184, `ParseModelBoneData` h:1123, `ModelParsedData_t` ctors cpp:465/:524, `Vertex_t::ParseVertexFromVTX` cpp:316/:344, `HandleModelMaterials` :2473, `ExportModelMeshes` :3229, `ExportModelCAST` :2877, `ExportModelSetupCAST` :2558, `ExportSeqDescCast` :3409), `core/mdl/animdata.{h,cpp}` (`ModelSeq_t` r2 ctor :436, `ModelAnim_t` r2 ctor :89, `pAnimdataNoStall` :264, `ParseAnimation` r2 :935, `ParseAnimDesc_Origin` :892, `r1::Studio_AnimPosition` :1428), `core/math/compressedvector.h` (`Quaternion64`, `Vector48`), `game/asset.h` (FourCC and folder tables) · **Games / versions:** Titanfall 1 (studio **version 52**, "r1") and Titanfall 2 (**version 53**, "r2"); version 54 (Apex) is rejected here (rpak only, see [rmdl](rmdl.md)); version 63 (Titanfall Xbox 360) passes the load switch but cannot be loaded · **Priority hint:** low to medium (only Titanfall 1/2 files; project.md §2 says a Valve-Source crate is "not planned", the backlog only lists "Titanfall 2 sequences (`source-mdl`)"), **L** for the model (studiohdr, VTX/VVD assembly, skins, Cast), **M** for sequences on top of it. This is the spec of the biggest rsx loose-file path; an r2-only first slice (embedded files, no sibling lookup) is the cheapest start.

> **Scope of what was read.** Read directly in this run: `mdl.cpp`, `sourcemodel.{h,cpp}`, `studio.cpp`, `studio.h` (vvd/vvc/vvw, flags, loose data, generic structs), `studio_r1.{h,cpp}` (header, bone, mesh, model, texture, linear bone), `studio_r2.{h,cpp}` (header, bone, hitbox, seqdesc, animdesc, rle anim, mesh, model, texture, `CalcBone*`), `optimize.h`, `phyfile.h`, `ParseModelVertexData_VTX`, `ParseVertexFromVTX`, `ParseModelBoneData`, `ModelMeshData_t::ParseMesh_VTX`, the Cast model writer, `HandleModelMaterials` (head), the r2 `ModelSeq_t`/`ModelAnim_t` ctors, `ParseAnimation` (r2), `ParseAnimDesc_Origin`, `r1::Studio_AnimPosition`, `ExportSeqDescCast`, `Quaternion64`/`Vector48`. Not read: `AngleQuaternion`/`QuaternionBlend`/`QuaternionAlign`/`AngleDiff` (stock Valve mathlib), `float16.h` conversion, the SMD/RMAX/QC writers, IK-rule/local-hierarchy structs (`mstudioikrule_t`, `mstudiolocalhierarchy_t`), per-tri AABB and UI-panel blocks of the r2 header, the r1 animation structs (`mstudioanimdesc_t` 0x64, `mstudioseqdesc_t` 0xD4: rsx never parses r1 sequences), `StringToGuid`. No game file was opened, so offsets were computed by hand from the struct definitions and cross-checked with rsx's own `static_assert` sizes (all sums below match the asserts).

## Summary
Titanfall 1 and 2 ship models as Valve Source "IDST" studio files: an `.mdl` with the skeleton, bodyparts / models / meshes, material names, skins, hitboxes, attachments, pose parameters and sequences, plus separate vertex data. **Titanfall 1 (v52)**: the vertex data lives in sibling files next to the `.mdl` (`<name>.dx11.vtx` mesh strips, `<name>.vvd` vertices, optional `<name>.vvc` colours / UV2, `<name>.phy` collision). **Titanfall 2 (v53)**: the same four blobs are **embedded in the `.mdl`** (offsets and sizes in the header). A user wants a Cast model (mesh per LOD and body part, skeleton, material references) and, for Titanfall 2, one Cast animation per sequence blend. The "loose `.seq`" in the task name is **not a file on disk**: rsx synthesises one `seq` asset per `mstudioseqdesc` of a v53 `.mdl` (named `models/<model dir>/<model stem>/<label>.seq`) so the existing animation exporter can write it.

## Where it lives
- **Container:** loose files on disk (no rpak, no starpak, no VPK entry in rsx; the Steam depot filter just lets `.mdl` through, `core/steam/steam_load.cpp` `SteamIsSupportedDepotFile`). rsx loads only the `.mdl` the user picks (`GroupPathByExtension` :23, extension `.mdl`, container type `MDL`) and finds the siblings itself.
- **FourCC / kind:** `mdl` is `AssetType_t::MDL = MAKEFOURCC('m','d','l','\0')` (`asset.h:23`), bound as the literal `'ldm'` (`sourcemodel.cpp:395`, `sourcemodel.h:71`). `seq` is `MAKEFOURCC('s','e','q','\0')` (`asset.h:24`), bound as `'qes'` (`sourcemodel.cpp:552`). Both have header alignment 8 (unused). There are no on-disk FourCCs: a real file starts with the studio magic `IDST` (`49 44 53 54`, `studio.h:20`).
- **Names:**
  - Model: the **internal name** of the studiohdr: v52 through `studiohdr2` (`studiohdr_t.studiohdr2index` -> `studiohdr2_t.sznameindex`, relative to `studiohdr2`); v53 `sznameindex` at 0x0C (relative to the studiohdr). rsx sets the asset name to `models/<internal name>` and the guid to `StringToGuid(<internal name>)` (`sourcemodel.h:35`, `.cpp:264`). The on-disk file name is not used for the name. `r1::studiohdr_t::pszName()` returns **null** when `studiohdr2index == 0` (then `StringToGuid(nullptr)` and `SetName(null)` fail; no such file seen). Whether the internal name carries a `.mdl` extension or a directory is unknown (rsx handles both: `stem()` / `GetStringAfterLastSlash`).
  - Sequence (v53 only): `models/<model dir>/<model stem>/<label>.seq`, `<label>` = `mstudioseqdesc.szlabelindex` string (`mdl.cpp:87-94`); the guid is `StringToGuid(that path)`. Sequences are created for every local sequence of the model, with no deduplication of equal labels.
  - Export folder prefix for both: `models` (`s_AssetTypePaths`, `asset.h:176-177`).
- **Sibling lookup (v52 only)** (`StudioLooseData_t` ctor, `studio.cpp:7-118`): start from the full on-disk path of the `.mdl`; for each extension in `[".dx11.vtx", ".vvd", ".vvc", ".vvw"]` set the file name to `GetStringAfterLastSlash(<internal model name>)` and call `replace_extension(<ext>)`; then `.phy` the same way. So the siblings are searched in the **same directory** as the `.mdl`, named after the **internal name's last component**, not the on-disk file name. A missing file is skipped silently (size 0). Files are read whole into one 16-byte-aligned buffer. `.vvc` is **always discarded** for v52 (`hasIDCV` defaults to `false` in `LoadSourceModelAsset` :210, comment "file exists but is not used (looking at you ogre_titan.mdl)"). `.vvw` is read when present but never used by the v52 vertex path.
- **Sibling lookup (v53):** none. `StudioLooseData_t(const char* file)` (:120) takes `vtxOffset/vvdOffset/vvcOffset/phyOffset` and the four sizes from the v53 header; offsets are relative to the start of the `.mdl`.

## Versions and detection
`HandleMDLLoad` (`mdl.cpp:33-60`): read the whole file, treat the start as `studiohdr_short_t { i32 id; i32 version; i32 checksum }`. `id != 'IDST'` -> skipped ("invalid file"). Then:

| `version` | name | rsx behaviour |
|---|---|---|
| 52 | r1, Titanfall 1 | supported: studiohdr `r1::studiohdr_t` (0x198), geometry from sibling files |
| 53 | r2, Titanfall 2 | supported: `r2::studiohdr_t` (0x2CC), geometry embedded, sequences synthesised |
| 63 | r1 Xbox 360 | **accepted by the switch** (`mdl.cpp:46`) but `LoadSourceModelAsset` has no case 63: `assertm(false)` + `return` with no name / parsed data. The asset stays in the list in a half-initialised state. Respak: `UnsupportedVersion`. |
| 54 | r5 (Apex) | logged "only supported through RPak export", skipped |
| other | | logged, skipped |

`AssetVersion_t.majorVer` = `version` (52 or 53), no minor. **Proposed:** score function on a loose file path: `id == 'IDST'` and `version in {52, 53}` -> `Version { major: 52 | 53, minor: 0 }`, score 10 (the magic is unique, no ambiguity with other kinds); anything else with the magic -> `UnsupportedVersion`. The header length helper (`studiohdr_short_t::length`, `studio.h:1114`) reads `length` at offset 76 for v52 and 80 for v53 (`UseHdr2()` is true for versions that are **not** 53/54, i.e. 52 and 63): the `length` field differs in position because v53 moved `sznameindex` to 0x0C. rsx copies exactly `length` bytes of the file as the "asset data", so everything addressed inside the file (v53 embedded blobs, strings) must lie within `length`.

`checksum` (offset 8) must equal the checksum field of every sibling/embedded blob (see Layout). Variants inside a version (minor layouts) are not distinguished by rsx.

## Layout
Little-endian. `Vector` = 3 x f32 (12 bytes), `Quaternion` = 4 x f32 (x, y, z, w; 16), `RadianEuler` = 3 x f32, `matrix3x4` = 12 x f32 (48). All `*index` fields are **byte offsets relative to the struct that holds them** unless noted, no pointers. The studio structs are `pack(4)` (`studio_r1.h:3`, `studio_r2.h:4`). VTX structs are `pack(1)`. Offsets below are computed; every total matches the `static_assert` in rsx.

### v52 studiohdr (`r1::studiohdr_t`, 0x198 = 408 bytes)
| off | field | notes |
|---|---|---|
| 0 | i32 `id` | `IDST` |
| 4 | i32 `version` | 52 |
| 8 | i32 `checksum` | must match vtx/vvd/(vvc)/phy |
| 12 | char[64] `name` | |
| 76 | i32 `length` | file size |
| 80 / 92 / 104 / 116 / 128 / 140 | Vector `eyeposition`, `illumposition`, `hull_min`, `hull_max`, `view_bbmin`, `view_bbmax` | |
| 152 | i32 `flags` | see flag table |
| 156 / 160 | `numbones` / `boneindex` | `mstudiobone_t` (216) |
| 164 / 168 | `numbonecontrollers` / `bonecontrollerindex` | not parsed |
| 172 / 176 | `numhitboxsets` / `hitboxsetindex` | `mstudiohitboxset_t` (12) |
| 180 / 184 | `numlocalanim` / `localanimindex` | r1 anim descs, not parsed |
| 188 / 192 | `numlocalseq` / `localseqindex` | r1 seqdescs, not parsed |
| 196 / 200 | `activitylistversion` / `eventsindexed` | runtime |
| 204 / 208 | `numtextures` / `textureindex` | `mstudiotexture_t` (0x40) |
| 212 / 216 | `numcdtextures` / `cdtextureindex` | `i32[numcdtextures]`, each an offset **relative to the studiohdr** to a C string |
| 220 / 224 / 228 | `numskinref` / `numskinfamilies` / `skinindex` | `i16` table |
| 232 / 236 | `numbodyparts` / `bodypartindex` | `mstudiobodyparts_t` (16) |
| 240 / 244 | `numlocalattachments` / `localattachmentindex` | `mstudioattachment_t` (92) |
| 248 / 252 / 256 | `numlocalnodes`, `localnodeindex`, `localnodenameindex` | |
| 260..280 | 6 x i32 deprecated flex fields | |
| 284 / 288 | `numikchains` / `ikchainindex` | |
| 292 / 296 | deprecated mouths | |
| 300 / 304 | `numlocalposeparameters` / `localposeparamindex` | |
| 308 | `surfacepropindex` | string |
| 312 / 316 | `keyvalueindex` / `keyvaluesize` | |
| 320 / 324 | `numlocalikautoplaylocks` / `localikautoplaylockindex` | |
| 328 / 332 | f32 `mass` / i32 `contents` | |
| 336 / 340 | `numincludemodels` / `includemodelindex` | not followed |
| 344 .. 372 | `virtualModel`, `szanimblocknameindex`, `numanimblocks`, `animblockindex`, `animblockModel`, `bonetablebynameindex`, `pVertexBase`, `pIndexBase` | ANI blocks never read |
| 376..379 | u8 `constdirectionallightdot`, `rootLOD`, `numAllowedRootLODs`, `unused` | |
| 380 | f32 `fadeDistance` | |
| 384 / 388 | deprecated flex-ui | |
| 392 | f32 `flVertAnimFixedPointScale` | |
| 396 | i32 `surfacepropLookup` | runtime |
| 400 | i32 `studiohdr2index` | offset to `studiohdr2_t` (0 = none) |
| 404 | i32 `sourceFilenameOffset` | |
`studiohdr2_t` (0xD0 = 208; at `studiohdr2index`; offsets inside it are relative to **it**): 0 `numsrcbonetransform`, 4 `srcbonetransformindex`, 8 `illumpositionattachmentindex`, 12 f32 `flMaxEyeDeflection`, 16 `linearboneindex`, 20 `sznameindex` (the model name), 24/28 bone flex driver count/index, 32 `m_nPerTriAABBIndex`, 36/40/44 node/leaf/vert counts, 48 `unkStringOffset` ("" or "Titan"), 52 `reserved[39]`.

### v53 studiohdr (`r2::studiohdr_t`, 0x2CC = 716 bytes)
Same first fields but `sznameindex` is at 12 and everything up to `length` shifts by 4. Offsets: 0 `id`, 4 `version` (53), 8 `checksum`, 12 `sznameindex` (relative to studiohdr), 16 char[64] `name`, 80 `length`, 84 `eyeposition`, 96 `illumposition`, 108 `hull_min`, 120 `hull_max`, 132 `view_bbmin`, 144 `view_bbmax`, 156 `flags`, 160 `numbones`, 164 `boneindex`, 168/172 bone controllers, 176 `numhitboxsets`, 180 `hitboxsetindex`, 184/188 `numlocalanim`/`localanimindex`, 192 `numlocalseq`, 196 `localseqindex`, 200/204 activity/events, 208 `numtextures`, 212 `textureindex`, 216 `numcdtextures`, 220 `cdtextureindex`, 224 `numskinref`, 228 `numskinfamilies`, 232 `skinindex`, 236 `numbodyparts`, 240 `bodypartindex`, 244 `numlocalattachments`, 248 `localattachmentindex`, 252 `numlocalnodes`, 256 `localnodeindex`, 260 `localnodenameindex`, 264 `localNodeUnk`, 268..284 deprecated flex (5 x i32), 288 `numikchains`, 292 `ikchainindex`, 296 `uiPanelCount`, 300 `uiPanelOffset`, 304 `numlocalposeparameters`, 308 `localposeparamindex`, 312 `surfacepropindex`, 316 `keyvalueindex`, 320 `keyvaluesize`, 324/328 ik autoplay locks, 332 f32 `mass`, 336 `contents`, 340/344 include models, 348 `virtualModel`, 352 `bonetablebynameindex`, 356..359 u8 x4, 360 `fadeDistance`, 364/368 deprecated flex-ui, 372 `flVertAnimFixedPointScale`, 376 `surfacepropLookup`, 380 `sourceFilenameOffset`, 384 `numsrcbonetransform`, 388 `srcbonetransformindex`, 392 `illumpositionattachmentindex`, 396 `linearboneindex` (relative to the studiohdr), 400/404 bone flex driver, 408 `m_nPerTriAABBIndex`, 412/416/420 per-tri AABB counts, 424 `unkStringOffset`, **428 `vtxOffset`, 432 `vvdOffset`, 436 `vvcOffset`, 440 `phyOffset`, 444 `vtxSize`, 448 `vvdSize`, 452 `vvcSize`, 456 `phySize`**, 460 `collisionOffset`, 464 `staticCollisionCount`, 468 `boneFollowerCount`, 472 `boneFollowerOffset`, 476 `unused1[60]`.
A blob exists when its size is > 0 (`pVTX()` etc., `studio_r2.h:1247`); rsx clamps negative offsets to 0 only in `ModelParsedData_t` (`FIX_FILE_OFFSET`), not in `StudioLooseData_t`. The r2 `ModelParsedData_t` constructor initialises `vvcOffset` from `vvdOffset` (copy-paste bug, `modeldata.cpp:540`); `StudioLooseData_t` reads the header correctly, so the bug has no effect on export.

`flags` bits that matter here (`studio.h:1023-1103`): 0x10 `STATIC_PROP` (no bones/transform; only SMD/QC flip coordinates), 0x1000000 `USES_VERTEX_COLOR` (-> `VERT_COLOR`), 0x2000000 `USES_UV2` (-> second texcoord layer), 0x4000 is `USES_EXTRA_BONE_WEIGHTS` only for Apex (unused on r1/r2: "unused" in `studio_r2.h:1008`).

### Bones
- **v52 `r1::mstudiobone_t`, 0xD8 = 216:** 0 `sznameindex` (relative to the bone), 4 `parent` (-1 = root), 8 `bonecontroller[6]`, 32 Vector `pos`, 44 Quaternion `quat`, 60 RadianEuler `rot`, 72 Vector `posscale`, 84 Vector `rotscale`, 96 matrix3x4 `poseToBone`, 144 Quaternion `qAlignment`, 160 `flags`, 164 `proctype`, 168 `procindex`, 172 `physicsbone`, 176 `surfacepropidx`, 180 `contents`, 184 `surfacepropLookup`, 188 Vector `scale`, 200 Vector `scalescale`, 212 `unused`.
- **v53 `r2::mstudiobone_t`, 0xF4 = 244:** 0 `sznameindex`, 4 `parent`, 8 `bonecontroller[6]`, 32 `pos`, 44 `quat`, 60 `rot`, 72 `scale`, 84 `posscale`, 96 `rotscale`, 108 `scalescale`, 120 `poseToBone`, 168 `qAlignment`, 184 `flags`, 188 `proctype`, 192 `procindex`, 196 `physicsbone`, 200 `surfacepropidx`, 204 `contents`, 208 `surfacepropLookup`, 212 u16 `collisionIndex` (0xFFFF = none), 214 u16 `collisionCount`, 216 `unused[7]`.
- **Linear bone** (`mstudiolinearbone_t`, 0x40, same in r1/r2): 0 `numbones`, then nine array offsets (relative to this struct): 4 `flagsindex` (i32[]), 8 `parentindex` (i32[]), 12 `posindex` (Vector[]), 16 `quatindex` (Quaternion[]), 20 `rotindex` (RadianEuler[]), 24 `posetoboneindex` (matrix3x4[]), 28 `posscaleindex` (Vector[]), 32 `rotscaleindex` (Vector[]), 36 `qalignmentindex` (Quaternion[]), 40 `unused[6]`. Located at `studiohdr2 + hdr2.linearboneindex` (v52, `studio_r1.h:1162`) or `studiohdr + linearboneindex` (v53); optional (0 = none). rsx only uses it in animation decoding (`CalcBone*`).
- Constants: `BONE_FIXED_ALIGNMENT 0x100000` (bone flag, alignment quaternion applied, `studio_r2.cpp:88`), `BONE_USED_BY_*` masks in `studio.h:736-761`, procedural types `STUDIO_PROC_*` 1..7.

### Bodyparts, models, meshes, textures, skins (identical in v52 and v53)
- `mstudiobodyparts_t` (16): 0 `sznameindex` (relative to the bodypart), 4 `nummodels`, 8 `base`, 12 `modelindex` (relative to the bodypart).
- `mstudiomodel_t` (0x94 = 148): 0 char[64] `name`, 64 `type`, 68 f32 `boundingradius`, 72 `nummeshes`, 76 `meshindex` (relative to the model), 80 `numvertices`, **84 `vertexindex`** (byte offset of this model's first vertex in the VVD vertex block: `/48` gives the vertex index), 88 `tangentsindex`, 92 `numattachments`, 96 `attachmentindex`, 100/104 deprecated eyeballs, 108 `pad[4]`, **124 `colorindex`**, **128 `uv2index`**, 132 `unused[4]`.
- `mstudiomesh_t` (0x74 = 116): 0 `material` (index into the texture table, **not** the skin-mapped index), 4 `modelindex` (negative offset back to the model), 8 `numvertices`, **12 `vertexoffset`** (mesh vertex start relative to the model's `vertexindex/48`), 16..28 deprecated flex/material-type fields, 32 `meshid`, 36 Vector `center`, **48 `vertexloddata`** = `{ i32 pad; i32 numLODVertexes[8] }` (pad at 48, counts at 52..83), then 84 (v52: `unused[8]`; v53: 8-byte pointer `unk_54` + `unused[6]`, 84..115). `numLODVertexes[lod]` is the vertex count of this mesh in LOD `lod`; rsx treats it as stored in the file (the field is "deprecated starting with rmdl v14", but r1/r2 fill it).
- `mstudiotexture_t`: v52 0x40: 0 `sznameindex` (relative to the entry; the material name, no extension), 4 `unused_flags`, 8 `used`, 12 `unused1`, 16/20 runtime pointers, 24 `unused[10]`. v53 0x2C: 0 `sznameindex`, 4 `unused_flags`, 8 `used`, 12 `unused[8]`. There is **no guid**: materials are resolved by name (Dependencies).
- **Skins:** `skinindex` -> `i16 skinref[numskinfamilies][numtextures]` (row stride is `numtextures`, not `numskinref`, `modeldata.h:959`); family 0 is the default skin and is the only one rsx exports by default; names are not read (`FixupSkinData` is commented out, so rsx would name them `default` / `skin_<n>`).
- **cdtextures:** string search directories (offsets as above); parsed (`ParseSourceModelTextureData`) but unused.
- Hitboxes: `mstudiohitboxset_t` {0 `sznameindex`, 4 `numhitboxes`, 8 `hitboxindex`} (12). v52 `mstudiobbox_t` (68): 0 `bone`, 4 `group`, 8 `bbmin`, 20 `bbmax`, 32 `szhitboxnameindex`, 36 `unused[8]`. v53 `mstudiobbox_t` (68): same up to 32, then 36 `forceCritPoint`, 40 `hitdataGroupOffset`, 44 `unused[6]`. Attachments (`mstudioattachment_t`, 92): 0 `sznameindex`, 4 `flags`, 8 `localbone`, 12 matrix3x4 `local`, 60 `unused[8]`. Parsed into `ModelParsedData_t` but not exported for source models (no `ExportModelHitboxes` path in `sourcemodel.cpp`).

### VTX (`.dx11.vtx` or embedded) `OptimizedModel`, `pack(1)`, version 7 (`OPTIMIZED_MODEL_FILE_VERSION`)
| struct | size | fields |
|---|---|---|
| `FileHeader_t` | 0x24 | 0 `version` (7), 4 `vertCacheSize` (i32), 8 `maxBonesPerStrip` u16, 10 `maxBonesPerFace` u16, 12 `maxBonesPerVert`, 16 `checkSum`, 20 `numLODs`, 24 `materialReplacementListOffset`, 28 `numBodyParts`, 32 `bodyPartOffset` |
| `BodyPartHeader_t` | 8 | `numModels`, `modelOffset` (relative to this struct) |
| `ModelHeader_t` | 8 | `numLODs`, `lodOffset` |
| `ModelLODHeader_t` | 12 | `numMeshes`, `meshOffset`, f32 `switchPoint` |
| `MeshHeader_t` | 9 | `numStripGroups`, `stripGroupHeaderOffset`, u8 `flags` (never read) |
| `StripGroupHeader_t` | 0x21 | `numVerts`, `vertOffset`, `numIndices`, `indexOffset`, `numStrips`, `stripOffset`, u8 `flags` (`0x02` HW skinned, `0x04` delta flexed, `0x08` suppress HW morph), `numTopologyIndices`, `topologyOffset` |
| `StripHeader_t` | 0x23 | `numIndices`, `indexOffset`, `numVerts`, `vertOffset`, i16 `numBones`, u8 `flags` (`1` trilist, `2` quad reg, `4` quad extra), `numBoneStateChanges`, `boneStateChangeOffset`, `numTopologyIndices`, `topologyOffset` |
| `Vertex_t` | 9 | u8 `boneWeightIndex[3]`, u8 `numBones`, u16 `origMeshVertID`, u8 `boneID[3]` |
| `BoneStateChangeHeader_t` | 8 | `hardwareID`, `newBoneID` |
All `*Offset` fields are **relative to the struct that holds them** (`optimize.h`). Index arrays are `u16`. `MaterialReplacement*` and topology data are never read.

### VVD (`.vvd` or embedded) vertex file, version 4, magic `IDSV` (`49 44 53 56`)
`vertexFileHeader_t` (64): 0 `id`, 4 `version` (4), 8 `checksum`, 12 `numLODs`, 16 `numLODVertexes[8]`, 48 `numFixups`, 52 `fixupTableStart`, 56 `vertexDataStart`, 60 `tangentDataStart` (offsets from the start of the file).
`vertexFileFixup_t` (12): `lod`, `sourceVertexID`, `numVertexes`. `mstudiovertex_t` (48): 0 `mstudioboneweight_t` (16 bytes: f32 `weight[3]` at 0, u8 `bone[3]` at 12, i8 `numbones` at 15), 16 Vector `position`, 28 Vector `normal`, 40 Vector2D `texcoord`. Tangents: `Vector4D[]` (16 each, xyz + w sign) at `tangentDataStart`.

### VVC (`.vvc`, v53 only in practice) magic `IDCV`, version 1
`vertexColorFileHeader_t` (56): 0 `id`, 4 `version`, 8 `checksum`, 12 `numLODs`, 16 `numLODVertexes[8]`, 48 `colorDataStart`, 52 `uv2DataStart`. Colour block = `Color32[]` (r, g, b, a bytes, 4 each), UV2 block = `Vector2D[]` (8 each), both indexed like VVD vertices.

### VVW (`.vvw`)
Header (44): `checksum`, `version` (1), `numLODVertexes[8]`, `weightDataStart`; entries `{ i16 weight; i16 bone }`. Apex only; **not used for v52/v53** (`ParseVertexFromVTX` basic-Source overload takes no VVW).

### PHY (`.phy`) Valve `vphysics` collision
`ivps::phyheader_t` (16): `size` (header size, 16), `id` (0 = Valve; 1 = Apex "irps"), `solidCount`, `checkSum` (checksum of the `.mdl`; `VerifyFileIntegrity` does **not** compare it, comment "hard to check phys"). Followed by `solidCount` compact surfaces (`compactsurfaceheader_t`: `size`, `vphysicsID` ("VPHY"), i16 `version`, i16 `modelType`, `surfaceSize`, Vector `dragAxisAreas`, `axisMapSize`; legacy surface + ledge tree) and a trailing key/value text block. rsx loads the bytes but **never decodes them for source models** (`SetExtraData(.., SRCMDL_PHYS)` only). Details in `phyfile.h` if a slice wants STL collision.

## Data outside the header
- **v52:** the four sibling blobs described under "Where it lives"; all uncompressed, no starpak. Everything else (strings, bones, ...) is inside the `.mdl`.
- **v53:** the same blobs embedded; offsets/sizes from the header. No compression anywhere.
- **Animation data (v53):** inside the `.mdl`, addressed from each `mstudioanimdesc` (below). The v52 `.ani` blocks (`szanimblocknameindex`) are not read; `StudioLooseData_t` has an `.ani` hook (`aniname`) that no caller passes. IDAG (`IDSTUDIOANIMGROUPHEADER`) is defined but unused for these versions.
- **Memory/limits:** rsx assumes at most `s_MaxStudioVerts = 65536` vertices and `65536 * 3` indices per mesh (fixed parse buffers, `studio.h:14-16`), and `Vector positions[256]` bones in `ParseAnimation` (r2, `animdata.cpp:939`).

## Dependencies
- **Materials (matl):** from `mstudiotexture_t` names. For each texture rsx forms `material/<name>_skn.rpak` and `material/<name>_fix.rpak`, hashes each with `RTech::StringToGuid`, and looks up a loaded `CPakAsset` by that name (`ParseSourceModelTextureData`, `sourcemodel.cpp:69-92`): `_skn` first, then `_fix`; if neither exists, `guid = 0` and no asset. The `cdtextures` directories are **not** used in the name. Notable bug: in the `_fix` branch `pMaterial->asset = sknAsset` (null) instead of `fixAsset`, so the guid is set but the asset pointer is null. The material name stored in the Cast `matl` node is the part of the name after the last `/` (via the matl asset name) or the **decimal guid** `0` if not found (mesh name `<model>_0`). In respak this is a name-to-guid link through `rpak.guid` (project.md §5.5): link to `matl` by `string_to_guid("material/<name>_skn.rpak")`, falling back to `_fix`; an unresolved material becomes a stub Cast material with the texture name.
- **Sequences -> model:** a `seq` needs its parent `mdl` (the rig: bones, pose, scales). Resolved by guid `RigGUID = StringToGuid(model internal name)`; in `PostLoadSourceSequenceAsset` (:436) the model is looked up in the asset list.
- **Include models** (`includemodelindex`), external ANI blocks, other sequence groups: not followed.

## Decoding
Pseudocode for the model (r1 and r2 share it; differences are marked). `hdr` = studiohdr; `rd(off, n)` a bounded read.
```
// 0. open
bytes = file; hdr = bytes
require hdr.id == 'IDST'; v = hdr.version in {52, 53}; require len(bytes) >= hdr.length
if v == 52: files = siblings found by name rule, hasIDCV = false
if v == 53: files = embedded (vtx/vvd/vvc/phy at header offsets; size>0 means present)
require vtx and vvd both present (rsx: if only one -> "not all required vertex files are present" and no geometry)
verify: vtx.version == 7 && vtx.checkSum == hdr.checksum
        vvd.id == 'IDSV' && vvd.version == 4 && vvd.checksum == hdr.checksum
        vvc (if used): id 'IDCV', version 1, checksum equal
// 1. static tables
bones[i]  = mstudiobone (stride 216 / 244); keep name, parent, flags, pos, quat, rot, scale, poseToBone, proctype, physicsbone, surfaceprop, contents
materials[i] = name(texture[i]) ; skin[f][i] = i16 skinref[f*numtextures + i]
bodyparts[b].models[m] = mstudiomodel; model.meshes[k] = mstudiomesh
// 2. LODs
numLODs = vtx.numLODs
if numLODs == 0 || numbodyparts == 0: no geometry (skeleton only)   // rsx asserts; note it reads vtx.numLODs BEFORE checking vtx != null
for lod in 0..numLODs:
    for bp in bodyparts, for m in bp.models:
        vtxModel = vtx.body[bp].model[m]; vtxLod = vtxModel.lod[lod]; switchPoint = vtxLod.switchPoint
        for k in 0..model.nummeshes:
            vtxMesh = vtxLod.mesh[k]
            if vtxMesh.numStripGroups == 0: drop this mesh from this model/LOD; continue
            // vertices of this mesh in this LOD
            base  = model.vertexindex / 48 + mesh.vertexoffset
            count = mesh.vertexloddata.numLODVertexes[lod]
            (verts, tangs) = vvd.PerLODVertexBuffer(lod, base, base + count)      // see below
            (colors, uv2)  = vvc.PerLODVertexBuffer(lod, vvd.fixups, base, base+count) if vvc used
            sg = vtxMesh.stripGroup[0]            // rsx asserts exactly 1 strip group ("not a single model from respak has two")
            hw = sg.flags & 0x02
            for strip in sg.strips:
                states = strip.boneStateChange[..]   // null when numBoneStateChanges == 0
                for i in 0..strip.numVerts:
                    pv   = sg.vertex[strip.vertOffset + i]
                    o    = pv.origMeshVertID                      // relative to `base`
                    vert = { pos = verts[o].position, normal = verts[o].normal, tangent = tangs[o],
                             uv0 = verts[o].texcoord, color = colors ? colors[o] : (255,255,255,255), uv1 = uv2[o] (only if flags & 0x2000000) }
                    for j in 0..verts[o].numbones:
                        w = verts[o].weight[ pv.boneWeightIndex[j] ]
                        b = (hw && states) ? states[ pv.boneID[j] ].newBoneID : pv.boneID[j]
                        weights.push(b, w)
                    out.vertex[strip.vertOffset + i] = vert
                copy sg.indices[strip.indexOffset .. +strip.numIndices] to out.indices at the same position
            mesh.vertCount += sg.numVerts; mesh.indexCount += sg.numIndices     // counts of the single strip group
            mesh.material = mesh.material  // index into the texture table; skin remap done at export
```
**`vvd.PerLODVertexBuffer(lod, minV, maxV)`** (`studio.h:143`): `maxV = min(maxV, numLODVertexes[0])`, `minV = max(minV, 0)`. If `numFixups == 0` copy vertices and tangents `[minV, maxV)` straight from the blocks. Otherwise walk the fixup table in order keeping `cur`: for each fixup, if `fixup.lod < lod` **or** `cur < minV` then `cur += fixup.numVertexes` and skip; if `cur >= maxV` stop; else `cur += fixup.numVertexes` and append `fixup.numVertexes` vertices (and tangents) starting at `fixup.sourceVertexID` (an index into the whole vertex block). This is rsx's own approximation of Valve's fixup logic (its comment says Titanfall fixups are sequential per mesh); the `cur` counter also advances for fixups that are skipped because `fixup.lod < lod`, and the result is only meaningful if `maxV - minV` matches `numLODVertexes[lod]`. **Risk:** unverified against real data; compare with rsx output. The VVC variant is the same loop over colour and UV2 arrays (`studio.h:214`).

Notes:
- The texcoord layout flags rsx assigns for the shared mesh/export code (`ModelMeshData_t::ParseMesh_VTX`, `modeldata.h:148`): `rawVertexLayoutFlags = VERT_LEGACY (0x1 pos | 0x100 normal | 0x800 tangent4 | 0x1000 blend idx | 0x2000 blend weights | texcoord0 fmt 2) | (flags & 0x1000000 ? VERT_COLOR 0x10 : 0) | (flags & 0x2000000 ? texcoord slot 2 fmt 2 : 0)`. `texcoordCount` = number of non-zero 4-bit nibbles from bit 24: 1, or 2 with UV2. In respak this is just "uv layers: 1 or 2".
- Normals: VVD normals are f32 Vectors; rsx packs them into its internal `Normal32` and unpacks again for Cast (lossy, `PackNormal` approximate). Respak should keep the f32 normals.
- Weights per vertex: 1 to 3 (`MAX_NUM_BONES_PER_VERT 3`), `numbones == 0` gives no weights for that vertex (Cast writer then writes bone 0 weight 0.0, see rmdl). Bone ids are u8 in VVD/VTX so `numbones` stays <= 256.
- Triangles: `StripHeader.flags & 1` is a plain triangle list; rsx **ignores flags and the quad flags (0x2/0x4)** and copies indices as a trilist. The index values are absolute within the strip group's vertex table (rsx writes `parseVertices[strip.vertOffset + i]` and copies indices with no offset fix-up).
- `VerifyFileIntegrity` failure -> the model is loaded without geometry (skeleton only).

**Sequences (v53 only).** `LoadSourceSequenceAsset` -> `ModelSeq_t(seqdesc)`; `PostLoad` -> `ParseSequence(seq, rig, hdr)` -> `ParseAnimation` per blend. Structures:
- `r2::mstudioseqdesc_t` (0xE8 = 232): 0 `baseptr` (i32, **negative** offset from this struct back to the studiohdr: `studiohdr = this + baseptr`), 4 `szlabelindex`, 8 `szactivitynameindex`, 12 `flags`, 16 `activity`, 20 `actweight`, 24 `numevents`, 28 `eventindex`, 32 Vector `bbmin`, 44 Vector `bbmax`, 56 `numblends`, 60 `animindexindex` (-> `i16[groupsize[0]*groupsize[1]]` = indices into `localanimindex`), 64 `movementindex`, 68 `groupsize[2]`, 76 `paramindex[2]`, 84 `paramstart[2]` (f32), 92 `paramend[2]` (f32), 100 `paramparent`, 104 f32 `fadeintime`, 108 f32 `fadeouttime`, 112 `localentrynode`, 116 `localexitnode`, 120 `nodeflags`, 124 f32 `entryphase`, 128 f32 `exitphase`, 132 f32 `lastframe`, 136 `nextseq`, 140 `pose`, 144 `numikrules`, 148 `numautolayers`, 152 `autolayerindex`, 156 `weightlistindex` (f32[numbones]), 160 `posekeyindex`, 164 `numiklocks`, 168 `iklockindex`, 172 `keyvalueindex`, 176 `keyvaluesize`, 180 `cycleposeindex`, 184 `activitymodifierindex`, 188 `numactivitymodifiers`, 192 `ikResetMask`, 196 `unk_C4`, 200 `unused[8]`.
  Sub-structs: `mstudioevent_t` (80: f32 `cycle`, `event`, `type`, char[64] `options`, `szeventindex`); `mstudioautolayer_t` (24: i16 `iSequence`, i16 `iPose`, `flags`, f32 `start`, `peak`, `tail`, `end`); `mstudioiklock_t` (32); `mstudioactivitymodifier_t` (8: `sznameindex`, bool `negate`).
- `r2::mstudioanimdesc_t` (0x5C = 92): 0 `baseptr` (to the studiohdr), 4 `sznameindex` (a leading `@` marks a sequence-declared anim: the real name is `name+1`), 8 f32 `fps`, 12 `flags`, 16 `numframes`, 20 `nummovements`, 24 `movementindex` (`mstudiomovement_t`, 44 bytes: `endframe`, `motionflags`, f32 `v0`, `v1`, `angle`, Vector `vector`, Vector `position`), 28 `framemovementindex`, 32 `animindex`, 36 `numikrules`, 40 `ikruleindex`, 44 `numlocalhierarchy`, 48 `localhierarchyindex`, 52 `sectionindex`, 56 `sectionframes`, 60 `unused[8]`. Flags used: `0x1` looping, `0x4` delta, `0x40000` `FRAMEMOVEMENT`. rsx forces `0x20000` (`ANIM_VALID`) on every r2 animation. `sectionindex` -> `i32[]` of per-section anim data offsets (relative to the animdesc).
- **Data lookup** (`ModelAnim_t::pAnimdataNoStall`, `animdata.cpp:264`), frame `f` (0-based), `n = numframes`, `sf = sectionframes`:
  ```
  index = animindex; section = 0
  if sf != 0:
      if n > sf && f == n - 1: f = 0; section = (n - 1) / sf + 1       // last frame of long anims is stored separately
      else: section = f / sf; f -= section * sf
      index = sections[section].animindex                              // i32 per section, relative to the animdesc
  data = (char*)animdesc + index            // start of the first bone header for frame block
  ```
  Section count when not given: `(n - 1) / sf + 2`.
- **Bone track record** `r2::mstudio_rle_anim_t` (fixed layout, 32 bytes per record, `studio_r2.h:555`): 0 f32 `posscale`, 4 u8 `bone`, 5 u8 `flags`, 6..7 pad, then data starting at `pData = this + 8`: record offset **8** rotation slot (8 bytes: `Quaternion64` raw if `RAWROT`, else `mstudioanim_valueptr_t` {i16 offset[3]}), **16** position slot (6 bytes: `Vector48` raw if `RAWPOS`, else valueptr), **22** scale slot (6 bytes: `Vector48` if `RAWSCALE`, else valueptr), **28** `i32 nextOffset` (relative to the record start; 0 = last). A record is 32 bytes; valueptr offsets are relative to the valueptr itself. Flags (`RleFlags_t`): `0x01` DELTA, `0x02` RAWPOS, `0x04` RAWROT, `0x08` RAWSCALE, `0x10` NOROT (no static and no tracked rotation). Bone records are sorted by bone; ids can skip and `0xFF` blocks exist ("handle like the game"); to find bone `b`, rsx walks `next` while `record.bone < b`.
- **Value pointers** (`mstudioanim_valueptr_t.offset[i]`, relative to the valueptr; `<= 0` = no track -> value 0): point to `mstudioanimvalue_t` runs: a header u8 `valid`, u8 `total` (overlaying an i16 `value`), followed by `valid` i16 values. `ExtractAnimValue(frame, run, scale, v1, v2)` (`studio_r1.cpp:7`):
  ```
  if run == null: 0
  if run.total == 1 && run.valid == 1: v = run[1].value * scale; return        // constant
  k = frame
  while run.total <= k: k -= run.total; run += run.valid + 1; if run.total == 0: return 0   // end of data
  if run.valid > k: v1 = run[k+1].value*scale; v2 = (run.valid > k+1) ? run[k+2].value*scale
                                                  : (run.total > k+1) ? v1 : run[run.valid+2].value*scale
  else: v1 = run[run.valid].value*scale; v2 = (run.total > k+1) ? v1 : run[run.valid+2].value*scale
  ```
  (the single-value overload is the same without `v2`).
- **Per-frame bone values** (`CalcBonePosition/Quaternion/Scale`, `studio_r2.cpp`): `cycle = frame/(numframes-1)` (0 if one frame), `fFrame = cycle*(numframes-1)`, `iFrame = int(fFrame)`, `s = fFrame - iFrame`.
  - Position: `RAWPOS` -> `Vector48`. Else `v = ExtractAnimValue(iFrame, valueptr[j], posscale)` per axis (blend `v1*(1-s)+v2*s` when `s > 0.001`); if not `DELTA`, `+ bone.pos` (from the linear bone block if present).
  - Rotation: `RAWROT` -> `Quaternion64`; `NOROT` -> `delta ? (0,0,0,1) : bone.quat`. Else three euler tracks scaled by `bone.rotscale`, `+ bone.rot` if not delta, converted with `AngleQuaternion`; if `s > 0.001` and the two samples differ, `QuaternionBlend(q1, q2, s)`; finally, if not delta and `bone.flags & 0x100000`, `QuaternionAlign(bone.qAlignment, q, q)`.
  - Scale: as position with `bone.scalescale`, `+ bone.scale` if not delta; `RAWSCALE` -> `Vector48`.
  - `Quaternion64` (8 bytes): bitfield LSB-first `x` 21 bits, `y` 21, `z` 21, `wneg` 1. `x = (raw - 1048576) / 1048576.5` (same for y, z), `w = sqrt(1 - x^2 - y^2 - z^2)` (rsx uses `FastSqrtFast`; use an exact sqrt), negated when `wneg`. `Vector48`: 3 x half-float (`float16`; the conversion is assumed to be IEEE binary16, `float16.h` not read).
- **Bones not in the animation** keep their default: `bone.pos/quat/scale`, or (0,0,0)/(0,0,0,1)/(1,1,1) for `DELTA`. Flags written for Cast: POS always set for r2 (`animdata.cpp:1005`), ROT unless `NOROT`, SCALE if `seqdesc.flags & 0x20000`.
- **Origin bone (bone 0) post-processing** (`ParseAnimDesc_Origin`): for every frame, if the animdesc has frame movement (`framemovementindex` and `0x40000`) or piecewise movements, add `Studio_AnimPosition(animdesc, cycle)` (position) to bone 0's position and the yaw to its angle; then `yaw -= 90`, `pos = (pos.y, -pos.x, pos.z)`, and rebuild bone 0's quaternion from the (euler of the) rotation. Frame movement (`mstudioframemovement_t`: f32 `scale[4]`, i16 `offset[4]`, offsets relative to the struct, valueptr semantics, 4 tracks x/y/z/yaw) uses the single-value `ExtractAnimValue` and, between frames, a lerp with `AngleDiff` for yaw (`animdata.cpp:1408-1503`). The piecewise path integrates `v0..v1` (`d = v0*f + 0.5*(v1-v0)*f*f`) over `mstudiomovement_t` blocks. Both fall back to zero if the cycle is outside.

## Output
**rsx (model)** `ExportSourceModelAsset` (`sourcemodel.cpp:309`): writes under `<export>/models/<model stem>/` (or the model's own folder with the global `exportPathsFull`). Format from the shared model export setting: Cast (default), RMAX, SMD (+QC; the QC is written only for SMD, `ExportModelMeshes` :3231). `version` argument is `54` for the QC writer. For Cast/RMAX each **LOD is its own file** (`<stem>_LOD<n>.cast`, `lodAsModel = true`); SMD writes one file per (LOD, model) with the model name. No `.phy`, no hitbox export, no raw dump. Sequences of the model (v53) are exported first if the global `exportRigSequences` is set (default unknown). The Cast tree is the same as rmdl: `root -> modl -> skel (bones: n, p, lp, lr) -> matl nodes -> mesh nodes (vp, vn, vc, u0.., wb, wv, f (winding reversed i2,i1,i0), ul, mi, m)`; see [rmdl](rmdl.md) for the byte layout of Cast nodes and properties. Differences for source models: weights from VVD (<= 3 per vertex), no bone-state remap table for r2 (strips carry their own state changes), colours present only with `USES_VERTEX_COLOR` and v53, UVs written as stored.
**rsx (seq)** `ExportSourceSequenceAsset` (:485): the export setting is the **aseq** binding's (`ANIMSEQ_CAST`, `RMAX`, `SMD`, or `RSEQ` which is a no-op returning true because "the raw data should be stored in the model file"). Folder `<export>/models/<model stem>/`, one file per blend: `<label>_<blendIdx>.cast` (`ExportSeqDescCast` :3409): `root -> anim (guid = seq guid; props: framerate f32 = fps, looping u8 = flags&1) -> skel (bone nodes) + curve nodes` per bone for position (3 axes), rotation (quaternion) and, if `seqdesc.flags & 0x20000`, scale; curve mode additive when `DELTA` else absolute; `animWeight = seqdesc.weight(bone)`. Bones with no data get a one-key curve of the default pose. Events, IK rules/locks, autolayers, pose parameters and blend grids are parsed into `ModelSeq_t` but **not exported**.

**Proposed respak:**
- New format `source_mdl` (kinds `mdl`, `seq`) with a read edge from a **loose file** (a `Blob`/path value, not `rpak.asset`): `file (.mdl) -> source_mdl`. For v52 the handler must read siblings through the same OpenDAL operator/directory as the `.mdl` (a `Volume` sibling lookup; project.md §6 file access). It exposes `Metadata` (name, guid, dependencies = material guids), `Skeleton`, `Mesh` (per LOD), and for v53 an `Animation` per `(sequence, blend)`.
- `source_mdl -> cast` reuses the rmdl Cast encoder (Cast model; the slice for [rmdl](rmdl.md) defines `Mesh`/`Skeleton`). Options: `{ "lods": "0" | "all" | [..], "skin": 0, "textures": true }`; proposed default **LOD 0, one `.cast`**. Sequence conversion: `seq -> cast` (animation Cast) reusing the `Animation` capability introduced by [aseq](aseq.md).
- Virtual entries for a `.mdl` container view: `<model name>.mdl` (mesh+skeleton) and `<seq label>.seq` (v53). Names as above; the `seq` entries have no bytes of their own.

## rsx quirks
- **`.seq` is not a file** and exists only for v53; v52 has no sequence support (`case 52: break`).
- **`ParseModelBoneData` ignores its template parameter** (`modeldata.h:1123-1140`): it always casts to `r5::mstudiobone_v8_t` (184-byte stride, different field positions) while being instantiated for `r1::mstudiobone_t` (216) / `r2::mstudiobone_t` (244). If this is really the code that runs, bones past index 0 are misread for r2 and nearly all fields are wrong for r1 (r2 only shares `sznameindex/parent/pos/quat/rot/scale`). **Not verified at runtime**; check with a real file against rsx's own Cast skeleton. Respak must use the r1/r2 structs from this spec.
- `ParseModelVertexData_VTX` reads `pVTX->numLODs` **before** the `!pVTX || !pVVD` null check (crash when the `.dx11.vtx` is missing). Respak: check first, report `MissingDependency`.
- v52 `.vvc` is dropped unconditionally (`hasIDCV = false`), so TF1 vertex colours / UV2 never appear in rsx output. Open question whether TF1 ever uses `.vvc` (only `ogre_titan.mdl` is named in the comment).
- Sibling names come from the internal model name's last component, not the on-disk name; renamed files are not found.
- `ParseSourceModelTextureData`: `_fix` branch stores `sknAsset` (null) instead of `fixAsset`; material lookups ignore `cdtextures`; guid 0 + "decimal guid" in the Cast mesh name when missing. Keep the lookup rule, fix the pointer, dedupe `matl` nodes by guid (also a rmdl quirk).
- Only the first strip group of each VTX mesh is read (`assertm(numStripGroups == 1)`); strip `flags` (quad lists) and bone `numBones` are ignored. Valid for retail files per the comment; respak should error on `numStripGroups > 1` rather than silently drop geometry.
- Strip bone-state remap indexes the strip's `BoneStateChange` array by `boneID` (`pBoneStates[boneID].newBoneID`), not by searching `hardwareID == boneID`. Keep only if confirmed.
- The VVD per-LOD fixup walk counts `cur` for skipped LODs too (see Decoding). Source's own logic skips them without counting.
- `ParseAnimation` (r2) uses `Vector positions[256]` / `Quaternion quats[256]` / `Vector scales[256]` on the stack: more than 256 bones overflow (r2 header comment: max bones 256). Respak: size by `numbones`.
- `Studio_AnimPosition` / `ParseAnimDesc_Origin` apply the Source-to-Cast origin rotation (`yaw - 90`, `(x,y) -> (y,-x)`) only to bone 0: the root bone is assumed to be the origin bone. Keep (output parity) but document.
- `GetStringAfterLastSlash` / `replace_extension` assume the internal model name has a `.mdl`-like or extension-less tail; names with extra dots are mis-split.
- The sequence label is used verbatim in a file name (`<label>.seq`); labels containing path separators or characters invalid on Windows are not sanitised (`mdl.cpp:94`).
- `HandleMDLLoad` passes the whole file through `memcpy` of `length` bytes without checking `length <= file size`; v63 files are registered but not loadable (see Versions).
- Global state: `g_rsxSettings.exportPathsFull`, `exportRigSequences`, `exportModelSkin`, `previewedSkinIndex`, `exportMaterialTextures`, `exportModelMatsTruncated`; the shared aseq export setting. Become target options.
- Normals go through lossy `Normal32` pack/unpack and approximate reciprocal sqrt; weights are not renormalised. Use f32 normals.

## Reuse in respak
- Cast writer and the `Mesh`/`Skeleton`/`Animation` capabilities planned for [rmdl](rmdl.md) and [aseq](aseq.md) (`respak-core/src/capability/{mesh,skeleton,animation}.rs`, `respak-formats/src/cast`). `guid::string_to_guid` for `material/<name>_skn.rpak` links and Cast hashes; `GuidRef` + `identify` for `matl` (as in `matl_json.rs::locate`) and the existing `matl`/`txtr` pipeline for textures.
- Reuse the bounded C-string reader convention from `txtr.rs`. The studiohdr tables for bodypart/model/mesh/skin/bone are conceptually the same as rmdl v8 (`studiohdr_v8_t`), but the r1/r2 byte layouts above are separate.
- New: a loose-file read edge with sibling lookup, `source_mdl.rs` (r1/r2 header, bone, model, mesh, texture, skin parsers, VTX/VVD/VVC walkers, `PerLODVertexBuffer`), r2 sequence/animation decoder (value runs, section lookup, `Quaternion64`/`Vector48`, origin handling), Valve mathlib bits (`AngleQuaternion`, `QuaternionBlend`, `QuaternionAlign`, `AngleDiff`). No `createdTime` or pak header involvement.

## Tests
- **Synthetic v53 model** (smallest end-to-end): header 716 bytes (zero-filled) with `id='IDST'`, `version=53`, `checksum=0x1234`, `length` = total size; 2 bones (244 bytes each; `parent = -1, 0`, non-default `pos`/`quat`); 1 texture (44 bytes, name `models/test/mat`); `numskinfamilies=2, numtextures=1, numskinref=1`, skin table `[0, 0]`; 1 bodypart / 1 model (148) / 1 mesh (116: `material=0, vertexoffset=0, vertexloddata.numLODVertexes[0..1]=4`); embedded VVD (64-byte header + 4 vertices x 48 + 4 tangents x 16, no fixups; one vertex with `numbones=2`), embedded VTX (36 + bodypart 8 + model 8 + LOD 12 + mesh 9 + stripgroup 33 + strip 35 + 4 x 9 vertices + 6 u16 indices = 2 triangles, one LOD). Check: bone decode, vertex count 4, indices 6, material name -> `material/models/test/mat_skn.rpak` guid, checksum mismatch -> no geometry.
- **Synthetic v52:** same with studiohdr 408 bytes, `studiohdr2` (208) holding `sznameindex`, sibling `.dx11.vtx`/`.vvd` files with the internal-name rule (file renamed on disk -> not found -> `MissingDependency`), `.vvc` present but ignored.
- **VVD fixups:** a two-fixup file (LOD 0 of 4 verts, LOD 1 of 2 verts) with `sourceVertexID` out of order; verify `PerLODVertexBuffer` for LOD 0 and 1 against hand-computed buffers, and the "skip-counts-towards-cur" behaviour.
- **Animation:** one sequence, one blend, 3 frames, bone 0 with an all-`RAWPOS` / `RAWROT` record and bone 1 with a value-run track (`valid=2, total=3`, values `[100, 200]`, scale 0.01) -> frame 0/1/2 equals `1.0, 2.0, 2.0`; `Quaternion64` of the identity (`x=y=z=1048576, wneg=0`) -> `(0,0,0,1)`; section table with `sectionframes=2` and `numframes=3` -> last frame uses section `(3-1)/2+1 = 2`, local frame 0; origin-bone rotation/swap (`pos (1,0,0)` -> `(0,-1,0)`).
- **Edge cases:** `numLODs == 0`; mesh with `numStripGroups == 0`; unknown version 63 -> `UnsupportedVersion`; truncated file (`length` > file size -> `Corrupt`); `Vector48` special values (NaN/Inf) if a real file has them; `numbones > 256`.
- **Real files** (no Steam runs without permission): any Titanfall 1 prop/weapon (`.mdl` + `.dx11.vtx` + `.vvd`, one with `.phy`, `ogre_titan.mdl`), a Titanfall 2 animated character or weapon (`.mdl` with embedded blobs and many sequences). Compare vertex/index counts, bone lists and sequence blend counts against rsx's Cast export.

## Open questions
1. Is Titanfall 1 / 2 loose-model support wanted at all? project.md §2 lists a Valve-Source crate as not planned; the backlog only has "Titanfall 2 sequences (`source-mdl`)". If yes: v53 (self-contained) first, v52 (siblings) later?
2. Where does this live (`respak-respawn` next to rmdl, or a new crate), and how are loose files with sibling lookup modelled in the container/volume layer (a "directory" input, project.md §6)?
3. Asset names: virtual `<internal name>.mdl` / `<label>.seq`, or `models/...` as rsx prints? Does the internal name contain `.mdl` and a directory in real files?
4. Output granularity: one `.cast` with LOD 0 (proposed) or one per LOD like rsx; per-sequence-blend animation files (rsx) or one Cast animation per sequence?
5. Is the `ParseModelBoneData` template bug real (see quirks)? Needs a real-file comparison before trusting rsx's r1/r2 skeletons.
6. Does Titanfall 1 ever use `.vvc` (rsx hard-disables it)? Does any real file have `numStripGroups > 1`, or use strip `flags` other than `1`?
7. Is the `vvd` fixup walk (rsx's variant) correct for all real files, or do we reimplement Valve's `CMDLCache` logic? Needs real data.
8. Material links: name only (`_skn`, then `_fix`) like rsx, or also try `cdtextures`/other suffixes? What if `matl` is not in a loaded pak: stub material or error?
9. Skins: only family 0, or export all families as material variants (names are not read by rsx; the file stores them after the skin table in Source)?
10. Collision (`.phy`, VPHY/compact-ledge format) and hitboxes: backlog, or skip? rsx exports neither for source models.
11. Version 63 (Xbox 360): treated as unsupported. Is it needed?
12. `float16` conversion and `AngleQuaternion`/`QuaternionBlend` details were not read; port from Valve's mathlib semantics and verify against rsx output.
