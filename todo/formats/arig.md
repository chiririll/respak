# arig · Respawn animation rig
**rsx sources:** `game/rtech/assets/animrig.h` (`AnimRigAssetHeader_v4_t`, `_v5_t`, `AnimRigAsset`), `game/rtech/assets/animrig.cpp` (`LoadAnimRigAsset` :15, `PostLoadAnimRigAsset` :137, `ExportRawAnimRigAsset` :223, `ExportAnimRigAsset` :242, `InitAnimRigAssetType` :327), `game/rtech/assets/model.cpp` (`GetModelVersionFromAsset` :946, `ParseExternalSequences` :1051), `game/rtech/assets/animseq.h` (`s_AnimRigTimeStamp_V7_V19_2` :90, unused), `game/rtech/utils/studio/studio_r5*.h` (the studiohdr the rig embeds, same structs as `mdl_`, see [rmdl](rmdl.md)), `game/asset.h` (`AssetType_t::ARIG` :18, post-load order :248-251, path prefix `animrig` :172) · **Games / versions:** Apex Legends only (asset versions 4, 5, 6, 7; the embedded studiohdr is any of the rmdl versions 8 to 20). Titanfall 2 has no `arig` (its skeleton lives in the loose `.mdl`, see `source-mdl`) · **Priority hint:** medium, **S** by itself (it is a header plus a truncated studiohdr that rmdl already has to parse), but it is the skeleton source for every `aseq`, so it gates animation export.

> **Scope of what was read.** All of `animrig.h`/`animrig.cpp` and the parts of `animseq.h`, `model.cpp`, `modeldata.cpp` named above were read directly in this run. Not read: `ModelParsedData_t` itself (constructor, `ParseModelBoneData*`, `ParseModelAnimTypes_*`, IK chains, pose parameters; they are the rmdl code, see [rmdl](rmdl.md)), `ExportModelMeshes` (called for the Cast/RMAX/SMD skeleton export, argument `54` unexplained), `CPakAsset::SetAssetName`/`WriteRSONDependencyArray` internals. No game file was opened; every offset below is computed by hand from the struct definitions and cross-checked against rsx's `static_assert(sizeof == 0x28)` (`animrig.h:19`, `:37`).

## Summary
An `arig` is a skeleton-only stand-in for a model. It holds a cut-down `studiohdr` (bones, attachments, hitboxes, IK chains, pose parameters; **no** meshes, materials, or vertex data) in the same layout the `mdl_` asset of the same pak uses, plus a list of `aseq` guids that animate that skeleton. Animations (`aseq`) do not name their skeleton; the rig (or the model) lists the sequences, and rsx wires each sequence to the first rig/model that lists it (`ParseExternalSequences`, `model.cpp:1051`). A user wants: the skeleton as a Cast/SMD skeleton, and the list of sequences it owns so the `aseq -> Cast animation` convert has bones to bind to.

## Where it lives
- Container: rpak. Kind FourCC as stored (file order): `arig`. rsx writes it reversed as the multi-char literal `'gira'` (`animrig.cpp:332`, `asset.h:249`). Header alignment 8 (`:333`). `kind_string()` extension would be `.arig`; rsx's raw-export extension is `.rrig` (`animrig.cpp:303`, the studio buffer as stored), and its folder prefix is `animrig` (`asset.h:172`). Proposed respak extension: `.arig` (Open questions 5).
- Name: **stored.** Header `name` page pointer to a NUL-terminated path (`LoadAnimRigAsset` :132-133, `SetAssetName(name, true)`; rsx asserts "Rig had no name"). The name is used as the skeleton name (`rigStem`) for exports.
- Streamed data: none. The whole asset is in pages.
- Order: `arig` is post-loaded after `asqd` and before `mdl_` and `aseq` (`asset.h:248-251`, comment "aseq after arig/model that way the skeleton is set before parsing"). In respak this is guid resolution at convert time, not a load order.

## Versions and detection
Asset version selects the **asset header** only; the **studiohdr variant is not self-describing** and rsx borrows it from the model assets of the same pak:

| asset version | asset header struct (size) | studiohdr variant | notes |
|---|---|---|---|
| 4 | `AnimRigAssetHeader_v4_t` (0x28) | rmdl version of the pak's `mdl_` assets | `numAnimSeqs` is an `int` |
| 5, 6, 7 | `AnimRigAssetHeader_v5_t` (0x28) | same | `numAnimSeqs` is a `short`, 2 extra bytes of unknown |
| other | not loaded (`default: return`, `animrig.cpp:61-62`) | | respak: `UnsupportedVersion` |

