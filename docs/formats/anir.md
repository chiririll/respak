# anir · Respawn animation recording
**rsx sources:** `game/rtech/assets/anim_recording.h` (`AnimRecordingAssetHeader_v0_t`, `AnimRecordingFrame_s`, `AnimRecordingOverlay_s`, `AnimRecordingFileHeader_s`), `anim_recording.cpp` (`LoadAnimRecordingAsset` :8, `ExportAnimRecordingAsset` :21, `InitAnimRecordingAssetType` :149), `game/asset.h` (`ANIR` :21, prefix `anim_recording` :175) · **Games / versions:** Apex Legends, asset version 1 (the only one rsx exports) · **Priority hint:** low, **S**. Recorded player movement for in-game playback (a developer tool artefact), not a skeletal animation.

> **Scope of what was read.** Both files in full. rsx writes a custom `.anir` file; no other tool is known to read it. The meaning of several fields is unknown (rsx names `field_34` etc.). No game file was opened; offsets computed by hand and checked against `static_assert(sizeof == 0x330)` (`anim_recording.h:91`).

## Summary
An `anir` is a recorded sequence of player state: per frame time, origin, view angles, button mask, current sequence index, cycle and playback rate, plus overlay (layered sequence) samples, with a table of pose-parameter names/ranges and sequence names used to interpret indices. Max 3000 frames (`ANIR_MAX_RECORDED_FRAMES`), 48 sequences, 15 pose parameters. A user would convert it to a readable dump (JSON) for inspection.

## Where it lives
- Container: rpak. FourCC `anir` (rsx literal `'rina'`, `anim_recording.cpp:154`), header alignment 8. Name: **not stored** (`SetAssetNameFromCache`, "has no name var", `:14-15`), so guid/cache only. rsx writes `<export>/anim_recording/<filename>.anir`.
- Streamed data: none. Recorded frames/overlays are page data.

## Versions and detection
Asset version 1 only (`ExportAnimRecordingAsset` returns false otherwise, `:27-28`). Respak: `rpak.asset` kind `anir` -> score 10, `Version { 1, 0 }`; other versions -> `UnsupportedVersion`.

## Layout
`AnimRecordingAssetHeader_v0_t` (0x330, `anim_recording.h:37-64`; offsets computed). Pointers are page pointers.
| off | type | field |
|---|---|---|
| 0x000 | page ptr[15] | `poseParamNames` (C strings; list ends at the first null) |
| 0x078 | `{f32 start, f32 range}`[15] | `poseParamValues` (`{startValue, endValue - startValue}`) |
| 0x0F0 | page ptr[48] | `animSequences` (names; ends at first null) |
| 0x270 | page ptr[16] | `unkString` |
| 0x2F0 | Vec3 | `startPos` |
| 0x2FC | Vec3 | `startAngles` |
| 0x308 | page ptr | `recordedFrames` (`AnimRecordingFrame_s[numRecordedFrames]`) |
| 0x310 | page ptr | `recordedOverlays` (`AnimRecordingOverlay_s[numRecordedOverlays]`) |
| 0x318 | i32 | `numRecordedFrames` |
| 0x31C | i32 | `numRecordedOverlays` |
| 0x320 | i32 | `runtimeSlotIndex` (runtime) |
| 0x324 | i32 | `slotIndexSign` (runtime) |
| 0x328 | i32 | `animRecordingId` |
| 0x32C | bool | `isPersistent` |
| 0x32E | i16 | `runtimeRefCounter` |
Comments: the writer code fills only 12 of the 15 pose-param entries; names/values come from `mstudioposeparamdesc_t`.

`AnimRecordingFrame_s` (0x44 = 68 bytes; `204000 / 3000` allocation comment agrees): `f32 currentTime` 0, `Vec3 origin` 4, `QAngle angles` 0x10, `i32 animRecordingButtons` 0x1C, `i32 animSequenceIndex` 0x20, `f32 animCycle` 0x24, `f32 animPlaybackRate` 0x28, `i32 overlayIndex` 0x2C, `i32 activeAnimOverlayCount` 0x30, `i16 field_34`, `i16 field_36`, `i32 field_38`, `i32 field_3C`, `i16 field_40`, `i8 field_42`, `i8 field_43`.
`AnimRecordingOverlay_s` (0x24 = 36 bytes; `108000 / 3000`): `i32 animSequenceIndex` 0, `f32 animOverlayCycle` 4, `f32 overlays[6]` 8, `i32 slot` 0x20.

## Data outside the header
None beyond the page data above. No compression.

## Dependencies
None by guid. Sequences are referenced **by name** (`animSequences`), indices in frames point into that table; the names are `aseq`/model sequence labels but nothing links them to assets.

## Decoding
Read the header, then the string/vector tables up to the first null pointer (max 15 and 48), then `numRecordedFrames` frames and `numRecordedOverlays` overlays with bounded reads (counts are bounded by 3000 by the game's allocation; reject larger as `Corrupt`).

## Output
**rsx `.anir`** (`anim_recording.cpp:21-147`), little-endian, packed (`AnimRecordingFileHeader_s`, `pack(1)`, 56 bytes): `i32 magic` (`'R'<<24|'I'<<16|'N'<<8|'A'`, i.e. bytes `ANIR` in LE order... as a native int), `u16 fileVersion = 1`, `u16 assetVersion`, `Vec3 startPos`, `Vec3 startAngles`, `i32 stringBufSize`, `i32 numElements`, `i32 numSequences`, `i32 numRecordedFrames`, `i32 numRecordedOverlays`, `i32 animRecordingId`. Then: pose-param names (`writeString`, NUL-terminated by assumption: `stringBufSize` adds +1 per name), `numElements` x `{f32,f32}` pose values, sequence names, frames, overlays. `stringBufSize`/`numElements`/`numSequences` are patched in afterwards (seek to offset 32) only if either count is non-zero. `isPersistent`, `runtimeSlotIndex`, `unkString` are dropped.
**Proposed respak:** read edge `anir` with `Metadata`; a convert `anir -> json` (header scalars, pose params `{name, start, end}`, sequence names, frames and overlays as arrays). No capability needed (not skeletal). The binary `.anir` is rsx-specific; skip.

## rsx quirks
- Pose-param loop stops at the first null pointer; strings are taken with unbounded `strlen` (`:87`, `:111`). Respak: bounded C-string reads.
- Header written first with zero counts and patched later; the patch is skipped when both are zero (harmless).
- Reuse of `AnimRecordingFileHeader_s`'s `Vector` type means a 12-byte vec; `QAngle` assumed 3 x f32.

## Reuse in respak
`rpak.asset` pointer/CString reads (as `txtr.rs`), JSON output helpers from `matl_json`. New: `anir.rs`.

## Tests
Synthetic: a 0x330 header with 2 pose params, 3 sequence names, 2 frames, 1 overlay; check counts, 15/48-entry bounds, null termination, and that 3001 frames is `Corrupt`. Real files: any Apex pak containing an `anir` (unknown which; none checked).

## Open questions
1. Is a JSON dump the wanted output, or skip `anir` entirely?
2. Meaning of `field_34`..`field_43`, `unkString`, and `overlays[6]`.
3. Exact `writeString` semantics (NUL-terminated?) are irrelevant if respak only emits JSON.
