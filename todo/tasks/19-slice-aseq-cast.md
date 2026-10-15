# 19 · Slice: rpak animation (`aseq`) → Cast
**Status:** in-progress (built; pilot check done on one real aseq 14; demo pending) · **Depends on:** 18 (`Skeleton`, Cast writer, `rmdl` bone parser) · **See:** PROJECT.md §5.5, §6, §12, [`../formats/aseq.md`](../formats/aseq.md), [`../formats/arig.md`](../formats/arig.md), [`../formats/anir.md`](../formats/anir.md)

## Goal
An Apex animation sequence opens from an rpak and converts to Cast animations, for Blender and other Cast importers:
```
respak convert paks/Win64/common.rpak/animseq/.../x.aseq --to '{"type":"cast"}'
```
Output is a composite (own folder, PROJECT.md §12): `x/<stem>_<i>.cast`, one file per blend animation. Each file carries the skeleton and per-bone position / rotation (/ scale) curves.

## Decided already (user, 2026-10-15)
- **Root rotation:** apply it (rsx behaviour), as an option `root_fixup` = `"rsx"` (default) | `"none"`. The Blender import at the demo decides whether the default stays.
- **`arig` variant without a `mdl_`:** configurable. Option `studio` picks the variant (`auto` default: sibling `mdl_` first, then the studiohdr heuristic from arig.md; or an explicit layout name). No variant found → `UnsupportedVersion`.
- **`dataExtra` from a starpak:** no preference from the user, so the proposal in aseq.md stands (page pointer first, then the mandatory starpak entry). Checked on the first real v12.1+ file.
- **Rig for a sequence:** option `rig` = `"auto"` (reverse index: the `arig` / `mdl_` that lists the `aseq`, first wins, `arig` before `mdl_`) or a guid / path.

## Proposed defaults (agree or change at the gate)
- **Versions:** aseq **12.1 to 14** only (seqdesc v18, animdesc v19.1, data in `asqd`, `STALL_ANIMDATA` lookup; 6-bit bone flags for 13/14). This is the latest-Apex-only rule from slice 18. 7 to 12.0 and 9 → `UnsupportedVersion` (backlog). 12 vs 12.1 needs `PakHeader::created_time` (added in 18).
- **Compression:** RLE tracks only. Datapoint animations (`0x200000`, "not shipping" in rsx) → `UnsupportedVersion` for now.
- **Granularity:** one `.cast` per blend animdesc, as rsx (`<stem>_<i>.cast`); option `blend` = `"all"` (default) or an index.
- **Constant channels:** written as full-length curves, exactly like rsx (no collapsing in this slice).
- **Sampling:** integer frames only (`s = 0`), not rsx's float cycle round trip.
- **`asqd` naming:** `0x<GUID>.asqd`; read edge only (bytes + metadata), no convert.
- **`Animation` capability** is defined here with the Cast encoder as its first consumer: skeleton binding, `fps`, `frame_count`, `looping`, `additive` + per-bone weights, per-bone `Track` (`Constant` | `Dense`) for translation / rotation (x,y,z,w) / scale, lazy `sample`. Root motion is folded into bone 0 (documented).
- **Metadata:** `aseq` implements `Metadata` (dependencies: asqd, autolayer aseq, models, settings, effects). A separate JSON target is not part of this slice.

## Proposed scope
- Plugin: `respak-respawn` (new `aseq.rs`, `asqd.rs`, `arig.rs`, `anim_codec.rs`) + `respak-formats` (Cast `Animation` / `Curve` nodes in `cast.rs`) + `respak-core` (`Animation`). No plugin-to-plugin dependency.
- `arig`: read edge that yields a `Skeleton` (reuses the rmdl bone parser; studio variant per the `studio` option). No Cast export of the rig itself.
- Detection: score on `rpak.asset` kind `aseq` / `asqd` / `arig`; `Version { major: entry.version, minor }`.
- Data: seqdesc v18, animdesc v19.1, section lookup `Stall_2`, RLE bone headers, value tracks (`mstudioanimvalue_t` chains), `Quaternion64`, half-float `Vector48`, bone-0 fix-up, delta (additive) animations.

