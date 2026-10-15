# aseq, asqd · Respawn animation sequence and its data
**rsx sources:** `game/rtech/assets/animseq.h` (`AnimSeqAssetHeader_v7_t/_v7_1_t/_v8_t` :8-59, `eSeqVersion` :61, `GetAnimSeqVersionFromAsset` :92, `AnimSeqAsset` :119, `RawSizeV7/V11/V12_1` :226+), `animseq.cpp` (`LoadAnimSeqAsset` :17, `AnimSeq_ParseExtraData` :80, `ExportRawAnimSeqAsset` :184, `ExportAnimSeqAsset` :246/:330, `ExportAnimSeqFromAsset` :274), `animseq_data.h/.cpp` (`asqd`: `LoadAnimSeqDataAsset` :5, `ParseAnimSeqDataForSeq` :74), `core/mdl/animdata.{h,cpp}` (`ModelAnim_t`, `ModelSeq_t`, per-version seqdesc/animdesc parsing :124-875, `pAnimdata*` section lookup :264-433, `ParseAnimDesc_Origin` :892, `ParseAnimation` :1046, `ParseSequence` :1269, `Studio_AnimPosition` :1669), `game/rtech/utils/studio/studio_r5.{h,cpp}` (RLE headers, anim value tracks, datapoint decoders), `studio_r5_v12.h` (animdesc v12.1 :97), `studio_r5_v16.h` (animdesc/seqdesc v16 :255/:309, seqdesc v18 :739, animdesc v19.1 :915, `AnimQuat32`/`AnimPos64` :1164-1240), `core/math/compressedvector.h` (`Quaternion64`, `Vector48`), `core/mdl/modeldata.cpp` (`ExportSeqDescCast` :3409), `core/mdl/cast.{h,cpp}` (Animation/Curve nodes, :464-591), `game/rtech/assets/model.cpp` (`ParseExternalSequences` :1051), `game/asset.h` · **Games / versions:** Apex Legends only (aseq asset versions 7 to 14, `asqd` version 1). Titanfall 2 sequences are in the loose `.mdl`/`.seq` (see `source-mdl`); rsx has an r2 animation path (`ParseAnimation(... r2::studiohdr_t*)` :935) used by that loader, not covered here · **Priority hint:** medium-high (all character/weapon animation), **L** (several seqdesc/animdesc layouts, three compressions, external data).

> **Scope of what was read.** Read directly: all of `animseq.h/.cpp`, `animseq_data.*`, `animdata.h`, `animdata.cpp` lines 1-1733, `studio_r5.cpp` in full, the animation parts of `studio_r5.h`, `studio_r5_v12.h`, `studio_r5_v16.h`, `compressedvector.h` (Quaternion64/48, Vector48), the Cast curve code and `ExportSeqDescCast`. Not read: `CAnimData` (`ToMemory`, bone buffers), IK rule/lock structs and compressed IK errors, events (`mstudioevent_v12_3_t` layout), movement structs (`mstudiomovement_t`), `ExportSeqDescRMAX/SMD`, QC writers, the pak loader that fills `dataExtra`. No game file was opened; all offsets are computed by hand from struct definitions (checked against rsx `static_assert`s where they exist).

## Summary
An `aseq` is one animation sequence: a Source-style `mstudioseqdesc` (label, activity, flags, events, blend grid, autolayers, bone weights, pose keys, IK locks) plus one `mstudioanimdesc` per blend (name, fps, frame count, flags, frame movement, IK rules, section table). The per-bone key data is run-length (RLE) or "datapoint" compressed and is split into **sections**. From v12.1 the main data moved into a separate `asqd` asset (one per animdesc, referenced by guid); sections flagged external live in the aseq's `dataExtra` blob (starpak). The sequence does not name its skeleton: bones come from the `arig` or `mdl_` that lists the `aseq` guid. A user wants each animdesc as a Cast animation (per-bone position, rotation, scale curves) that Blender and other tools import.

## Where it lives
- Container: rpak. `aseq` FourCC as stored (file order): `aseq`; rsx literal `'qesa'` (`animseq.cpp:399`). `asqd`: `'dqsa'` (`animseq_data.cpp:148`). Header alignment 8 for both. rsx export folders `animseq` and `animseq_data` (`asset.h:173-174`); raw extensions `.rseq` (+ `.rseq_extn`, `.json`) and `.asqd`. Proposed respak names: `<name>.aseq`, `0x<GUID>.asqd` (Open questions 5).
- Name (`aseq`): **stored**, header `name` page pointer, used as sequence path (`animseq.cpp:76`). `asqd`: the loader sets **no** name (`animseq_data.cpp:5-29`); export uses `GetAssetName()`, so it is whatever the generic cache/guid fallback gives (Open questions 5).
- Streamed data: `aseq` header `dataExtra` (v7.1+) with `dataExtraSize`, "external data, sometimes streamed" (`animseq.h:34`, `:57`). rsx reads `getStarPakStreamEntry(false)` (`animseq.cpp:24`) and stores it in `dataExtraStreamed`, but **never uses it** (grep: only assigned in `animseq.h`). It reads the data through `hdr->dataExtra`, so the pointer is expected to be valid already (see Open questions 1 for the proposed fallback). `asqd`: page data only.
- Order: `asqd`, `arig`, `mdl_`, then `aseq` (`asset.h:246-252`) so the skeleton and data blobs exist before parsing. In respak this is guid resolution.

## Versions and detection
`GetAnimSeqVersionFromAsset` (`animseq.h:92`): the asset version through `s_seqVersionMap` (7..14, `:76-86`), refined by two rules; anything else -> `VERSION_UNK` (`LoadAnimSeqAsset` `default: return`).
- 7 -> 7.1 if `headerStructSize == sizeof(AnimSeqAssetHeader_v7_1_t)` (56), else 7.
- 12 -> 12.1 if the pak header `createdTime >= 0x01DC1DF805C28000` (comment "09/05/2025"), else 12. A date-based guess, same kind as rmdl's 19.x rules.
- `s_AnimSeqTimeStamp_V13 = 0x01DD1EED32D6C000` ("07/29/2026") is declared and unused.

