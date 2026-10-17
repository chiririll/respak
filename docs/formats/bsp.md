# bsp · Respawn BSP map (`rBSP`) and entity files
**rsx sources:** `game/bsp/bsp.h` (`lump_t`, `BSPHeader_t`, `dmodel_t`, `dmesh_t`, `dmaterialsort_t`, `dtexdata_t`, `dportal_t`, `CBSPData`), `game/bsp/bsp.cpp` (`PopulateFromPakAsset`, `ConstructPreviewData`, `Export`, `ParseModelFlagsFromEntities`, vertex stride helpers), `game/bsp/entities.cpp/.h` (`BSP_ParseEntities`, `BSPEntity_s`), `game/rtech/utils/bsp/lumps.h` (`lumptype_e`, `s_LumpNames`), `game/bsp/bvh.h` and `game/rtech/utils/bvh/bvh.h` (BVH structs, not used by `CBSPData`), `game/rtech/assets/wrap.cpp:9-16, 55-118, 153-189, 245-264, 323-342` (wrap hooks), `game/vpk/vpk.h:9-23` (file type table), `core/features.h:10` (`HAS_BSP_SUPPORT`), `core/render/dx.h:67-76` (mesh type flags) · **Games / versions:** Apex Legends only (structs "all based on apex for now", `bsp.h:40`); Titanfall 1 / Titanfall 2 are **not** handled by rsx · **Priority hint:** medium value (map geometry and entity lists), **L** if full geometry is wanted, **S** for "give the user the raw lumps and `.ent` files".

> **Scope warning.** rsx does not read BSP files from disk or from a VPK. It only parses a BSP that is wrapped inside an Apex rpak (a `wrap` asset, see [wrap.md](wrap.md)), with the lumps stored as separate wrap assets. Everything about Titanfall 1/2 headers, LZMA lump compression, game lumps, static props and the `snd`/`spawn` entity files is **not in rsx**; it is marked "unverified / community knowledge". The rsx BSP code is also explicitly unfinished (`// todo`, `// very temp`, `// structs are all based on apex for now`).

## Summary
A Respawn map is a Source-engine-derived BSP (`rBSP`): a header with 128 lump slots, each lump a typed array (planes, vertices, meshes, BVH collision, lightmaps, cells and portals, ...). In Apex the lumps are not inside one file: the `.bsp` wrap holds only the header, and each non-empty lump is its own wrap asset (`maps/<map>.bsp.<lump>.bsp_lump`). Entities live in separate text files `maps/<map>_<vm>.ent`. rsx uses this to (1) preview the world geometry and (2) export an **OBJ** (positions, normals, faces) into a file named `*.bsp`. A user would want: the individual lumps as blobs, the entity lists as text/JSON, a re-assembled `.bsp`, and the visible geometry as a mesh.

## Where it lives
- **Apex (rpak):** wrap assets (`wrap` asset type, FourCC `wrap`, see wrap.md). Three name families, all with the first folder stripped by the wrap loader (`wrap.cpp:45-49`), and the VM extension (`.client`, `.server`, `.ui`) still in the stored name:
  - header file: `maps/<map>.bsp` (+ VM extension, probably `.client`: unverified). Type = BSP by extension after stripping the VM extension (`wrap.cpp:163-172`).
  - lumps: `maps/<map>.bsp.<IIII>.bsp_lump.client`, where `<IIII>` = lump index as **4 digit upper-case hex** (`std::format("{:04X}")`, `bsp.cpp:316`). The `.client` suffix is hard-coded: server/ui copies of lumps are never read.
  - entities: `maps/<map>_env.ent` and `maps/<map>_script.ent` (`bsp.cpp:288-295`), looked up **without** a VM suffix (see quirks, this may never match).
  - rsx's `<map>` is the file stem of the BSP asset name (`wrap.cpp:181`), e.g. `mp_rr_canyonlands_64k_x_128k`.
  - Names are stored (wrap path), never hashed.
- **Titanfall 1 / 2 (VPK):** `maps/<map>.bsp` is a file in a Respawn VPK (see [vpk.md](vpk.md)); rsx never reads it. Format details below for those games are community knowledge only. TF2 also ships `.ent` files next to the BSP (`maps/<map>_env.ent` etc.), unverified.
- No FourCC of its own: a `.bsp` is a `wrap` rpak asset. rsx's `VPKFileType_e::BSP` is a file-type tag (`vpk.h:12`), `CBSPData` is stored in `WrapAsset::parsedData` (`wrap.cpp:184`).
- No streamed data: the lumps are separate wrap assets and may themselves be compressed/streamed (wrap.md "Data outside the header").