## rsx review
**What it does** (details and offsets in aseq.md / arig.md): `animseq.{h,cpp}` (version detection, load, extra data), `animseq_data.{h,cpp}` (`asqd`), `animrig.{h,cpp}`, `core/mdl/animdata.{h,cpp}` (seqdesc/animdesc parsing, section lookup, `ParseAnimation`, `ParseAnimDesc_Origin`), `utils/studio/studio_r5*.{h,cpp}` (RLE headers, value tracks, `Quaternion64`), `modeldata.cpp` `ExportSeqDescCast`, `cast.cpp` (Animation / Curve nodes).

**Problems** (kept out of the port):
- Asserts and unchecked page-pointer reads in the section lookup and track walk → bounded reads, `Corrupt`.
- Version 9 in the map but never constructed; grouping 7.1/8/10/11 together although 11 uses other structs → table of variants, 9 rejected.
- Float cycle round trip for the frame index → integer frames.
- `flagBitOffset` not advanced for bones without a header in the `asqd` size walk → size is not needed at all (ranged reads), so the code is dropped.
- Standalone export fails silently without a rig; skipped sequences not reported → `MissingDependency` / `Warning`.
- Global settings (`exportRigSequences`, `exportPathsFull`) → target options.
- Every frame of every channel keyed, including constants → kept for now (behaviour parity), collapse is backlog.
- Whole-asset in-memory parse → ranged reads per animdesc, frames decoded on demand.

**Fit:**
| rsx | respak |
|---|---|
| `GetAnimSeqVersionFromAsset` | score function on `rpak.asset`, `Version` from the table; `created_time` from `PakHeader` |
| seqdesc / animdesc parsing | `aseq.rs`, ranged reads, tables instead of `switch` chains |
| `asqd` loader | `asqd.rs`, read edge, referenced by guid per animdesc |
| RLE tracks, `Quaternion64`, `Vector48` | `anim_codec.rs`, pure functions, unit-tested |
| rig reverse lookup | reverse index in the plugin or the `rig` option |
| `ExportSeqDescCast` | `aseq -> cast` encoder over the `Animation` capability |

**Behaviour changes against rsx** (for the demo comparison): integer-frame sampling; sequences without a resolvable rig fail with `MissingDependency` instead of being skipped; datapoint animations are `UnsupportedVersion` instead of decoded; versions below 12.1 are `UnsupportedVersion`.