Studio variant (`animrig.cpp:23-29`, `:37`, `:47`, `:56`): `LoadAnimRigAsset` looks up `PakLoadedAssetTypeInfo_t` for `MDL_` in the same pak (`pak->GetLoadedAssetTypeInfo()`), asserts the models are consistent (`inconsistentHeaderSize || inconsistentVersions == false`, "pak had a hodgepodge of versions"), and calls `GetModelVersionFromAsset(pak, hdr->data, mdlInfo.version, mdlInfo.headerSize)` with the **mdl asset's** version and header struct size. The refinement logic is exactly the rmdl detection (12.x by studiohdr words 97/101/102/41, 13.1 by model stride, 14.1 by local node names, 19.x by `createdTime` and `sourceFilenameOffset == 244`), see [rmdl](rmdl.md) "Versions and detection" (`model.cpp:946-1049`). Two consequences:
- A pak with an `arig` but no `mdl_` hits `assertm(false, ...)` and then `loadedAssetInfo.at(...)` which **throws** (`:23-28`). Respak must not depend on a sibling `mdl_` existing.
- The 12.x branch checks `headerStructSize == sizeof(ModelAssetHeader_v12_1_t)` etc., which only makes sense with the mdl header size, so the rig cannot reuse the check on its own header (0x28).

Proposed respak: score function on `rpak.asset` with `kind == "arig"` -> score 10, `Version { major: entry.version, minor: 0 }` for 4 to 7. The studio variant is a second detection step inside the read edge: either take the pak's mdl info like rsx (needs a pak-wide pass), or detect from the embedded studiohdr alone (Open questions 1). `createdTime` (pak header 0x08) is needed for 19.x and is **not exposed by `respak-core`'s `PakHeader` today** (same gap as rmdl).
- Dead code: `s_AnimRigTimeStamp_V7_V19_2 = 0x1DCD1EAF65B4000` ("model v19.2, cascades into animrig, 22nd April 2026", `animseq.h:90`) is declared but never used (grep: no other references).

## Layout
Little-endian. Page pointers (8 bytes) are relocated by the rpak volume to `.mem` offsets; `null` = 0 or `u64::MAX` as in `txtr.rs`.

### Asset header (v4, `animrig.h:6-19`; v5/v6/v7, `animrig.h:21-37`)
| off | v4 | v5/v6/v7 | meaning |
|---|---|---|---|
| 0x00 | page ptr `data` | page ptr `data` | studiohdr + "rrig buffer" (the cut-down model data) |
| 0x08 | page ptr `name` | page ptr `name` | NUL-terminated path |
| 0x10 | i32 `unk_10` | i16 `unk_10` | unknown |
| 0x12 | (upper half of `unk_10`) | i16 `numAnimSeqs` | count of `animSeqs` in v5+ |
| 0x14 | i32 `numAnimSeqs` | i32 `unk_14` | count in v4, unknown in v5+ |
| 0x18 | page ptr `animSeqs` | page ptr `animSeqs` | -> `u64 guid[numAnimSeqs]` (aseq guids) |
| 0x20 | i64 `unk_20` | i64 `unk_20` | "probably reserved" (rsx comment) |
| total | 0x28 | 0x28 | |

Offsets computed from the struct order (natural alignment; the `static_assert`s confirm the size only). The `animSeqs` guids are `AssetGuid_t` (a `u64`).

### The embedded studiohdr ("rrig buffer")
`data` points to a normal studiohdr of the pak's rmdl variant. What it contains, from what the loader parses (`animrig.cpp:65-130`):
- bones: `ParseModelBoneData<mstudiobone_v8_t>` (v8-12), `<mstudiobone_v12_1_t>` (v12.1-15), `ParseModelBoneData_v16` (v16-18), `ParseModelBoneData_v19` (v19-20);
- attachments (`mstudioattachment_v8_t` / `_v16_t`), hitboxes (`ParseModelHitboxData<mstudiobbox_v8_t>` / `_v16`), IK chains and pose parameters (`ParseModelAnimTypes_V8` / `_V16`);
- local sequences (`numlocalseq`): **must be zero**, rsx asserts "arig had internal sequences" (`:157`) and has a full fallback that parses them (`ParseModelSequenceData_*`, same version split as the models) that "should never get hit".
Struct layouts and the bone formats are the rmdl ones; do not duplicate them here. Per-variant buffer size, used by rsx only for the raw `.rrig` dump (`animrig.h:96-132`):
- v16: `FIX_OFFSET(boneDataOffset) + boneCount * sizeof(mstudiobonedata_v16_t)` (= 128 per bone, see rmdl);
- v17/v18: same with `studiohdr_v17_t`;
- v19, v19.1: `IALIGN16(FIX_OFFSET(linearboneindex) + sizeof(mstudiolinearbone_v19_t) + dataPerBone * boneCount)` with `dataPerBone = 4 + 2 + 3*12 + 2*16 + 48 = 122` (`int` + `short` + 3 `Vector` + 2 `Quaternion` + `matrix3x4`);
- v19.2, v19.3, v20: same with `studiohdr_v19_2_t`;
- v8-v15: the studiohdr's own `length` field (via `ModelParsedData_t::length`).
The v16+ sizes end right after the bone data, which suggests (not verified) that the bone block is the last thing in the buffer; the attachments/hitboxes/IK parsed above must then sit before it.