## Versions and detection
- rsx reads `header->version` into `m_version` (`bsp.cpp:303`) and **never uses it again**. No `ident` check, no version switch, no lump `version` use. Only `header->flags & 1` is tested.
- Detection in rsx is by file extension `.bsp` inside a `wrap` asset (`vpk.h:17`).
- **Proposed score (`blob → bsp`)**: `u32le(0) == 0x50534272` (bytes `72 42 53 50`, "rBSP") → 10; `u16le(4)` in a known set (unverified: TF1 29, TF2 37, Apex 47-50; community) → +2; `len >= 0x810` → +1; extension `bsp` → +1. rBSP with unknown `version` → `UnsupportedVersion`. Mapping: `Version { major: version, minor: flags }` (flags bit 0 changes the storage model, so it deserves to be in the version). Only the Apex layout (external lumps, `flags & 1`) is described by rsx; header prefix: 0x810 bytes.
- Valve `VBSP` (`ident = 0x50534256`) is out of scope (project.md §2).
- In TF1/TF2 `flags` does not exist as a field (community: the third dword is `mapRevision`, the version is a 32-bit int, the lump count is 127 and the lump struct holds a FourCC at +0x0C instead of `uncompLen`). Unverified; see Open questions.

## Layout
All little-endian, natural alignment. `Vector` / `float3` = 3 x f32 (12 bytes; assumed, `Vector` is defined elsewhere).

### `BSPHeader_t` (`bsp.h:23-32`, 0x810 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | ident | `'rBSP'` (comment only, not checked) |
| 0x04 | 2 | u16 | version | BSP version; stored, unused |
| 0x06 | 2 | u16 | flags | bit 0 = lumps are stored outside this file (separate wrap assets). Other bits unknown ("surely flags and not just a bool right?") |
| 0x08 | 4 | i32 | mapRevision | map revision; unused |
| 0x0C | 4 | i32 | lastLump | highest lump index in use (rsx iterates `i < lastLump`, so the last slot itself is not visited; see quirks) |
| 0x10 | 0x800 | `lump_t[128]` | lumps | `NUM_BSP_LUMPS = 0x80` (`bsp.h:13`) |