## Sketch (written 2026-10-15, compiles, bodies are `todo!()`)
Not yet registered in the plugins (registering changes `formats_prints_the_graph`; that happens at build, as in 16/18). Only unused-variable/dead-code warnings fire.
- `respak-core::capability::animation`: `Animation` (async, lazy, `Cap` impl added) with `blend_count()`, `info(blend) -> AnimationInfo { name, fps, frame_count, looping, additive, has_scale }`, `bones() -> &[Bone]` (the bound skeleton), `bone_weights() -> Vec<f32>` (empty = all 1), `track(blend, bone, cx) -> BoneTrack { translation, rotation [x,y,z,w], scale }` where each channel is `Track<T> = Constant(T) | Dense(Vec<T>)`, and `sample(blend, bone, frame, cx) -> BoneSample`. Channels without data are `Constant` (bind pose, or identity for additive). Values are as the game stores them; root motion is folded into bone 0.
- `respak-formats::cast`: `CastWriter::animation(name, fps, looping, bones)` (an `anim` node instead of `modl`, with the `skel`) and `curve(bone, key, mode, weight, frames, values)` (a `curv` node: `nn`, `kp`, `kb` by frame count, `kv`, `m`, `ab` when additive). `CastOptions` gains `root_fixup: RootFixup (rsx | none, default rsx)` and `blend: BlendSelect (all | n, default all, with resolve(blend_count))`. `CastAnimationEncode` (`Animation -> cast`, composite `<stem>_<i>.cast` per blend, each with the skeleton) and the private `root_fixup` (bone 0: yaw -90, `(x, y) -> (y, -x)`).
- `respak-respawn`:
  - `anim_codec` (pure, bounded, unit-tested at build): `FlagWidth`, `flag_array_size`, `bone_flags`, `RleHeader::parse`, `ValuePtr::parse/track`, `block_units` (`s_AnimSeekLUT`), `decode_track`, `unpack_quaternion64`, `half_vec3`, `euler_to_quat`, `BoneFrame`, `decode_bone`.
  - `aseq`: `variant(asset_version, created_time)`, `AseqHeader::parse` (64 bytes), `SeqDesc`, `AnimDesc` (+ `locate(frame)` = `Stall_2`), `SectionData::{Asqd, Extra}`, `Weights`, `AseqAsset: Data + Animation + Metadata`, `AseqRead::new(RigSelect, StudioSelect)` (`rpak.asset -> aseq`), `RigSelect` (`"auto"` | guid/path).
  - `asqd`: `AsqdHeader::parse`, `AsqdAsset: Data + Metadata` with a ranged `read(offset, len)`, `AsqdRead`.
  - `arig`: `ArigHeader::parse` (v4 count `i32@0x14`, v5-7 `i16@0x12`), `studio_variant`, `ArigAsset: Data + Skeleton + Metadata`, `ArigRead::new(StudioSelect)`, `StudioSelect` (`"auto"` | layout name), and `owner_of(aseq_guid, origin, studio, cx)` (the reverse index: first `arig`, else first `mdl_`, that lists the sequence).
  - Reuse: `rmdl::parse_bones` (the bone block of `Studio::parse` moved into a function, behaviour unchanged, tests pass), `rmdl::B` and `read_guids` made `pub(crate)`; `fix_offset`, `hex`, `locate`, `pak_of` reused as they were.

Design points this sketch decides (shout if wrong):
- **`Animation` serves several blends.** One aseq holds a blend grid, and the encoder writes one file per blend, so the trait takes a `blend` index on `info`/`track`/`sample` and has `blend_count()`; the other choice (one `Animation` value per blend) would need a second value type and a bundle edge.
- **The skeleton is part of `Animation`** (`bones()`), not a second capability looked up on the value. The aseq copies the bones of its rig at read time, so the Cast encoder stays generic (no pak knowledge) and needs no extra link input.
- **The rig is resolved in `AseqRead`, not in the Cast convert.** Options of a read edge cannot come from the request, so `rig` and `studio` are plugin settings like `search` / `ignore_patches` (`AseqRead::new(rig, studio)`, `ArigRead::new(studio)`); the plugin builder methods are added at build, with the registration. A request-level override would need the rig as a link input (backlog).
- **`root_fixup` is applied by the Cast encoder, not by the capability.** `Animation` stays in game space (frame movement folded into bone 0, no yaw/axis change), so `root_fixup: none` is the raw data and the capability is reusable for other formats. The fix-up needs quaternion to Source-angle conversion in `cast.rs`.
- **`Track` has no `Option`**: a channel without data is a `Constant` rest value (as rsx writes it); `has_scale` says whether scale is real data.
- **`asqd` is not a `Blob`.** "Bytes" means the ranged `read(offset, len)` the aseq uses, because the blob has no stored size.
- **Effects guids** (header 0x28) are left out of `dependencies()`: the header has no count for them.
- **`variant` rejects 12.0 as well as 7 to 11**, since only seqdesc v18 / animdesc v19.1 are decoded; 12.0 uses the v16 animdesc.
- **`StudioSelect`/`RigSelect` serialise as plain strings** (`"auto"` or a name) through `From<String>`; they are not part of the Cast target options.