## Data outside the header
- `data` (studiohdr + bones + attachments + hitboxes + IK + pose parameters) and `animSeqs` guid array: pages. No starpak, no compression.
- The `aseq` assets listed in `animSeqs` carry the animation data (see [aseq](aseq.md)).

## Dependencies
- `aseq` assets by guid (`animSeqs`). rsx resolves with `g_assetData.FindAssetByGUID` (`model.cpp:1067`) across all loaded paks; a missing guid is skipped silently.
- **Reverse link:** the `aseq` header does not name its rig. In `PostLoadAnimRigAsset` (`animrig.cpp:151`) `ParseExternalSequences` sets `animSeq->rig = <this rig's parsed data>` on each listed `aseq` **only if no rig is set yet** (`model.cpp:1080-1081`, first one wins). Because `arig` is post-loaded before `mdl_` (`asset.h:248-251`), a rig takes precedence over a model that lists the same `aseq`.
- Models list `aseq` and `arig` guids themselves (`animRigs`/`animSeqs` in the rmdl asset header), so an `arig` is normally reached from a model and shares its skeleton (bone names/order) with it. Whether the rig's bones are always identical to the model's is unverified (Open questions 3).
- In respak: `Metadata::dependencies()` = the `aseq` guids. The reverse mapping (which rig owns which `aseq`) is a derived index, not a stored field.

## Decoding
```
asset  = rpak.asset;  ver = asset.version                       // 4, 5, 6, 7
hdr    = read(asset.header, 0x28)                               // page ptrs relocated
n      = ver == 4 ? i32 @0x14 : i16 @0x12                       // numAnimSeqs
seqs   = n x u64 at hdr.animSeqs
studio = detect_variant(...)                                    // see Versions
rig    = studiohdr at hdr.data                                  // bones etc. via the rmdl parser
require rig.numlocalseq == 0                                    // else Corrupt/unsupported (rsx asserts)
skeleton = bones(rig)                                           // pos/quat parent-local, as rmdl
```
Everything else is the rmdl skeleton decode (v16-18 split bonehdr/bonedata, v19+ linearbone, v8-15 `mstudiobone`).