### `lump_t` (`bsp.h:15-21`, 16 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | fileofs | offset in the BSP file; **unused by rsx** (lumps are external). In an inline BSP this would be the data offset (unverified) |
| 0x04 | 4 | i32 | filelen | lump size in bytes; 0 = empty / absent |
| 0x08 | 4 | i32 | version | lump struct version; unused (rsx has no per-lump versions) |
| 0x0C | 4 | i32 | uncompLen | uncompressed length; unused. Hint: non-zero when the lump is compressed (unverified; rsx has no LZMA code, compression is the wrap's Oodle) |

### Lump ids (`lumps.h:3-135`)
128 ids `0x00..0x7F`. Names from `lumptype_e`; "struct" is what rsx actually interprets (only lumps listed in "rsx use" are read, all others are opaque blobs in rsx). `UNUSED_n` = not used by Apex per the enum (rsx never reads them).

| Id | Name | rsx use / struct |
|---|---|---|
| 0x00 | ENTITIES | not read (entities come from `.ent` files) |
| 0x01 | PLANES | - |
| 0x02 | TEXDATA | `dtexdata_t[]` (16 B), used for material names |
| 0x03 | VERTEXES | `float3[]` positions |
| 0x04 | LIGHTPROBE_PARENT_INFOS | - |
| 0x05 | SHADOW_ENVIRONMENTS | - |
| 0x06-0x0D | UNUSED_6..13 | - |
| 0x0E | MODELS | `dmodel_t[]` (64 B) |
| 0x0F | TEXDATA_STRING_DATA | NUL-terminated strings, indexed by byte offset |
| 0x10 | CONTENTS_MASKS | - |
| 0x11 | SURFACE_PROPERTIES | - |
| 0x12 | BVH_NODES | collision BVH `dbvhnode_t` (see below), not parsed for the BSP |
| 0x13 | BVH_LEAF_DATA | - |
| 0x14 | PACKED_VERTICES | - (uint16 packed vertices for BVH primitives, community) |
| 0x15-0x17 | UNUSED_21..23 | - |
| 0x18 | ENTITYPARTITIONS | not read (community: list of entity file groups) |
| 0x19-0x1D | UNUSED_25..29 | - |
| 0x1E | VERTNORMALS | `float3[]` normals |
| 0x1F-0x22 | UNUSED_31..34 | - |
| 0x23 | GAME_LUMP | not read |
| 0x24 | UNUSED_36 | - |
| 0x25 | UNKNOWN_37 | "connected to VIS lumps" |
| 0x26 | UNKNOWN_38 | "connected to CSM lumps" |
| 0x27 | UNKNOWN_39 | "connected to VIS lumps" |
| 0x28 | PAKFILE | "zip file, contains cubemaps"; not read |
| 0x29 | UNUSED_41 | - |
| 0x2A | CUBEMAPS | - |
| 0x2B | UNKNOWN_43 | - |
| 0x2C-0x35 | UNUSED_44..53 | - |
| 0x36 | WORLD_LIGHTS | - |
| 0x37 | WORLD_LIGHT_PARENT_INFOS | - |
| 0x38-0x46 | UNUSED_56..70 | - |
| 0x47 | VERTS_UNLIT | vertex lump, stride 20 |
| 0x48 | VERTS_LIT_FLAT | vertex lump, stride 20 |
| 0x49 | VERTS_LIT_BUMP | vertex lump, stride 32 |
| 0x4A | VERTS_UNLIT_TS | vertex lump, stride 24 |
| 0x4B | VERTS_BLINN_PHONG | not handled (no stride, no mesh type) |
| 0x4C-0x4E | VERTS_RESERVED_5..7 | - |
| 0x4F | MESH_INDICES | `u16[]` |
| 0x50 | MESHES | `dmesh_t[]` (28 B) |
| 0x51 | MESH_BOUNDS | - |
| 0x52 | MATERIAL_SORT | `dmaterialsort_t[]` (12 B) |
| 0x53 | LIGHTMAP_HEADERS | - |
| 0x54 | UNUSED_84 | - |
| 0x55 | TWEAK_LIGHTS | - |
| 0x56-0x60 | UNUSED_86..96 | - |
| 0x61 | UNKNOWN_97 | - |
| 0x62 | LIGHTMAP_DATA_SKY | - |
| 0x63 | CSM_AABB_NODES | - |
| 0x64 | CSM_OBJ_REFS | - |
| 0x65 | LIGHTPROBES | - |
| 0x66 | STATIC_PROP_LIGHTPROBE_INDEX | - |
| 0x67 | LIGHTPROBETREE | - |
| 0x68 | LIGHTPROBEREFS | - |
| 0x69 | LIGHTMAP_DATA_REAL_TIME_LIGHTS | - |
| 0x6A | CELL_BSP_NODES | - |
| 0x6B | CELLS | - |
| 0x6C | PORTALS | `dportal_t` (12 B) struct exists (`bsp.h:83-93`), **never used** |
| 0x6D | PORTAL_VERTS | - |
| 0x6E | PORTAL_EDGES | - |
| 0x6F | PORTAL_VERT_EDGES | - |
| 0x70 | PORTAL_VERT_REFS | - |
| 0x71 | PORTAL_EDGE_REFS | - |
| 0x72 | PORTAL_EDGE_ISECT_EDGE | - |
| 0x73 | PORTAL_EDGE_ISECT_AT_VERT | - |
| 0x74 | PORTAL_EDGE_ISECT_HEADER | - |
| 0x75 | OCCLUSIONMESH_VERTS | - |
| 0x76 | OCCLUSIONMESH_INDICES | - |
| 0x77 | CELL_AABB_NODES | - |
| 0x78 | OBJ_REFS | - |
| 0x79 | OBJ_REF_BOUNDS | - |
| 0x7A | LIGHTMAP_DATA_RTL_PAGE | - |
| 0x7B | LEVEL_INFO | - |
| 0x7C | SHADOW_MESH_OPAQUE_VERTS | - |
| 0x7D | SHADOW_MESH_ALPHA_VERTS | - |
| 0x7E | SHADOW_MESH_INDICES | - |
| 0x7F | SHADOW_MESH_MESHES | - |

Lump versions: rsx records none. Struct layouts below are for the Apex versions the author had, no version number is attached (unknown).

### `dmodel_t` (`bsp.h:42-49`, 64 bytes), LUMP 0x0E
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 12 | float3 | mins | bounds |
| 0x0C | 12 | float3 | maxs | bounds |
| 0x18 | 4 | i32 | firstMesh | index into MESHES |
| 0x1C | 4 | i32 | meshCount | |
| 0x20 | 32 | i32[8] | unk | unknown |

Model 0 is the world; models `1..` are brush entities, referenced from `.ent` files as `"model" "*N"`.

### `dmesh_t` (`bsp.h:68-81`, 28 bytes), LUMP 0x50
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | firstIdx | first index in MESH_INDICES |
| 0x04 | 2 | i16 | triCount | triangles; `<= 0` skipped |
| 0x06 | 2 | i16 | firstVtxRel | unused |
| 0x08 | 2 | i16 | lastVtxOfs | unused |
| 0x0A | 1 | i8 | vtxType | declared, but rsx takes the type from `flags` instead |
| 0x0B | 1 | i8 | unused_0 | |
| 0x0C | 4 | i8[4] | lightStyles | unused |
| 0x10 | 4 | i16[2] | luxelOrg | lightmap origin; unused |
| 0x14 | 2 | i8[2] | luxelOfsMax | unused |
| 0x16 | 2 | i16 | mtlSortIdx | index into MATERIAL_SORT |
| 0x18 | 4 | i32 | flags | `flags & 0x600` selects the vertex lump (below) |

Vertex type flags (`bsp.h:35-38`): `0x000` LIT_FLAT, `0x200` LIT_BUMP, `0x400` UNLIT, `0x600` UNLIT_TS. Mesh vertex type → lump: LIT_FLAT → 0x48, LIT_BUMP → 0x49, UNLIT → 0x47, UNLIT_TS → 0x4A (`bsp.cpp:183-207`).

### `dmaterialsort_t` (`bsp.h:51-58`, 12 bytes), LUMP 0x52
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 2 | i16 | texdata | index into TEXDATA |
| 0x02 | 2 | i16 | lmapIdx | lightmap; unused |
| 0x04 | 2 | i16 | cubemapIdx | unused |
| 0x06 | 2 | i16 | lastVtxOfs | unused |
| 0x08 | 4 | i32 | firstVertex | added to every index of the meshes using this sort entry |

### `dtexdata_t` (`bsp.h:60-65`, 16 bytes), LUMP 0x02
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | nameStringTableID | **byte offset** into LUMP 0x0F (rsx indexes the string lump directly, `bsp.cpp:568`; in Valve BSPs this is an index into a table, so the name of the field differs from its use here) |
| 0x04 | 4 | i32 | width | unused |
| 0x08 | 4 | i32 | height | unused |
| 0x0C | 4 | i32 | flags | unused |

### Vertex lumps 0x47-0x4A
Records of `stride` bytes. The first two u32 are **indices**: `posIdx` into LUMP 0x03 (float3 array) and `nmlIdx` into LUMP 0x1E (float3 array). The remainder (derived from the D3D input layouts, `bsp.cpp:25-134`, and comments; field names are rsx's, not confirmed):
| Lump | Stride | After the two indices |
|---|---|---|
| 0x47 UNLIT, 0x48 LIT_FLAT | 20 | `uv f32[2]`, `unk u32` |
| 0x4A UNLIT_TS | 24 | `uv f32[2]`, `unk u32[2]` |
| 0x49 LIT_BUMP | 32 | layout list: `uv f32[2]`, `colour u32`, `lmapUV f32[2]`, `unk u32`. An earlier comment in the same function orders it `uv, unk(-1), lmapUV, colour`; the two orders disagree, which is right is **unknown** |

`GetVertexStrideByLumpId`'s comment: `20 + (0x40c >> (4 * (lumpId & 3)) & 0xF)` computes the same strides from the game's own code (`bsp.cpp:136`).

Index buffer: `u16 MESH_INDICES[firstIdx .. firstIdx + triCount*3]`; actual vertex index = `indices[k] + mtlSort.firstVertex`; triangles are plain lists.

### Entity file (`.ent`), text
```
ENTITIES01                       // or "ENTITIES02 num_models=28": whole first line skipped, must start with "ENTITIES"
// comments to end of line
{
"key" "value"
...
}
{ ... }
```
Strings are `"` ... `"` with **no escape handling** (first closing quote ends the string). Whitespace and `//` comments between tokens are skipped. Result: `BSPEntity_s { vector<pair<string,string>> keyValues }` in file order; duplicates allowed.

### BVH structs (collision, `game/bsp/bvh.h`)
`dbvhnode_t` (LUMP 0x12): three `dbvhaxis_t` (8 x i16 each: min/max for 4 children) = 0x30 bytes, then four u32 bitfields: `{cmIndex:8, index0:24}`, `{pad:8, index1:24}`, `{child0Type:4, child1Type:4, index2:24}`, `{child2Type:4, child3Type:4, index3:24}` = 0x40 bytes total. Child types: 0 NODE, 1 NO_CHILD, 2 INVALID, 3 UNK_3 (subleaf list), 4 PRIM_TRI, 5 PRIM_TRI_PACKED, 6 PRIM_QUAD, 7 PRIM_QUAD_PACKED, 8 UNK_8, 9 STATIC_PROP. Bitfield packing assumes MSVC low-bit-first. Helper structs (`dbvhleaf_poly_t`, `dbvhsubleaf_t`, `dbvhtype8_t`, `dbvhtri_t` 11/9/9/3 bits, `dbvhquad_t` 10/9/9/4 bits) are declared there. **`CBSPData` does not use any of this**; the same structs are duplicated in `game/rtech/utils/bvh/bvh.h` (with `CollisionModel_t`, `exportSTL/OBJ`), which is used for studio-model collision (`model.cpp`, `bvh.cpp`). Out of scope for bsp except as lump 0x12/0x13/0x14 background.

## Data outside the header
- **Lumps (Apex, `flags & 1`):** one wrap asset per non-empty lump (`filelen != 0`), read with `GetWrapAssetData` (`wrap.cpp:75-118`: starpak/page data, optional Oodle). The BSP's own `fileofs` is not used. The decoded size should equal `lump.filelen`; rsx does not check (it records `filelen` as size).
- **`flags & 1 == 0`:** nothing is loaded; no inline lump reader exists in rsx. The lumps would be at `fileofs .. +filelen` of the same file (unverified); `uncompLen`/compression behaviour unknown.
- **Compression:** rsx has no BSP-specific compression. LZMA lump compression in Titanfall BSPs (community: lump compressed with a Valve `LZMA` header and `uncompLen` set) is **not in rsx**; unverified.
- **Entities:** two text files (above). They are wrap assets with TEXT type: trailing NUL kept in memory (`std::string(entData.get())` relies on it).
- **Game lump / static props:** LUMP 0x23 is named but never parsed; static props appear only as BVH child type 9. Format unknown.
- **PAKFILE (0x28):** said to be a ZIP with cubemaps; not parsed.

## Dependencies
- The `.bsp` wrap needs its **lump wrap assets** (`maps/<map>.bsp.<IIII>.bsp_lump.client`), looked up in rsx's global asset list. A missing lump → log line "expected lump ID ... to exist" (`bsp.cpp:322`), geometry then unavailable. In respak this is `MissingDependency { item: <lump asset name>, file: <rpak> }`. They normally live in the same map rpak (`mp_rr_*.rpak`) (assumed from wrap.md; unverified for all maps).
- **Entities:** `maps/<map>_env.ent`, `maps/<map>_script.ent` (optional; used for preview flags only).
- **Materials (preview only):** `material/<texdata name>_wldc.rpak` (`bsp.cpp:569`); first texture of the `matl` (`txtrAssets[0]`), highest RPak-resident mip. Not needed for export or for lump extraction. respak: `rpak.guid` of the material name.
- No guid references inside the lumps.

## Decoding
```
open(bsp wrap bytes):
  h = read BSPHeader_t (0x810 bytes)
  check ident == 'rBSP'            // rsx does not
  if h.flags & 1:
    for i in 0 .. h.lastLump:      // rsx: i < lastLump (see quirks); safer: i < 128
      if h.lumps[i].filelen != 0:
        name = "maps/{map}.bsp.{i:04X}.bsp_lump.client"
        lump[i] = wrap data of name   (or MissingDependency)
  else: inline lumps (unverified)

geometry(model m):
  for mesh in meshes[m.firstMesh .. +m.meshCount] with triCount > 0:
    sort  = materialSort[mesh.mtlSortIdx]
    vtype = mesh.flags & 0x600     -> lump id and stride (table above)
    for k in mesh.firstIdx .. +triCount*3:
      v   = vertexLump[vtype][indices[k] + sort.firstVertex]   // stride bytes
      pos = vertexes[u32 v@0]; nml = vertnormals[u32 v@4]; uv etc. from the rest
  material name = texdata[sort.texdata].nameStringTableID -> NUL string in lump 0x0F

entities(file):
  skip first line (must start "ENTITIES"); then repeat { "k" "v" ... }
```
Model flags for preview (`bsp.cpp:354-390`): for each env entity with both `model` and `classname`, `model` starting with `*` → `stoi(model[1..])` is a model index; `classname == "fog_volume"` → `DMDD_FOGVOL (1<<2)`; classname prefix `trigger_` → `DMDD_TRIGGER (1<<0)` (see quirks, this does not work). Further flags `DMDD_WATER (1<<1)`, `DMDD_EXPVOL (1<<3)` exist but are never assigned (`dx.h:67-70`).

## Output
rsx:
- **Export** (`ExportWrapAsset`, `wrap.cpp:245-264`): writes to `<export>/wrap/<name>` where `<name>` ends in `.bsp` (with VM extension swapped, wrap.md). The content is **Wavefront OBJ text, not a BSP** (`CBSPData::Export`, `bsp.cpp:641-711`): all positions as `v x y z`, all normals as `vn x y z`, then per mesh `f p//n p//n p//n` (1-based indices taken from the first two u32 of each vertex). Missing: UVs, materials, objects/groups per model, index offsets beyond `firstVertex`, any other lump. Only models/meshes drawn through `LUMP_MODELS`/`MESHES`.
- No export of individual lumps, entities (the `.ent` wraps are exported as plain text by the generic wrap export), or a re-assembled BSP. (This contradicts wrap.md, see final notes.)
- **Preview:** D3D11 model with per-vertex-type shaders, mesh-type filters (`Triggers`, `Water`, `Fog Volumes`, `Exposure Volumes` check boxes), textures from `_wldc.rpak` materials.
- No options besides the check boxes; `HAS_BSP_SUPPORT` is a compile-time define (`features.h:10`).

Proposed respak (not implemented; for the gate):
- Format `bsp` (respawn crate): `from("blob", BspRead)`, `provides::<dyn Container>()` presenting a volume `lumps/<IIII>_<NAME>.lump` (+ `header.json`), where for rpak-sourced maps the entries are resolved from the sibling wrap assets (project.md §5.5 linking by name/guid). `Blob` for the whole file only when lumps are inline.
- Optional second target: `Mesh` (new capability) from the six lumps above (positions, normals, indices, vertex lump, models, material names), as OBJ/glTF; materials via `matl` link.
- Entities: separate small format `ent` (text → JSON `[{key: [values]}]` keeping duplicates and order), or leave as `wrap` Blob text.
- A re-assembled `.bsp` (header + lumps at offsets, `flags & 1 == 0`) is new work, **not** in rsx; whether the game accepts it is unknown.

## rsx quirks
- Export writes OBJ into a path named `*.bsp`, in a function commented "very temp" (`bsp.cpp:640`); the loop prints an empty line first (`faceString` initially empty) and ends without a newline.
- `lastLump` loop is `i < lastLump` as `uint8_t i` (`bsp.cpp:310`): if `lastLump` is the highest index the last lump is skipped; if it is the lump count it is fine. Which one the field means: unknown.
- Inline lumps (`flags & 1 == 0`) are not loaded, yet `ConstructPreviewData`/`Export` dereference `GetLumpData(...).get()` unconditionally (null deref on missing lumps; `GetLumpData` returns `nullptr` after logging).
- Lump asset names hard-code `.client`; `.ent` names are looked up without a VM extension while wrap names keep it (`wrap.cpp:47`). Whether `maps/<map>_env.ent` exists as such is unverified; if wrap names always include the VM extension the entity lookup never matches and `hasEntities_*` stay `false`... and are **uninitialised** members (constructor does not init `hasEntities_env/script`, `bsp.h:185-186`; also `m_version`, `l`).
- `ParseModelFlagsFromEntities`: inside the prefix loop it ORs `flagValue` (0 for `trigger_*`) instead of `prefixFlagVal`, so triggers are never flagged (`bsp.cpp:378`). Also `modelVal[0]` on an empty string and `stoi` on garbage can throw.
- `POPULATE_BSP_ENTS`: result of `BSP_ParseEntities` ignored; `GetWrapAssetData` may return nullptr (crash); relies on the NUL terminator.
- `ParseError` logs without newline; error leaves `hasEntities` `true` with empty list.
- `SetLumpData` asserts the lump was not set before. `m_lumpSizes` records `filelen` even when the lump asset is missing.
- `CONVERT_VERT_STRIDE` macro is not parenthesised.
- `Vector`-based `l.numVertPositions` counts from `filelen` of the header, not the decoded lump.
- The only version-ish parts: `dmodel_t` has an `unk[8]` tail, `dmesh_t.vtxType` is ignored for `flags & 0x600`; `LUMP_VERTS_BLINN_PHONG` unhandled (`GetVertexLumpIdByMeshFlag` asserts on other values).
- `bvh.h` duplicated in `game/bsp/` and `game/rtech/utils/bvh/` (same struct names, ODR hazard if both included).
- `dportal_t` and the BVH structs are declared but unused by the BSP code.
- Preview: `meshTextureCache` is only filled when a texture is found, `highestTextureMip` may be null and is pushed anyway.
Drop all of these; keep the struct layouts, lump list, naming scheme and stride table.

## Reuse in respak
- `wrap` (once built) for lump blobs; `rpak.asset` volume names and `rpak.guid` for lookup; Oodle in `oodle.rs`.
- Volume pattern: `starpak.rs`, core `volume.rs` (a BSP as a lump volume).
- `matl`/`txtr` (`Bitmap`) for material textures if a textured mesh target is built.
- New: header parser, lump table, vertex-lump decoder, entity parser, mesh capability (if wanted).

## Tests
- Synthetic header: `72 42 53 50`, version 0x0030 (placeholder), flags 1, mapRevision 0, lastLump 5, lump[3] = `{0, 36, 0, 0}`; lump 3 = three `float3`. Check `0x810` header length, lump 3 name `maps/<map>.bsp.0003.bsp_lump.client`, missing lump → `MissingDependency`.
- Synthetic geometry: 1 model, 1 mesh (`triCount 1`, `flags 0x000`), 1 material sort (`firstVertex 2`), 3 verts of 20 bytes with `posIdx/nmlIdx` pointing into 4-entry arrays, indices `[0,1,2]`; expect OBJ faces with `+2` offset.
- `.ent` parse: `"ENTITIES02 num_models=28\n// c\n{\n\"classname\" \"worldspawn\"\n}\n{\"model\" \"*3\"}"`, no header → error, unterminated entity/string → error, duplicate keys kept in order.
- Edge cases: `filelen == 0`, `flags & 1 == 0`, `lastLump = 128`, `triCount < 0`, bad `mtlSortIdx`, `texdata` offset out of the string lump (must be `Corrupt`, rsx indexes blindly).
- Real (no Steam runs without permission): Apex `mp_rr_*.rpak` (`maps/<map>.bsp`, `maps/<map>.bsp.NNNN.bsp_lump.client`, `maps/<map>_env.ent`); compare rsx OBJ export. Titanfall 2 `vpk/englishclient_mp_*.bsp.pak000_dir.vpk` for the TF2 header (not covered by rsx).

## Open questions
1. Scope: lumps-as-volume only, entities, or full mesh export? Mesh needs a new capability.
2. Titanfall 1/2 support: rsx has none; header layout and lump set differ (community). Do it from another reference or only Apex?
3. `lastLump` semantics, `flags` bits other than 0, lump `version` per-lump layouts, TF1/TF2 version numbers (29 / 37 community), Apex versions (47-50 community).
4. Exact entity file naming and whether `snd`/`spawn` (and `_script`, `_env`) files exist per VM; rsx only reads `env` and `script`. How `ENTITYPARTITIONS` (0x18) and `ENTITIES` (0x00) relate to the `.ent` files (community: partitions name the files; unverified).
5. LZMA lump compression in Titanfall and the `uncompLen` convention.
6. Inline-lump (`flags & 1 == 0`) BSPs: do they exist in Apex, and in which games?
7. Game lump (0x23) / static prop layout: unknown (nothing in rsx).
8. LIT_BUMP vertex field order (see Layout).
9. Is re-assembling a `.bsp` from lumps desirable (rsx does not)?