## Gate answers (user, 2026-10-15)
1. Versions: aseq **12.1 to 14** only, datapoint → unsupported (accepted with "okok").
2. Granularity: one `.cast` per blend animdesc, as rsx (accepted).
3. Rig: the animation is useless in Blender without the skeleton, so it is needed and stays in the Cast file. It **resolves automatically by default** (`rig: "auto"`, reverse index); the explicit `rig` option remains only as an override.
4. Extensions `.aseq` / `.asqd` / `.arig` (accepted).
5. Real files: yes (user, 2026-10-15): one pilot `aseq` + its rig from the cached `common.rpak` (`--steam 1172470 --depot 1172471`), within the same permission as slice 18. No other Steam runs.
6. Tests: the proposed list below is agreed.

## Built (2026-10-15)
- `respak-core`: the `Animation` capability as sketched (`blend_count`, `info`, `bones`, `bone_weights`, `track`, `sample`; `Track` is `Constant` or `Dense`), `Cap` impl added.
- `respak-formats::cast`: `CastWriter::animation` / `curve`, `CastOptions { root_fixup, blend }`, `BlendSelect::resolve`, `CastAnimationEncode` (`Animation -> cast`, registered on the `cast` format beside the mesh edge). 7 new unit tests (16 in the module): anim node and curve properties, `kb` width at 1 / 255 / 256 / 65535 / 65536 frames, additive `ab`, bad curves, root fix-up, option parsing.
- `respak-respawn`:
  - `anim_codec`: flag arrays (4 and 6 bit), RLE headers, value pointers, `s_AnimSeekLUT` block sizes, every track kind (raw, base plus bytes, polynomial plus 2/4-bit residuals), `Quaternion64`, half-float `Vector48`, Euler and quaternion conversion, section indexing, per-bone decode. 15 unit tests, including random bytes.
  - `aseq` (`.aseq`): variant table (12.1 by pak time, 13, 14), 64-byte header, seqdesc v18, animdesc v19.1, `Stall_2` lookup, frame movement, `Animation` + `Metadata`, `AseqRead` with the rig resolved when the sequence is read (`rig` setting: `auto` or guid / path). Sections are read through a growing window from the `asqd` or `dataExtra` and cached per sequence.
  - `asqd` (`.asqd`): header, `AsqdAsset::read` (strict) and `read_upto` (clamped), `Metadata`.
  - `arig` (`.arig`): header (v4 `i32` count, v5-7 `i16`), `studio_variant` (option, sibling `mdl_`, heuristic), `Skeleton` through `rmdl::parse_bones`, `owner_of` (reverse index over the pak and the `common.rpak` beside it, `arig` before `mdl_`, remembered in the plugin cache and checked again on use) and `explicit_rig`.
  - `RespawnPlugin::rig` / `studio`, and the formats `aseq`, `asqd`, `arig` registered. `testing::aseq` fixtures (sequence block, sections, tracks, `Quaternion64` / `Vector48` encoders) and `PakBuilder::{aseq, asqd, asqd_without_data, arig, rmdl_with_sequences}`.
  - 38 tests in `crates/respak-respawn/tests/aseq.rs`: variants and unsupported versions, header, seqdesc / animdesc parsing and sanity failures, section lookup, 4-bit and 6-bit decode through a pak, stall + regular + trailing + external sections, `Quaternion64` / `Vector48` constants, additive and weights, no data, frame movement, missing rig / `asqd` / `dataExtra`, datapoint, truncated section, random bytes (headers, blocks, `asqd` content), dependencies, `asqd` reads, `arig` headers / studio variants / local sequences, rig choice (arig over mdl_, mdl_, `common.rpak`, explicit guid and path).