| asset ver | variant | header (size) | seqdesc | animdesc | data lookup (`AnimdataFuncType_t`) | bone-flag width | rsx season comments |
|---|---|---|---|---|---|---|---|
| 7 | 7.0 | v7 (48) | `mstudioseqdesc_v8_t` | `mstudioanimdesc_v8_t` (0x34) | `NOSTALL` | n/a (see below) | seasons 0-6 |
| 7 | 7.1 | v7.1 (56) | v8 | `mstudioanimdesc_v12_1_t` | `STALL_BASEPTR` | 4 | seasons 7-8 |
| 8 | 8.0 | v8 (64) | v8 | v12.1 | `STALL_BASEPTR` | 4 | "season 8.1" |
| 9 | 9 | v8 | **not constructed** (see quirks) | | | | |
| 10 | 10 | v8 | v8 | v12.1 | `STALL_BASEPTR` | 4 | seasons 9-14 |
| 11 | 11 | v8 | `mstudioseqdesc_v16_t` | `mstudioanimdesc_v16_t` (0x28) | `STALL_BASEPTR` | 4 | |
| 12 | 12.0 | v8 | `mstudioseqdesc_v18_t` ctor arg 0 | v16 | `STALL_BASEPTR` | 4 | |
| 12 | 12.1 | v8 | v18 ctor arg 1 | `mstudioanimdesc_v19_1_t` (0x30) | `STALL_ANIMDATA` | 4 | |
| 13, 14 | 13, 14 | v8 | v18 ctor arg 1 | v19.1 | `STALL_ANIMDATA` | **6** | 14 = "v20 mdl / v14 seq", s30.1 |

(`animseq.h:134-196`, `animseq.cpp:80-150`, `animdata.cpp:683-875`.) rsx's own comment: the bone-flag width follows the model version, 4 bits for model 19.1/19.2, 6 for 19.3/20 (`animrig.cpp:200-213`). Bone-flag width for v7 to v11: the 4-bit macros are the default argument (`ParseSequence(..., flagWidth = ANIM_BONEFLAG_BITS_4)`, `animdata.h:542`); v7's NOSTALL path has no `flagWidth` parameter in practice, see Decoding.

Model-embedded sequences (`mdl_`/`arig` `numlocalseq`) use the same seqdesc/animdesc structs: model 8-12 -> NoStall v8; 12.1-15 -> v8 + STALL v8; 16-17 -> v16; 18-19 -> v18/0; 19.1-19.2 -> v18/1 with 4-bit; 19.3-20 -> 6-bit (`animrig.cpp:159-213`, `animdata.cpp:1292-1403`).

Proposed respak: score function on `rpak.asset` with `kind == "aseq"` -> score 10, `Version { major: entry.version, minor }` with minor 1 for 7.1 and 12.1, else 0. Needs `entry.header_size` and pak `createdTime` (not in `PakHeader` today). Unknown version or 9 -> `UnsupportedVersion`. `asqd`: score 10, `Version { 1, 0 }`.

## Layout
Little-endian. Page pointers (8 bytes) are relocated by the rpak volume. The seqdesc/animdesc structs use **relative offsets from the struct address** (not the `baseptr` int field). v16+ structs are `pack(2)` with `FIX_OFFSET` u16 offsets (`FIX_OFFSET(v) = (v & 0xFFFE) << (4 * (v & 1))`, see rmdl).

### aseq asset header
| field | v7 (48) | v7.1 (56) | v8 (64) |
|---|---|---|---|
| `data` (page ptr, seqdesc + animdescs + tracks) | 0x00 | 0x00 | 0x00 |
| `name` (page ptr, C string) | 0x08 | 0x08 | 0x08 |
| `unk_10` (rsx: "set to an allocated struct for 16 bytes on load") | - | - | 0x10 |
| `models` (page ptr -> `u64 guid[]`) | 0x10 | 0x10 | 0x20 |
| `numModels` | u64 @0x18 | u32 @0x18 | u16 @0x18 |
| `numSettings` | u64 @0x28 | u64 @0x28 | u16 @0x1A |
| `dataExtraSize` (bytes) | - | u32 @0x1C | u32 @0x1C |
| `effects` (page ptr -> guids) | - | - | 0x28 |
| `settings` (page ptr -> guids) | 0x20 | 0x20 | 0x30 |
| `dataExtra` (page ptr, external sections) | - | 0x30 | 0x38 |
(`animseq.h:8-59`; sizes asserted by rsx; offsets computed.)

### asqd asset header (v1, `animseq_data.h:5-9`, 8 bytes)
`data` page ptr @0. The blob has no stored size; rsx computes it by walking the bone headers (`ParseAnimSeqDataForSeq`, see Decoding) and only needs it for raw export.

### mstudioseqdesc
Fields rsx uses. v8 (`studio_r5.h:594`, pack 4, size 0xD0): `baseptr` 0, `szlabelindex` 4, `szactivitynameindex` 8, `flags` 0xC, `activity` 0x10, `actweight` 0x14, `numevents` 0x18, `eventindex` 0x1C, `bbmin` 0x20, `bbmax` 0x2C, `numblends` 0x38, `animindexindex` 0x3C (-> `int[groupsize0*groupsize1]` of offsets to animdescs), `movementindex` 0x40, `groupsize[2]` 0x44, `paramindex[2]` 0x4C, `paramstart[2]` 0x54, `paramend[2]` 0x5C, `paramparent` 0x64, `fadeintime` 0x68, `fadeouttime` 0x6C, `localentrynode` 0x70, `localexitnode` 0x74, `nodeflags` 0x78, `entryphase` 0x7C, `exitphase` 0x80, `lastframe` 0x84, `nextseq` 0x88, `pose` 0x8C, `numikrules` 0x90, `numautolayers` 0x94, `autolayerindex` 0x98, `weightlistindex` 0x9C (-> `f32[boneCount]`), `posekeyindex` 0xA0, `numiklocks` 0xA4, `iklockindex` 0xA8, `keyvalueindex` 0xAC, `keyvaluesize` 0xB0, `cycleposeindex` 0xB4, `activitymodifierindex` 0xB8, `numactivitymodifiers` 0xBC, `ikResetMask` 0xC0, `unk_C4` 0xC4, `weightFixupOffset` 0xC8, `weightFixupCount` 0xCC.

