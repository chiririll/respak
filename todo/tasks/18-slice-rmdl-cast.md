# 18 · Slice: rpak model (`mdl_`) → Cast
**Status:** in-progress (built and tested on synthetic data and one real v20 model; Blender import check and the demo pending) · **Depends on:** 13 (`matl`), `txtr`, `png` edge · **See:** PROJECT.md §5.5, §6, §12, [`../formats/rmdl.md`](../formats/rmdl.md)

## Goal
An Apex model opens from an rpak and converts to Cast, one file per (LOD, bodypart model), with its skeleton, materials and textures, for Blender and other Cast importers:
```
respak convert paks/Win64/common.rpak/mdl/.../x.mdl_ --to '{"type":"cast"}'
```
Output is a composite (own folder, PROJECT.md §12): `x/<bodypart>_<i>_LOD<n>.cast` + `x/textures/*.png`. A model with no LODs gets a skeleton-only `x/x.cast`. (The `.mdl_` extension is a gate question.)

## Decided already (user, 2026-10-15)
- **Granularity:** one `.cast` per (LOD, model), as rsx does. The `lods` option is `0` (default), `all` or a list.
- **Cast writer** lives in `respak-formats` (common format, next to `png`/`dds`), so `aseq` animations can reuse it later.
- **LOD is chosen in the request** (the `lods` option above).
- **Versions:** the user has no games, so the target is the **latest Apex** version only (16–20 as listed in the scope; older majors → `UnsupportedVersion`).
- **Skins:** an option picks the skin family (default 0).
- **Metadata:** an option exports **all metadata of the model** (header, bones, bodyparts/models/meshes, materials, skins, LODs, hitboxes, attachments, dependencies) as JSON next to the Cast files. Exact shape is settled in the sketch.
- **Real files:** `common.rpak` is cached for `--steam 1172470 --depot 1172471`; the user permitted downloading **one pilot model** for testing (2026-10-15). No other Steam runs.
- **Checked (2026-10-15, from the cache, 0 chunks downloaded):** `list paks/Win64/common.rpak` gives 180958 entries, 4784 of them `mdl_`, all named by guid only (`0x….mdl_`). `discover` on `0x0000E8F3FC35BDD2.mdl_` reports `rpak.asset`, kind `mdl_`, **version 20**. So the latest depot's models are asset version 20 (studiohdr `v19_2` layout, to be confirmed on real bytes); versions 16–19 stay in scope by the spec but cannot be checked on real data. Pilot choice (a small static prop) is made once the reader can print a model's counts. `copy` cannot dump an asset yet (backlog: raw dump), so the pilot data is read through the volume during the build; the chunks it needs are fetched then, within the permission above.

## Proposed defaults (agree or change at the gate)
- `Mesh` and `Skeleton` capabilities are defined in this slice, with the Cast encoder as their first consumer.
- Cast details are written as rsx writes them (raw UVs, `nodeSize` including its header, `v2/v3/v4` ids); verified in the demo by importing into Blender. If an importer disagrees, that is a fix in this slice.
- Cast typing stays narrow like rsx (`b`/`h` bone indices, `h` face indices). A mesh over 65535 vertices is an error, not a silent wrap.
- Blend shapes, physics, animations: out. Hitboxes appear in the metadata JSON only, not as geometry.

## Proposed scope
- Plugin: `respak-respawn` (new `rmdl.rs`) + `respak-formats` (new `cast.rs`) + `respak-core` (`Mesh`, `Skeleton`). No plugin-to-plugin dependency.
- Versions **16–20** (rev4 VG): studiohdr v16, v17 (v17/v18/v19.0/v19.1) and v19.2 (v19.2/v19.3/v20). Older majors → `UnsupportedVersion` for now (follow-up per rev2/rev3/rev1).
- Detection: score on `rpak.asset` kind `mdl_`; `Version { major: entry.version, minor }` where the 19.x minor needs the pak `createdTime`, so `PakHeader` gains that field.
- Data: bones (split form for v16–18, linear-bone form for v19+), bodyparts → models → meshes, material guids, skin 0, VG from the mandatory starpak or `staticStreamingData`, per-group decompression (NONE / RTech / Snowflake / Oodle), vertex decoding (`Vec3`, `Vector64`, packed blend + extra weights, `Normal32`, colour, UVs).

## rsx review
**What it does** (details and offsets in rmdl.md): `model.cpp` (`GetModelVersionFromAsset`, `LoadModelAsset`, `ExportModelStreamedData`); `studio_r5_v16.h` and `studio.h` (structs, `FIX_OFFSET`, VG rev4); `core/mdl/modeldata.cpp` (`ParseModelVertexData_v16`, `ParseVertexFromVG`, `ExportModelCAST`, `ParseMaterialIntoCast`); `core/mdl/cast.cpp` (writer); `compressedvector.h` (`Vector64`, `Normal32`).