- `respak-cli`: global `--rig` and `--studio` (plugin settings, like `--search`).
- 9 end-to-end tests in `integration/tests/aseq.rs` (where slice 18's live; one drives the CLI `run` with `--rig`): one file per blend with the expected curves, `root_fixup` `rsx` vs `none`, `blend`, additive weights, scale curves, 300 frames (16-bit `kb`), errors per output, cancellation.
- Docs: README, `docs/cli.md`, backlog.
- Totals: 69 new tests. `cargo test --workspace` passes. `cargo clippy --workspace --all-targets -- -D warnings` fails only on the existing `len_without_is_empty` in `respak-core/src/capability/blob.rs` (not touched); with that one lint allowed it is clean. `cargo tree` for `respak-respawn` / `respak-formats` shows no new dependency (`respak-respawn` already depended on `respak-formats`; `Cargo.toml` is unchanged).

Findings and decisions that differ from, or complete, the spec:
- Last frame: first built with a section of its own (the trailing section); the pilot check showed rsx's `Stall_2` has no such case, so now every frame with `f >= stall` is table entry `(f - stall) / sf` at local frame `(f - stall) % sf`; a single-frame animation or `sectionframes = 0` is all section 0. A missing table entry is `Corrupt`.
- An external section with no `dataExtra` is `MissingDependency` for that animation (the notes' "use the stall frame" fallback is not built). `dataExtra` is read from the page pointer, else from the mandatory starpak entry (not covered by a test: no synthetic starpak fixture for it).
- Datapoint animations fail with `UnsupportedVersion` when their tracks are asked for, not when the sequence is read, so the metadata and the other blends still work.
- `root_fixup: rsx` is a turn about Z by -90 degrees applied after the bone's rotation (the same as lowering the Source yaw by 90 degrees, without the angle round trip) and `(x, y) -> (y, -x)`.
- A channel is a `Constant` only if every frame took it from a constant (or had no data) and all values are equal; a track is `Dense` even when its values happen to repeat.
- `kb` is `u8` up to 255 frames, `u16` up to 65535, else `u32` (the notes say "by numframes" only). An additive curve with no weight gets `ab = 1`.
- The studio heuristic for an `arig` without a `mdl_`: `u16` at 0xD8 = 244 means `v19_2`; else a non-zero `u16` at 0xCA means the linear-bone form (major 19); else split (major 18); nothing to go on is `UnsupportedVersion`. `v16` and `v17` read bones the same way, so `v17` is reported for both and `studio` = `v17` stands for major 18.
- `weightlistindex` 0 is read as "all weights 1" (the spec names only 1 and 3).
- The 6-bit flag read accepts a last field that ends at the end of the array (rsx reads the next byte of whatever follows).
- `owner_of` has no `studio` parameter and `AseqRead::new` takes only the rig setting: the rig is read through the registered `arig` edge, which has the `studio` setting.
- The end-to-end tests are in `integration/tests/aseq.rs` (the task text said `crates/respak-cli/tests`; slice 18 put its tests in `integration/tests`). `formats_prints_the_graph` needed no change: it builds the engine with `FormatsPlugin` only, which already listed `cast`.
- Test order: the unit tests of `cast`, the `aseq` / `asqd` / `arig` tests were written first and seen failing (`todo!()`). `anim_codec` was written together with its tests (three test mistakes fixed on the first run), and the end-to-end tests came after the encoder.

## Pilot check (2026-10-15, one real file, 0 chunks downloaded: everything came from the cache)
`paks/Win64/common.rpak/0x00006FD412AF3101.aseq`, asset version **14**, a first-person arms idle (`ptpov_mythic_r301_idle...`), 2 blends (201 and 191 frames, 30 fps, looping, additive). `--rig auto` took the `arig` v7 (guid 0x9a9af9cee75e42d3, 181 bones) in about 75 s (it scans the arig headers; `--cache` would remember it). Output: 2 files, each 1 `anim`, 181 bones, 724 curves (4 per bone: `tx ty tz rq`), `kb` per frame, mode `additive`; 204 curves vary, 520 are constant; all quaternions unit length; positions up to 0.73 / 1.05 units, rotations small except one tendril bone. Header, seqdesc v18, animdesc v19.1, 6-bit flags and the RLE tracks decoded without `Corrupt`. `root_fixup` `rsx` turns the root bone to exactly (0, 0, -0.7071, 0.7071) and `none` leaves identity. `dataExtra` came from the **page pointer** (a `.stream` also exists, not needed). Sections: stall 16, 62 frames each, 3 external (`dataExtra`) + 1 `asqd`.
- **Bug found and fixed:** the last frame was read from the trailing section (table entry `(n - stall - 1) / sf + 1`). rsx's `Stall_2` (the one used for 12.1+) has no such case, so the last frame is found like any other. Changed in `AnimDesc::locate`; the unit test and the stall/external end-to-end test were updated (synthetic only).

## Not verified yet
- Only one aseq (14) was opened: 12.1 and 13 headers, the 4-bit width for 12.1, and the 12 / 12.1 pak date cut-off. The pilot is a delta animation, so the absolute path (rest pose as base) and a non-additive Cast were not checked on real data.
- The stall = 0 case, and whether the trailing section really is never needed (the last frame differs from it by about 0.0005 in the pilot).
- Frame movement: the pilot has none (`framemovementindex` unset), so the layout is still unchecked. Previously: the layout (section offsets relative to the struct, four tracks per section, accumulated values, yaw in degrees) is read from the notes' summary; the structs were not read in rsx. A wrong layout shows as `Corrupt` or as a drifting root.
- Autolayer entries are assumed to be 0x20 bytes like `mstudioautolayer_v8_t`; a table that does not fit is ignored. Effects guids are not listed.
- The starpak path of `dataExtra` has no test (the pilot used the page pointer).
- The reverse index took about 75 s on the real `common.rpak` with a warm chunk cache and no `--cache`; whether the rig is the same skeleton as the model's is the demo.
- One tendril bone (`def_b_03_right`) holds a 162 degree delta rotation on frames 0 to 139 and identity afterwards (the later sections carry no data for it); looks authored, but not compared with rsx.
- Blender import: skeleton and root orientation with `root_fixup` `rsx` vs `none`, loop and fps, keys on every frame (the `nodeSize` convention and the node and property names are rsx's, unconfirmed).
- The `arig` studio heuristic worked on the pilot's arig v7 (no options); other variants and the sibling `mdl_` rule are unchecked.

## Open from the format notes
Bone-flag width for aseq 7 to 11 (out of scope with the version limit), real mapping of aseq 13/14 to model 19.3/20 (checked on the pilot), whether an arig's bones always match its model's (checked on the pilot), Titanfall 2 sequences (`source-mdl`, not planned).

## Tests (agreed)
Agreed at the gate (see aseq.md "Tests" for the synthetic cases): detection and `UnsupportedVersion` cases, track chains and block seek, 4-bit / 6-bit flag arrays, `asqd` + stall / external sections, `Quaternion64` / `Vector48` known values, bone-0 fix-up (`rsx` vs `none`), additive weights, Cast curve nodes (`kb` width at 255 / 256 / 65536 frames), missing rig / missing asqd / `sectionframes = 0` / `numframes = 1` / sanity-check failure, random bytes never panic.

## Leaves out
Aseq versions below 12.1, datapoint compression, events, IK, autolayers, blend-grid parameters, frame-movement beyond the bone-0 fold, SMD / QC / RMAX, raw `.rseq` dump, constant-curve collapsing, `anir` conversion, Titanfall 2 sequences.

## Demo
The user converts an animation of a pilot or weapon, imports it with the matching model in Blender (skeleton lines up, root orientation with `root_fixup` `rsx` vs `none`, loop and fps) and compares frame counts and a few bone transforms with rsx.