v16 (`studio_r5_v16.h:309`, pack 2, ends at 0x70): `szlabelindex` u16 0, `szactivitynameindex` u16 2, `flags` i32 4, `activity` u16 8, `actweight` u16 0xA, `numevents` u16 0xC, `eventindex` u16 0xE, `bbmin` 0x10, `bbmax` 0x1C, `numblends` u16 0x28, `animindexindex` u16 0x2A (-> `u16[]`, each `FIX_OFFSET`'d, relative to the seqdesc), `paramindex[2]` i16 0x2C, `paramstart[2]` 0x30, `paramend[2]` 0x38, `fadeintime` 0x40, `fadeouttime` 0x44, `localentrynode` u16 0x48, `localexitnode` u16 0x4A, `numikrules` u16 0x4C, `numautolayers` u16 0x4E, `autolayerindex` u16 0x50, `weightlistindex` u16 0x52, `groupsize[2]` u8 0x54, `posekeyindex` u16 0x56, `numiklocks` u16 0x58, `iklockindex` u16 0x5A, `unk_5C` u16 0x5C, `cycleposeindex` u16 0x5E, `activitymodifierindex` u16 0x60, `numactivitymodifiers` u16 0x62, `ikResetMask` i32 0x64, `unk_68` i32 0x68, `weightFixupOffset` u16 0x6C, `weightFixupCount` u16 0x6E.
v18 (`:739`) = v16 plus `noInterpFrameOffset` u16 0x70, `noInterpFrameCount` u16 0x72 (size 0x74). v18 `weightlistindex` special values: **1** = all-zero weights, **3** = all-one weights (256 entries each), else `FIX_OFFSET` (`:1411-1428`). The `aseq` v12 loader treats `weightlistindex == 1 || 3` as "no bone count available" (`animseq.h:166-170`).
Blend count: `AnimCount() = groupsize[0] * groupsize[1]`, and `ModelSeq_t::numblends` is set to that, not to the `numblends` field (`animdata.cpp:513`, "apex can have numblends set to 0 despite having anims"). Blend index list: v8 `int[]` offsets; v16/v18 `u16[]` with `FIX_OFFSET`.
Autolayers (`mstudioautolayer_v8_t`, 0x20): `u64 sequence` (**aseq guid**) 0, `iPose` 8, `flags` 0xC, `start` 0x10, `peak` 0x14, `tail` 0x18, `end` 0x1C (`studio_r5.h:551`). Events: v8 `{cycle f32, event i32, type i32, options[256], szeventindex i32}` (`:540`); a v12.3 variant exists, chosen by `(autolayerindex - eventindex) / numevents == sizeof(mstudioevent_v12_3_t)` (`animdata.cpp:633`); v16 `{cycle, event, type, unk_C, optionsindex u16, szeventindex u16}` 0x14. Event layouts beyond that were not read.

### mstudioanimdesc (offsets relative to the animdesc)
| field | v8 (0x34, pack 4) | v12.1 (0x48, pack 4) | v16 (0x28, pack 2) | v19.1 (0x30, pack 2) |
|---|---|---|---|---|
| `baseptr` (unused) | 0x00 | 0x00 | - | - |
| `sznameindex` | i32 @4 | i32 @4 | u16 @0xC | u16 @0xC |
| `fps` f32 | 8 | 8 | 0 | 0 |
| `flags` i32 | 0xC | 0xC | 4 | 4 |
| `numframes` i32 | 0x10 | 0x10 | 8 | 8 |
| `nummovements` / `movementindex` | 0x14 / 0x18 | 0x14 / 0x18 | - | - |
| `framemovementindex` | i32 @0x1C | i32 @0x1C | u16 @0xE | u16 @0xE |
| `animindex` (non-zero when data not in sections) | 0x20 | 0x20 | i32 @0x10 | - (data is in `asqd`) |
| `numikrules` | 0x24 | 0x24 | u16 @0x14 | u16 @0x10 |
| `ikruleindex` | 0x28 | 0x28 | u16 @0x16 | u16 @0x16 |
| `animDataAsset` (u64 **asqd guid**) | - | - | - | 0x18 |
| `sectionDataExternal` (runtime ptr) / unk | - | unk i64 @0x38, ptr @0x40 | ptr @0x18, u16 @0x20 | ptr @0x20, u16 @0x28 |
| `sectionindex` | 0x2C | 0x2C | u16 @0x22 | u16 @0x2A |
| `sectionstallframes` | - | 0x30 | u16 @0x24 | u16 @0x2C |
| `sectionframes` | 0x30 | 0x34 | u16 @0x26 | u16 @0x2E |
Sources: `studio_r5.h:500`, `studio_r5_v12.h:97`, `studio_r5_v16.h:255`, `:915`. `sznameindex` of v16/v19.1 goes through `FIX_OFFSET`. The name is "`name__cmd_hash`" as compiled; a leading `@` marks a seq-declared name (`IsSeqDeclared`, `animdata.h:232`).
Section entry: v8 `{i32 animindex}`; v12.1 `{i32 animindex, bool isExternal}` (8 bytes); v16+ `{i32 animindex}` with the **sign bit marking external**, real index `~animindex` (`studio_r5_v16.h:248`).
Section count (`ModelAnim_t::SectionCount`, `animdata.h:271`): if `numsections` set, that; else `trail = (flags & DATAPOINT) ? 0 : 1`, `stall = sectionstallframes > 0`, `maxIndex = (numframes - sectionstallframes - 1) / sectionframes + trail + stall`; count = `maxIndex + 1`, or `maxIndex` when `useAnimData` (v19.1, "the first section is omitted, always at offset 0 of the animdata"). Only computed if `sectionframes != 0` and `ANIM_VALID` (`animdata.cpp:132`, `:167`, `:202`, `:237`).

### Flags (animdesc `flags` / seqdesc `flags`, `studio.h:879-913`, `studio_r5.h:476-498`)
`0x1` LOOPING, `0x2` SNAP, `0x4` DELTA (additive), `0x8` AUTOPLAY, `0x10` POST, `0x20` ALLZEROS (no real data), `0x40` unknown (`studio.h` names it FRAMEANIM, `studio_r5.h` UNK40), `0x80` CYCLEPOSE, `0x100` REALTIME, `0x200` LOCAL, `0x400` HIDDEN, `0x800` OVERRIDE (forward declared, empty), `0x1000` ACTIVITY, `0x2000` EVENT, `0x4000` WORLD, `0x8000` NOFORCELOOP, `0x10000` EVENT_CLIENT, **`0x20000`**: on an animdesc "has animation data" (`ANIM_VALID`), on a seqdesc "uses scale data" (`ANIM_SCALE`), `0x40000` FRAMEMOVEMENT, `0x80000` SINGLE_FRAME, `0x100000` unknown ("reactive animations?"), **`0x200000` DATAPOINT** (datapoint compression instead of RLE), `0x1000000` added in s30.1, unseen, rsx asserts it is absent (`animdata.cpp:820-821`).

### RLE animation data (`ANIM_VALID` without `ANIM_DATAPOINT`)
At the section start (resolved by the lookup in Decoding):
1. **Bone flag array**, `ANIM_BONEFLAG_SIZE(boneCount, width)` bytes = `IALIGN2((width * boneCount + 7) / 8)`. 4-bit: nibble `i` = `byte[i/2] >> (4 * (i%2))`, masked `0x0F`. 6-bit: little-endian u16 at byte `bitpos>>3`, shifted by `bitpos & 7`, masked `0x3F`, with `bitpos = 6 * i` (`studio_r5.h:400-413`).
2. **Bone headers**, one per bone whose flags contain any of POS/ROT/SCALE (`0x7`), in bone order, starting right after the flag array: `mstudio_rle_anim_t`, 2 bytes: `size:13` (total bytes of this header + data, i.e. next = `this + size`), `flags:3` (`0x1` ANIMSCALE, `0x2` ANIMROT, `0x4` ANIMPOS: that channel is a track pointer, else a constant) (`studio_r5.h:415-445`). Data follows, order **pos, rot, scale**:
   - pos: constant `Vector48` (3 x float16, 6 bytes), or (ANIMPOS) `f32 posScale` + `mstudioanim_valueptr_t` (8 bytes total);
   - rot: constant `Quaternion64` (8 bytes), or (ANIMROT) one `valueptr` (4 bytes);
   - scale: constant `Vector48` (6 bytes), or (ANIMSCALE) one `valueptr` (4 bytes).
3. `mstudioanim_valueptr_t` (4 bytes, `studio_r5.h:364`): `offset:13` (bytes from this struct to the first track), `flags:3` (`0x4` X, `0x2` Y, `0x1` Z present), `axisIdx1 u8`, `axisIdx2 u8` (track index offsets, in `mstudioanimvalue_t` units of 2 bytes, to the 2nd and 3rd present axes' tracks). Track pointers: `values[0] = base`, `values[1] = base + axisIdx1`, `values[2] = base + axisIdx2`; axis `j` (0=X,1=Y,2=Z) is read from `values[j]` only if `flags & (0x4 >> j)` (`studio_r5.cpp:160`, `:169`).
4. Bone flags other than data: `0x8` UNK8 ("used in virtual models for new anim type"; only meaningful in datapoint), `0x10`, `0x20` (6-bit only, "TBD"; rsx asserts clear in RLE, `animdata.cpp:1218`).

### Track encoding: `mstudioanimvalue_t` (2 bytes, `studio.h:862`)
Union of `{u8 valid, u8 total}` and `i16 value`. A track is a chain of blocks; each block covers `total` frames. To find frame `f`: `while (block.total <= f) { f -= block.total; block += GetAnimValueOffset(block); }` (`studio_r5.cpp:109-121`). Block size in 2-byte units (`GetAnimValueOffset`, `:24-29`): `LUT[valid*3] + ((LUT[valid*3+1] + total * LUT[valid*3+2]) >> 4)` with `s_AnimSeekLUT[60]` = `1,15,16, 2,7,8, 2,15,0, 3,15,0, 4,15,0, 5,15,0, 6,15,0, 7,15,0, 2,15,2, 3,15,2, 4,15,2, 5,15,2, 6,15,2, 7,15,2, 2,15,4, 3,15,4, 4,15,4, 5,15,4, 6,15,4, 7,15,4` (`:11-17`; `valid` therefore 0..19). Value decode (`ExtractAnimValue`, `:31-107`), result times `scale`:
- `valid == 0`: `value = block[1 + f]` (an `i16` per frame).
- `valid == 1`: `value = block[1]`; if `f > 0` add the signed byte at `((i8*)&block[2])[f-1]`.
- `valid >= 2`: `bitIdx = (valid-2)/6`, `numPoly = (valid-2)%6`; `value = block[1]`; `cycle = f / (total - 1)`; for `i in 0..numPoly`: `value += block[i+2] * cycle^(i+1)`. If `bitIdx`: `value -= {0,3,15,0}[bitIdx]`; `bits = {0,2,4,0}[bitIdx]`; `bitOffset = f * bits`; `value += 2.0 * (((i16*)&block[numPoly+2])[bitOffset >> 4] >> (bitOffset & 15)) & ((1<<bits)-1))`.
Scales: rotation `FIXED_ROT_SCALE = 0.00019175345` (radians per unit, Euler); position `posScale` from the bone header; scale `FIXED_SCALE_SCALE = 0.0030518509` (`studio_r5.cpp:20-21`).
Per-bone result (integer frame, `s == 0`; `CalcBonePosition/Quaternion/Scale`, `:148-313`):
- rot, tracked: `RadianEuler angle[j] = track value` for present axes, else the bone's default `rot` (from the rig bone); `q = AngleQuaternion(angle)`. Constant: `Quaternion64` -> `q` (see below).
- pos, tracked: `tmp[j]` = value for present axes, 0 else; **`pos += tmp`** onto the starting pos (rig bone pos, or zero for delta). Constant: `pos = Vector48` (replaces).
- scale: same shape as pos (`scale += tmp`, constant replaces), scale factor above.
- `Quaternion64` (`compressedvector.h:11`): u64 bitfield LSB first: `x:21, y:21, z:21, wneg:1`; `x = (x - 1048576) * (1/1048576.5)`, same y,z; `w = sqrt(1 - x²-y²-z²)` (negated if `wneg`).
- `Vector48`: three IEEE half floats.
Non-integer frames (`s != 0`) interpolate: values blend linearly, rotation blends two Euler-derived quaternions with `QuaternionBlend` unless equal.

### Datapoint animation data (`ANIM_DATAPOINT`, `0x200000`)
rsx comment: "datapoint anims are not shipping" and the newer `useFlagTmp` function variants are unused and untestable (`studio_r5.h:1048`). Layout per bone (old variants, the ones `ParseAnimation` calls, `studio_r5.cpp:320-499`), same flag array and `mstudio_rle_anim_t` header as RLE (the header's `size` still steps to the next bone; data begins at `header + 1`), each channel a "track" in order **rot, pos, scale**:
- rot: `u8 valid, u8 total`, `frameIndices` (`total` x u8, omitted when `total >= sectionLength`), `AnimQuat32[valid]`, `AxisFixup_t[total]` (3 x i8 each).
- pos: `u8 valid, u8 total`, indices, `AnimPos64[valid]` (8 bytes), `AxisFixup_t[total]`. With bone flag `0x8` (virtual): `u8 total`, `f16 packedScale`, indices, `AxisFixup_t[total]`; value = `adjustment * (packedScale / 127)`.
- scale: `u8 total`, indices, `Vector48[total]`.
`AnimQuat32` (u32 bitfield LSB first, `studio_r5_v16.h:1164-1200`): `value0:7, value1:7, value2:7` (signed), `scaleFactor:3`, `droppedAxis:2`, `numInterpFrames:6`. Unpack (`studio_r5.cpp:746`): `scaleComponent = scaleFactor ? 0.011048543 : 0.0055242716`, `scaleFixup = (1 << scaleFactor) * 0.000021924432`, `axis_k = (value_k + 0.5) * scaleComponent + fixup.adjustment[k] * scaleFixup`; dropped component `sqrt(1 - sum(axis²))` (0 if the sum >= 1); `droppedAxis` 0..3 selects which of x,y,z,w is the dropped one, the three stored axes fill the others in order. `AnimPos64` (`:1208`): `i16 values[3]`, `scaleFactor:9`, `numInterpFrames:7`; unpack `pos[k] = (adjustment[k]/100 + values[k]) * (scaleFactor + 1)`. Each datapoint covers `numInterpFrames` following frames (`CalcBoneSeek_DP`, `studio_r5.h:1054`); frame selection by `CalcBoneInterpFrames_DP` (`:1104`): if `total >= sectionLength` frames are dense (prev = floor(frame), next = prev+1), else binary search in `frameIndices`. Datapoint values are stored **relative**; for non-delta animations rsx composes them with the bone: `q' = boneQuat * q`, `pos' = boneQuat * pos * conj(boneQuat) + bonePos` (`animdata.cpp:1152-1176`).

### Section lookup (which bytes hold frame `f`) and external data
All return a pointer to the bone flag array and rewrite `f` to a section-local frame (`animdata.cpp:264-433`):
- `NoStall` (v7): `index = animindex`; if `sectionframes`: `section = f / sectionframes`, `f -= section * sectionframes`, but if `numframes > sectionframes && f == numframes-1` the last frame is stored separately: `f = 0`, `section = (numframes-1) / sectionframes + 1`; `index = section[s].animindex`; result `animdesc + index`.
- `Stall_0` (RLE, v12.1-v18 data layout): if `sectionframes`: if `f >= sectionstallframes`: last-frame special case `section = (numframes - stall - 1)/sectionframes + 2`, `f = 0`; else `section = (f - stall)/sectionframes + 1`, `f -= sectionframes * ((f-stall)/sectionframes) + stall`; `index = section[s].animindex`. Section 0 (`f < stall`) uses `animindex`. If the section is external: `sectionDataExtra + index` (when `dataExtra` is null: falls back to section 0 and `f = stall - 1`). Else `animdesc + index`.
- `Stall_1` (datapoint, no trailing section): `sectionbase = (f - stall)/sectionframes; section = sectionbase+1; f -= stall + sectionbase*sectionframes`; also returns `sectionLength` (`sectionframes`, the remainder, or `stall` for section 0).
- `Stall_2` (v12.1+, `asqd`): as `Stall_1` but with no `animindex` and section 0 not stored; sections after the stall section are `section-1` in the table; non-external -> `animData + index` where `animData` is the **asqd blob**; external -> `sectionDataExtra + index`; `animDataAsset == 0` means no data.
So from 12.1 on: `index` in a non-external section entry is a byte offset into the asqd blob, external ones into the aseq's `dataExtra`.

`asqd` size (`ParseAnimSeqDataForSeq`, `animseq_data.cpp:74-141`): the last non-external section's `animindex` is the start (0 if no sections); then walk `boneCount` flags and `pNext()` over every bone with a header; blob size = end - start of blob. The loop only advances `flagBitOffset` for bones **with** a header (`:129-135`), which is wrong for 6-bit flags (see quirks).

### Frame movement (root motion)
`mstudioframemovement_t` (`studio_r5.h:450`): `f32 scale[4]` (x,y,z,yaw), `i32 sectionframes`, then per-section offsets (`i32` v8/v12.1, `FIX_OFFSET` u16 v16+). Each section is `u16[4]` offsets (relative to the section) to `mstudioanimvalue_t` tracks (same track encoding, scale from `scale[i]`). With `ANIM_DATAPOINT`: one section, datapoint pos (`u16 valid, total`, indices, `AnimPos64`, `AxisFixup`) then yaw (`u16 total`, indices, float16). Evaluated at `cycle` by `Studio_AnimPosition` (`animdata.cpp:1669`). Older `mstudiomovement_t` piecewise movement exists (`nummovements`); not read.

## Data outside the header
- `data`: seqdesc, animdescs, section tables, v7/v7.1/v8-11 inline tracks, IK, events, strings, weight lists.
- `dataExtra` (aseq v7.1+, `dataExtraSize`): external sections, referenced by negative/`isExternal` section entries. "Sometimes streamed".
- `asqd` blobs (aseq v12.1+): bone flag arrays + bone headers + tracks, referenced by `animDataAsset`.
- Compression: none beyond the animation encodings above; the asqd/dataExtra bytes are not RTech/Oodle compressed in anything rsx does.
- Raw export size: `AnimSeqAsset::dataSize` is computed heuristically by walking the data (`RawSizeV7/V11/V12_1`, `animseq.h:226-652`); only used for the `.rseq` dump. Drop.

## Dependencies
- **`arig` / `mdl_` (the skeleton).** Not referenced from the aseq; the rig/model lists the aseq (`animSeqs`) and `ParseExternalSequences` sets `seq->rig` (first wins; arig before mdl) (`model.cpp:1051`, [arig](arig.md)). A sequence with no rig is not parsed and cannot be exported as Cast (`animseq.cpp:80-86`, `:343-348`). Respak needs a reverse index (aseq guid -> owner) or an explicit rig option.
- **`asqd`** by guid per animdesc (`animDataAsset`, v12.1+); rsx looks it up with `FindAssetByGUID` and skips the animation (log: "required asset ... not loaded") when absent (`animseq_data.cpp:84-91`, `animdata.cpp:1281`).
- `models` (guids, header), `effects` (v8 header), `settings` (guids): listed in the header; rsx does not resolve them for conversion (dump to `.json` only, `animseq.cpp:203-241`). Autolayer `sequence` guids point to other `aseq`.
- In respak: `Metadata::dependencies()` = asqd guids, autolayer aseq guids, models, settings, effects.

## Decoding
```
asset = rpak.asset;  ver = detect(asset)                  // table above
hdr   = read(asset.header, size per ver)
seq   = seqdesc at hdr.data  (v8 / v16 / v18 per table)
rig   = owner skeleton (bones: name, parent, pos, quat, rot(euler), scale)
for each blend i in 0 .. groupsize0*groupsize1:
    ad = animdesc at seq + animindex[i]
    skip if !(fps in [0,2048]) or numframes not in [0,0x20000]     // rsx sanity check, treats as no animdesc
    base[b] = delta ? (0, identity, 1) : (rig.pos, rig.quat, rig.scale); rot[b] = rig.rot
    for f in 0 .. numframes:                                // integer frames only
        flags_ptr, lf, seclen = lookup(ad, f)               // Section lookup
        panim = flags_ptr + flagArraySize(boneCount, width)
        bitpos = 0
        for b in 0 .. boneCount:
            fl = boneFlags(flags_ptr, b, width, bitpos); bitpos += width
            if fl & 7:                                      // has header
                decode pos/rot/scale per channel (RLE or datapoint), see Layout
                panim = panim.next
            store frame f for b (flags kept per bone)
    origin fix-up on bone 0 (below)
```
Notes:
- Evaluate integer frames directly (`s = 0`). rsx instead computes `cycle = f/(numframes-1)`, `fFrame = cycle*(numframes-1)`, `iFrame = (int)fFrame` (`animdata.cpp:1107-1112`), where float rounding can give `iFrame = f-1, s = 0.99999` and an interpolated sample; harmless but needless.
- `numframes == 1`: `cycle = 0`.
- Bone 0 ("origin") fix-up (`ParseAnimDesc_Origin`, `animdata.cpp:892-933`), applied to every animation: force flags POS|ROT on bone 0; per frame add frame-movement/movement position (and yaw) at that cycle to bone 0's pos/yaw, convert the quaternion to a Source `QAngle`, `yaw += -90`, pos `(x, y) -> (y, -x)`, back to a quaternion. This is a coordinate change for the root only.
- RLE needs only `boneCount` and the bone defaults from the rig; datapoint non-delta also needs rig quats (above).
- If `ANIM_VALID` is not set (or `animDataAsset` unresolved) the animation has no tracks: every bone gets the rest pose for every frame, flags 0; rsx's Cast writer then writes only the skeleton (`modeldata.cpp:3454-3459`).
- v7 (NoStall) is the only layout without `isExternal`; r2 (Titanfall 2) uses a different per-bone `mstudio_rle_anim_t` with explicit bone ids (`animdata.cpp:989-1011`), not covered.

## Output
**rsx** (`ExportAnimSeqAsset`, settings `CAST`, `RMAX`, `SMD`, `RSEQ`; default Cast; `modeldata.h:1579-1586`): standalone export needs the sequence to be parsed with a rig. Folder `<export>/animseq/<seqStem>/` (or asset dir). A sequence is one file **per blend animdesc**: `<seqStem>_<animIdx>.cast` (`modeldata.cpp:3420`); from a rig/model export, into `anims_<rigStem>/` (`animseq.cpp:274-327`, `animrig.cpp:271-299`). `RSEQ`: raw seqdesc buffer + `.rseq_extn` (the `dataExtra` bytes) + `.json` dependency list.
**Cast animation (`ExportSeqDescCast`, `modeldata.cpp:3409-3534`):**
- Root -> `Animation` node (id `0x6D696E61`, hash = the aseq asset guid). Properties: `fr` (f32 framerate = animdesc `fps`), `lo` (u8 looping = `flags & LOOPING`). `n` (name) is **not** written.
- Child `Skeleton` (hash `StringToGuid(fileNameBase)`) with `Bone` nodes from the rig: local (parent-local) position and quaternion, parent index (`MakeBone(name, parent, &pos, &quat, isGlobal=false)`). Scale not written.
- `Curve` nodes (id `0x76727563`), keyed by the bone name, per bone in this order: **position** as three float curves (`kp` = `tx`,`ty`,`tz`), **rotation** as one `Vector4` quaternion curve (`kp` = `rq`), **scale** as three float curves (`sx`,`sy`,`sz`) only if the seqdesc has `0x20000`. Properties: `nn` (node/bone name), `kp` (key property), `kb` (key frame buffer: `0..numframes-1`, type `u8`/`u16`/`u32` by `numframes`; every frame is keyed), `kv` (key value buffer, one value per frame), `m` (mode string), `ab` (f32 additive blend weight, written only in `additive` mode) (`cast.h:482-544`, `cast.cpp:404-591`).
- Mode: `additive` if the animdesc has `DELTA` (weight = the seqdesc bone weight `weight(boneIdx)`), else `absolute` (weight 1 default, not written). Channels a bone has no data for are written as a **constant**: the rig's rest value (absolute) or identity (delta), repeated for all frames.
- No `NotificationTrack` (events dropped), no blend-grid parameters, no IK, no autolayers.
- Cast framing as in rmdl (`cast` magic, version 1).

**Proposed respak targets:**
- A new `aseq` format: read edge `rpak.asset (aseq) -> aseq` (`Metadata`, dependencies) plus `Animation` capability implemented per animdesc (see below). Needs the rig as a link input: `aseq -> cast` takes `{ "rig": guid | "auto" }` with `auto` = the reverse-index owner.
- `asqd`: read edge only, bytes plus metadata, no standalone convert (rsx's `.asqd` dump is a raw copy).
- `aseq -> cast` convert edge, composite output (per PROJECT.md §12 rules): default **all blend animdescs as one file each** named `<stem>_<i>.cast` (matches rsx), or a single file for the common 1x1 case. Options `{ "rig": ..., "blend": "all" | n }`.
- Shared Cast writer with rmdl (Animation/Curve nodes are the only additions).

## Animation capability: fields a refined `Animation` trait needs
Today `pub trait Animation: Send + Sync {}` (`respak-core/src/capability/animation.rs`). From the above, per animdesc (one blend):
- `skeleton: Arc<dyn Skeleton>` binding or bone name list, plus an explicit "bones not animated" rule (default to rest pose);
- `fps: f32`, `frame_count: u32`, `looping: bool`, `additive: bool` (delta) and optional per-bone `weight: f32`;
- `tracks: Vec<BoneTrack>` indexed like the skeleton, each `{ translation: Option<Track<[f32;3]>>, rotation: Option<Track<[f32;4]>> (x,y,z,w), scale: Option<Track<[f32;3]>> }`, where a `Track` is either `Constant(v)` or `Dense(Vec<v>)` of `frame_count` samples (sparse key lists are an encoder concern; the decoder samples every frame);
- root motion folded into bone 0 (as rsx does), documented in the trait, or a separate `root_motion` field;
- `name`, and optional events `[(cycle, name, options)]` for formats that carry them (rsx drops them).
A lazy sampling method (`fn sample(&self, bone, frame) -> ...`) is better than materialising `bones x frames` for large clips; the trait should allow both.

## rsx quirks
- `s_seqVersionMap` contains **9** but neither the `AnimSeqAsset` constructor (`animseq.h:137-195`) nor `AnimSeq_ParseExtraData` (`animseq.cpp:91-145`) handles it; the seqdesc stays default-constructed. Respak: reject 9 as unsupported unless a real file shows otherwise.
- `7` constructs the seqdesc from a `v8` struct, `7.1/8/10` use the same seqdesc with the **v12.1 animdesc**, and `AnimSeq_ParseExtraData` groups `7.1, 8, 10, 11` together (`STALL_BASEPTR`) even though 11 uses the v16 structs. Keep the mapping in the table, not the grouping.
- Rig discovery is by reverse lookup, first lister wins (`model.cpp:1080`); standalone aseq export fails without it (`assert` + return false, `animseq.cpp:343-348`).
- `ANIMDESC_SANITY_CHECK` (`fps < 0 || fps > 2048 || numframes < 0 || numframes > 0x20000`) makes the whole sequence drop to 0 blends (`animdata.cpp:511`, `:546`). Keep as a corrupt-data check.
- Float cycle round-trip for frame selection (above). Drop.
- `flagBitOffset` in `ParseAnimSeqDataForSeq` is not advanced for bones without a header (`animseq_data.cpp:129-135`), unlike `ParseAnimation`; with 6-bit flags this reads the wrong bits. Only affects the computed `asqd` size.
- Bone-flag masks: 4-bit uses `0x0F` (RELEASE), 6-bit `0x3F` (RETAIL); `0x10`/`0x20` are asserted clear in RLE only.
- Cast export keys **every** frame and writes constants as full-length curves; it also ignores `ab` weights in absolute mode. Output is large. Respak may collapse constant curves (behaviour change) once a Cast reader confirms single-key curves are accepted (Open questions 4).
- Static buffers: `ParseAnimation` uses `std::vector`, but `Studio` r2 path uses fixed `[256]` arrays (`animdata.cpp:939`).
- Datapoint (`ANIM_DATAPOINT`) and 6-bit flags have been tested against little or no shipping data (`studio_r5.h:1048`, `:407` "TBD if this works the same").
- `ExportAnimSeqFromAsset` skips sequences that were not post-loaded or not parsed (`animseq.cpp:305-315`) without reporting; respak should report.
- Global state: `g_rsxSettings.exportRigSequences`, `exportPathsFull`; become options.

## Reuse in respak
- `rpak.asset` (`header_path`, `read_mem`, `mem_offset_of`, `stream`), `guid` helpers and `GuidRef` for asqd/arig/mdl guids, `Metadata`, the rmdl/arig bone parsers, `FIX_OFFSET`, the Cast writer from rmdl.
- New: `aseq.rs` (headers, seqdesc/animdesc v8/v12.1/v16/v18/v19.1, section lookup), `asqd.rs`, `anim_codec.rs` (RLE tracks, `Quaternion64`/`Vector48`/half floats, datapoint), `Animation` capability fill-in, `aseq_cast.rs`, `PakHeader.created_time`.
- Half floats: check `half` or a small own implementation (rsx `float16`, not read).

## Tests
- Synthetic: (a) 2-bone v16 seqdesc + v16 animdesc, `numframes = 4`, no sections; bone 0 `valid=0` tracks with known `i16` values, bone 1 constant `Quaternion64`; expect exact floats. (b) track chains: `valid=1` (base + signed bytes), `valid=8` (2-bit packing), `valid>=3` polynomial; block-seek across two blocks (check `GetAnimValueOffset` table values, e.g. `valid=0,total=5` -> 6 units, `valid=1,total=4` -> 4 units). (c) 4-bit vs 6-bit flag arrays for 5 bones. (d) a v19.1 animdesc referencing a synthetic `asqd` blob with a stall section + one section + one external section (`dataExtra`). (e) datapoint: one `AnimQuat32` with known fields, `AnimPos64`, `AxisFixup`, `numInterpFrames` seeking. (f) Cast output: curve count, `kb` width at `numframes` 255/256/65536, additive mode + `ab`.
- Detection: header size 48 vs 56 for v7, `createdTime` split for 12 vs 12.1, version 9 and 15 -> `UnsupportedVersion`.
- Edge cases: no rig, missing asqd guid, `sectionframes = 0`, `numframes = 1`, sanity-check failure (`fps = 4096`), `groupsize` 0, `weightlistindex` 1 and 3, truncated blob (bounded reads -> `Corrupt`).
- Real files (no Steam runs without permission): any Apex character pak's `aseq` plus its `arig`; compare frame count, fps and a few bone transforms at frames 0 and last against rsx's Cast export (allowing for the dropped quirks).

## Open questions
Resolved (questions 1, 3, 4, 5 of the first pass; numbering kept out of the text above):
1. **`dataExtra` and the starpak: answered from rsx, plus a proposal.** rsx never reads aseq data from a starpak: `getStarPakStreamEntry(false)` is only stored in `dataExtraStreamed` and never used (`animseq.cpp:24`, `animseq.h:210`); external sections are read through `hdr->dataExtra` as a plain page pointer (`animseq.h:129`, `animdata.cpp:320-321`, `animseq.cpp:199`). Other kinds that do use a stream call `getStarPakData(entry.offset + skip, size, opt)` explicitly (`wrap.cpp:57-66`, `cpakfile.h:1232-1256`; model reads its VG the same way, `model.cpp:28`, `:225`). **Decision (proposed):** read `dataExtra` as a page pointer first (this is the only path rsx exercises). If the pointer is null, `dataExtraSize > 0` and the asset has a mandatory starpak entry, read `dataExtraSize` bytes from that entry (`starpak::split_reference` + `Starpak::entry`, as rmdl does for the VG). Rationale: it matches every other streamed kind, costs one branch, and fails soft (the stall frame is used) if neither exists. Verify on a real v12.1+ aseq which of the two occurs.
2. Bone-flag width for aseq 7 to 11 (all 4-bit assumed from the default argument): still open. **Checked on a real file (2026-10-15 pilot):** aseq **14** with a 181-bone `arig` v7 decodes cleanly with **6-bit** flags (unit quaternions, smooth curves, no `Corrupt`); the 12.1/13 widths are still unchecked.
2a. **Pilot findings (aseq 14, `common.rpak` of the latest depot, two blends, 201 and 191 frames, 30 fps, flags `0x20005` = valid + looping + delta):** header, seqdesc v18 and animdesc v19.1 offsets matched the notes; `weightlistindex` was 3 (all ones); no frame movement; `dataExtra` is a **page pointer** (size 12984) and a `.stream` entry also exists (the page pointer was used and decoded to real data); 4 table entries for stall 16 / section frames 62 (3 external, 1 in the `asqd`). Last frame: the table's last entry is the trailing section, but rsx's `Stall_2` has no special case, so the last frame is found by the normal arithmetic and the trailing section is never read (respak did read it at first; fixed, see task 19). The `arig` here is version 7 (181 bones) and its studio layout came from the heuristic.
3. **Bone-0 root fix-up vs the rmdl Cast skeleton: answered from rsx, plus a proposal.** They are not consistent in rsx. The model Cast writes bone bind pose unchanged (`modeldata.cpp:2574`, `cast.cpp:345-352`), and vertices are written unchanged too (`StaticPropFlipFlop` is applied only in the SMD/QC paths, `modeldata.cpp:3077`, `qc.cpp:990`). The animation Cast writes its skeleton node the same raw way (`modeldata.cpp:3449`) but its bone-0 curves include the -90 yaw and `(x,y)->(y,-x)` fix-up (`animdata.cpp:920-926`), so bone 0 in the animation is rotated relative to its own skeleton node and to the model. The rmdl task notes (`tasks/18-slice-rmdl-cast.md`) say Cast is written "as rsx writes it" with no root transform, so the same mismatch would carry over. **Decision (proposed):** an `aseq -> cast` option `root_fixup` = `"rsx"` (default, parity with rsx output so the demo comparison and its known Blender workflow hold) or `"none"`; the demo checks in Blender whether the animated root lines up with the imported model, and the default flips if it does not. Rationale: the intent of the fix-up is not documented in rsx, so removing it blindly risks a regression and keeping it blindly risks a mismatch; an option settles it with one real import.
4. **Cast constant curves and quaternion order: answered from rsx, spec not available locally.** No Cast spec or importer source exists on this machine (searched `Projects`, `respak/docs`; `respak-formats/src/cast.rs` has no curve code yet). rsx quaternion order is `x, y, z, w` in memory (`core/math/vector.h:190`) and is written raw as a `Vector4` for both bones and `rq` curves (`cast.cpp:350`, `:502`). rsx never writes a single-key curve: constants are replicated to `numframes` keys (`cast.cpp:463-481`, `modeldata.cpp:3491-3494`). **Decision (proposed):** write full-length curves with `kb = 0..n-1` exactly like rsx; do not collapse constants in this slice. Rationale: it is the only form known to import; collapsing is a size optimisation to try later against a real importer.
5. **`asqd` naming: proposal.** rsx stores no name for `asqd` (`animseq_data.cpp:5-29`), and the rmdl notes show guid-only kinds are listed as `0x<GUID>.<kind>` (`tasks/18-slice-rmdl-cast.md`, "all named by guid only"). **Decision (proposed):** extension = the stored FourCC, so `0x<GUID>.asqd`, `<name>.aseq`, `<name>.arig`; do not adopt rsx's `.rseq`/`.rrig` (rsx-only raw dump extensions). Rationale: consistent with `.mdl_` and `kind_string()`; extension is free to change at the gate.
6. Output granularity: one `.cast` per blend animdesc (rsx) or a single file for a 1x1 sequence. Still open (user preference).
7. Version 9, the v12.3 event layout, IK rules, `noInterpFrames` and `weightFixup` are not decoded here. Proposal: out of the first slice, curves only.
8. Titanfall 2 sequences (r2 RLE) go through `source-mdl`; should the `Animation` capability serve both? Still open.