**Problems** (kept out of the port):
- Unbounded pointer reads in detection and parsing → bounded reads, `Corrupt`.
- Whole-asset in-memory export with global settings (`exportModelSkin`, `exportMaterialTextures`, …) → target options.
- Bone-state default map of 8 entries read out of bounds → identity map when there is no table.
- Texcoord loop tests a constant instead of the mesh flags; normal/tangent flag bits never inspected; `offset == vertCacheSize` only a debug assert → hard stride check, error on a layout we don't know.
- `Vector48` positions returned as zero → `UnsupportedVersion`.
- OODLE failure treated as "uncompressed" without copying → error.
- Cast bone-weight buffer indexing wrong for more than 256 bones; no dedupe of `matl` nodes → per-mesh buffers, dedupe by guid.
- Textures exported inside material "preparation", concurrently for models sharing materials → the linking layer dedupes.
- Material table built from LOD 0 meshes only (assert for others) → built from all exported meshes.
- Approximate `FastRSqrtFast` normalisation → exact.

**Fit:**
| rsx | respak |
|---|---|
| version detection | score function on `rpak.asset`, `Version` from the table; `createdTime` added to `PakHeader` |
| studiohdr / VG parsing | `rmdl.rs`, ranged reads (`read_mem`, `Starpak::entry`), per-group decode on demand |
| `ModelBone_t`, parsed LOD/model | `Skeleton`, `Mesh` capabilities |
| Cast writer | `respak-formats::cast` (node/property builder → bytes) |
| `ExportModelCAST` + material export | `rmdl → cast` encoder; textures through the mediator (`matl` → `txtr` → `png`), written under `textures/` |

**Behaviour changes against rsx** (for the demo comparison): LOD files for models with zero meshes are skipped; `matl` nodes are deduped; mesh names use one guid format (hex); more than 256 bones are written correctly; unknown vertex layouts fail instead of being misparsed.

## Sketch (written 2026-10-15, compiles, bodies are `todo!()`)
Not yet registered in the plugins (registering changes `formats_prints_the_graph`; that happens at build, as in 16). Only unused-variable/dead-code warnings fire.
- `respak-core::capability::skeleton`: `Bone { name, parent, position, rotation [x,y,z,w], scale }`, `Skeleton::bones()`.
- `respak-core::capability::mesh`: `Mesh` (async, lazy): `info() -> MeshInfo { models, lod_count, skin_count }`, `read(lod, model, skin, cx) -> MeshModel { parts: Vec<MeshPart> }` (one part per material: positions, normals, colours, UV layers, `influences` bone indices/weights per vertex, triangles in engine winding), `materials(skin, cx) -> Vec<MeshMaterial { guid, name, textures: Vec<MaterialTexture { slot, guid, texture: Option<Value> }> }>`. `Cap` impls for `Mesh` and `Skeleton` added.
- `respak-formats::cast`: `CastWriter` (`model`, `material`, `mesh`, `finish`), `CastOptions { lods: all | n | [n..], skin, textures, image }`, `CastEncode` (`Mesh -> cast`, composite output). It never needs `respak-respawn`: the material and its located textures come through the `Mesh` capability.
- `respak-respawn::rmdl`: `RmdlHeader::parse` (asset header + CPU data + variant), `StudioLayout { V16, V17, V19_2 }`, `RmdlAsset: Data + Mesh + Skeleton + Metadata`, `RmdlRead` (`rpak.asset -> rmdl`), decoders (`unpack_vector64`, `unpack_normal32`, `fix_offset`, `unpack_weight`), `RmdlAsset::describe` (all metadata as JSON).
- `respak-respawn::rmdl_json`: `RmdlJsonEncode` (`rmdl -> rmdl.json`).

Design points this sketch decides (shout if wrong):
- **Materials through the capability.** The Cast encoder is generic (`Mesh -> cast`), so the textures reach it as ready `Bitmap` values inside `MeshMaterial`. The pak lookups (`matl` by guid, shader slot names, `txtr`) stay in `respak-respawn`, which reuses the `matl_json` code. No plugin-to-plugin dependency.
- **Metadata is a separate target `rmdl.json`**, like `matl.json`, not a flag on `cast` (a Cast writer cannot know pak structures). Ask for both in one `run` request to get them together.
- **Format name `rmdl`**, volume extension `.mdl_`.
- `PakHeader` will need `created_time` for 19.x; it is not added in the sketch.

## Gate answers (user, 2026-10-15)
1. Versions: latest Apex only (no games owned); v20 is the main target.
2. Extension `.mdl_`, 19.x date heuristic accepted, test list agreed (answered "yes" to the proposed defaults).
3. Skins: an option picks the family. Metadata: an option exports everything (target `rmdl.json`, see above).
4. Blend shapes, `Vector48`, `VERT_UNK`: backlog.
5. Real files: the cached latest-depot `common.rpak`, one pilot model.