## Output
**rsx** (`ExportAnimRigAsset` :242; settings `CAST`, `RMAX`, `SMD`, `RRIG`, `modeldata.h:1559-1566`; default is the first, Cast):
- Folder `<export>/animrig/<rigStem>/` (or the asset's own directory when `exportPathsFull`).
- If `exportRigSequences` (default true, `exportsettings.cpp:7`) and `numAnimSeqs > 0`: every listed `aseq` that is loaded, post-loaded and parsed is exported through the `aseq` export setting into `anims_<rigStem>/<aseq filename stem>_<i>.cast` (see [aseq](aseq.md)), `animrig.cpp:271-275`, `animseq.cpp:274-327`.
- Cast/RMAX/SMD: `ExportModelMeshes(parsedData, <rigStem>.rrig, setting, 54)` which exports the **skeleton only** (comment "rmax & cast just export the skeleton for now, perhaps in the future we could also export IK?", `:301-311`).
- `RRIG`: raw studio buffer (`parsedData.length` bytes) plus a `.rson` next to it with the `seqs` guid array (`WriteRSONDependencyArray(..., "seqs", ...)`, `:223-239`).

**Proposed respak targets:**
- A new `arig` format: read edge `rpak.asset (arig) -> arig` with `Metadata` (name, guid, dependencies = aseq guids) and the `Skeleton` capability (same type the rmdl slice defines; the rig is its second implementor).
- `arig -> cast` (skeleton only): shares the rmdl Cast skeleton writer (bone nodes `n`, `p`, `lp`, `lr`). Optional: write its sequences as sibling animation casts through the `aseq -> cast` edge.
- No separate raw target needed; the studio buffer dump (`.rrig`) is of use only to rsx's own re-import. Drop unless asked.

## rsx quirks
- Studio variant borrowed from the pak's `mdl_` assets; `.at()` throws when none exist (`animrig.cpp:23-31`). Replace with detection from the studiohdr alone, or with an explicit `UnsupportedVersion` when ambiguous.
- Case `7` is a copy of `5`/`6` (`animrig.cpp:52-60`); merge.
- `ExportAnimRigAsset` exports sequences through `g_assetData.FindAssetByGUID`, a global lookup. In respak, linking resolves guids through the mediator.
- `asset.h` comment says rigs are post-loaded so sequences get the skeleton; the real effect is the "first lister wins" rule above. If a rig and a model list the same `aseq`, the rig's bone list is used. Keep the rule or make it explicit (Open questions 3).
- The raw `.rrig` size for v19.x is a computed bound, not stored (`animrig.h:114-128`). Raw dump only; drop.
- The `54` argument to `ExportModelMeshes` is unexplained (not read).

## Reuse in respak
- `rpak.asset` (`header_path`, `read_mem`, `mem_offset_of`), `txtr.rs` null-pointer convention, `GuidRef` + `identify` for guids.
- All the studiohdr bone parsing from the rmdl slice (arig is the rmdl skeleton path with no VG).
- New: `arig.rs` (0x28 header, version-dependent count width), pak-wide or self-contained studio-variant detection, `createdTime` in `PakHeader` (shared with rmdl).

## Tests
- Synthetic: a v5 header (0x28 bytes) with `numAnimSeqs = 2` at 0x12 and a v4 header with the count at 0x14; `animSeqs` guid array; a v16 studiohdr with 3 bones in the split form and no sequences; check the bone list equals the same bytes read through the rmdl path.
- Edge cases: asset version 3 or 8 (-> `UnsupportedVersion`), `numAnimSeqs = 0`, null `animSeqs`, studiohdr with `numlocalseq != 0`, truncated `data`.
- Real files (no Steam runs without permission): any Apex pak with an `arig` (character or weapon paks; no specific pak or path checked); compare bone count/names against the owning model's rmdl skeleton, and the `seqs` list against rsx's `.rson`.

## Open questions
1. **Studio-variant detection without a sibling `mdl_`: proposal (rsx has nothing).** rsx only ever uses the `mdl_` info of the same pak (`animrig.cpp:23-37`). What the rig needs is just the **skeleton layout**, and that is mostly decidable from the studiohdr itself: v19.2/v19.3/v20 (`studiohdr_v19_2_t`) when the u16 at studiohdr+0xD8 equals 244 (rmdl step 5; `GetModelVersionFromAsset`, `model.cpp:946-1049`); v16 to v19.1 otherwise, with `sourceFilenameOffset` (u16 at +0xC8) equal to 224. The remaining ambiguity is v16-18 (split `bonehdr`/`bonedata`, 128-byte bone data) versus v19.0/19.1 (16-byte bone data + linear-bone block); the 19.x bone-flag width (4 vs 6 bit) does not matter for the rig (it matters for `aseq`, which has its own version). **Decision (proposed):** (a) look up a sibling `mdl_` in the same pak when present and reuse its detected variant (rsx behaviour); (b) otherwise detect from the studiohdr as above and resolve v16-18 vs v19.0/19.1 by whether `linearboneindex` (u16 at +0xCA) is non-zero and `boneDataOffset - boneHdrOffset` fits `12 * boneCount` (v19 has the 16-byte form; to be confirmed on real bytes); (c) anything else -> `UnsupportedVersion`. Rationale: never fails just because a pak has no model, reuses the rmdl reader, and the one weak rule (b) is an explicit, testable heuristic. Also mixed paks (rsx asserts they do not exist) are unverified.
2. What are the `unk_10` (v4/v5), `unk_14` (v5+), and `unk_20` header fields? Not used by rsx. Still open.
3. Are an `arig`'s bones always identical (names, order, parents) to the owning model's? When a rig and a model both list the same `aseq`, rsx binds the rig. Still open (real-file check).
4. Which Apex seasons map to asset versions 4, 5, 6, 7? Not stated in rsx for this type. Still open.
5. File extension: **proposal** `.arig` (the stored FourCC, consistent with `.mdl_`); `.rrig` is rsx's raw-dump extension only.