## Built (2026-10-15)
- `respak-core`: `Mesh` / `Skeleton` capabilities as sketched (`Cap` impls added).
- `respak-formats::cast`: `CastWriter`, `CastEncode` (`Mesh -> cast`), `CastOptions`. Registered as format `cast`. 9 unit tests.
- `respak-respawn::rmdl` (`rmdl`, extension `mdl_`) and `rmdl_json` (`rmdl.json`): asset header + CPU data, studiohdr for the three layouts (v16, v17, v19_2), bones (split and linear form), body parts/models/meshes, material table, skins, LODs, vertex groups with all four codecs, rev4 vertex decoding (`Vec3`/`Vector64`, packed and extra-weight blends, `Normal32`, colour, UV layers), materials located through the mediator. 16 unit tests; `PakBuilder::rmdl` and `testing::rmdl` fixtures; 7 end-to-end tests in `respak-cli/tests/rmdl.rs`.
- `PakHeader::created_time` (offset 0x08) and `RpakAsset::pak_created_time` for the 19.x variants.
- Docs: README and `docs/cli.md`.
- Real data (cached latest depot, nothing downloaded for the metadata, one 800 KB chunk for the geometry): `paks/Win64/common.rpak/0x0000E8F3FC35BDD2.mdl_` (asset version 20, `hspray_epic_v21_valkyrie_08`, a flat card) gives `rmdl.json` and four LOD `.cast` files. The hand-computed v19_2 offsets matched: 3 bones, attachments, 1 body part, 1 material, 4 LODs, one Oodle group (203 → 1264 bytes), 8 vertices and 12 indices per LOD, positions/normals/UVs consistent with the bounds, front/back faces with opposite normals.

Findings that differ from the spec:
- `VERT_UNK` (0x40) is set on real meshes and adds **no** bytes (rsx: "flag gets ignored"); with it the stride was 28 = position 8 + blend 8 + normal 4 + UV 8. It is accepted; the stride check still guards the layout.
- UV layers are read in ascending slot order (the spec's "slots 0, 2, 3" case), not required to be contiguous.
- Cast node hashes are FNV-1a of the name, not rsx's `StringToGuid` (a hash only has to be unique inside the file).

## Not verified yet
- The Cast files have not been imported into Blender (`nodeSize` convention, `v2/v3/v4` ids, UV orientation are rsx's, unconfirmed).
- Only one model has been decoded for real: no skinned character, no extra bone weights, no multi-body-part weapon, no streamed (starpak) vertex data, no Snowflake/RTech groups. Versions 16-19 have only synthetic tests.
- Textures: `matl` slot names fail on the real shader of this model (`DXBC: no RDEF chunk`, the backlog's "SM5.1 DXBC"), so Cast material slots stay unlinked; the textures are not exported in the pilot run.
- `cargo clippy --workspace --all-targets -- -D warnings` fails only on the existing `len_without_is_empty` in `respak-core/src/capability/blob.rs` (not touched); with that one lint allowed it is clean.

## Tests (agreed)
Unit, synthetic fixtures built in test code, no game files:
- **Detection:** v16 / v17 / v19.0 / v19.2 / v20 by header size and `sourceFilenameOffset`; 19.x by `createdTime`; unknown major (21) → `UnsupportedVersion`; wrong kind not scored; header shorter than the struct → `Corrupt`.
- **Offsets:** `FIX_OFFSET` for both bit-0 parities; offsets past the studiohdr length → `Corrupt`.
- **Skeleton:** two bones (split form), then the v19 linear-bone form; names, parents (-1 root), pos/quat; more than 256 bones.
- **Vertices:** `Vector64` known values (`x = 0` → -1024, max); `Normal32` axis-aligned normals (all three dropped-component indices, both signs); blend with 1, 2, 3 weights and with extra weights (sum ≈ 1); colour and two UV slots; a `vertCacheSize` that doesn't match the layout → error; non-contiguous texcoord slots → error; `Vector48` → `UnsupportedVersion`.
- **VG:** one group / one LOD / two meshes; empty mesh (`flags == 0`) skipped; each compression type (NONE; RTech/Snowflake/Oodle with a small sample where one exists); `staticStreamingData` path; missing starpak entry → `MissingDependency` with the skeleton still readable; no LODs → skeleton only.
- **Materials:** guid table, skin 0 mapping, a missing `matl` → stub material named by the guid.
- **Cast writer:** header (`cast`, version 1), node header 24 bytes and `nodeSize`, property encoding for every type, strings, child order; a round trip through a small test reader; triangle winding reversed; one `matl` node for a material used by two meshes.
- **End to end (synthetic):** fake rpak + starpak → `convert … --to cast` writes the expected files, composite folder, cancel mid-way gives `Cancelled`.
- Random bytes after a valid header never panic.

Real files (env vars, skipped with a message when unset, never committed): one static prop, one character with extra bone weights, one weapon with several bodyparts; counts of bones, vertices and indices compared with rsx's Cast export.

Always: clippy `-D warnings`, `cargo test --workspace`, `cargo tree -p respak-respawn` / `-p respak-formats` show no plugin-to-plugin dependency, public items documented.

## Leaves out
Versions below 16; hitboxes, physics (STL), SMD/QC/RMAX, raw `.rmdl` dump, animations (`arig`/`aseq`), blend shapes, skins other than 0, tangents in Cast, `Vector48`.

## Demo
The user converts a prop, a character and a weapon to Cast, imports them in Blender (skeleton, weights, UVs, textures) and compares counts with rsx exports.
