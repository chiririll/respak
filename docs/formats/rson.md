# rson · RSON (Respawn object notation tree)
> **Verified against** Apex `paks/Win64/common_early.rpak` (14 assets), `script_ui.rpak` (2), `script_client.rpak` (2), `ai_behaviors.rpak` (2), `mp_lobby.rpak` (1): header size 0x10, version 1, node stride 0x28 and the points in *Verified on real data* below. Walked every node of every one of them (4619 nodes).
**rsx sources:** `game/rtech/assets/rson.h` (`eRSONFieldType`, `RSONNodeValue_t`, `RSONAssetHeader_v1_t`, `RSONAsset`, `RSONAssetNode_t`), `game/rtech/assets/rson.cpp` (`LoadRSONAsset`, `PreviewRSONAsset`, `ExportRSONAsset`, `RSONAssetNode_t::R_ParseNodeValues`, `RSONAssetNode_t::R_WriteNodeValue`, `WriteRSONDependencyArray`, `InitRSONAssetType`), `game/asset.h:60, 141, 215` (FourCC, colour, export folder `rson`), `core/main.cpp:88, 160`, `docs/assets/rpak/RSON.md` (empty in rsx) · **Games / versions:** Apex Legends; asset version 1 only (which seasons: unknown); not checked for Titanfall 2 · **Priority hint:** low–medium (a generic config/data tree, real contents unknown), **M** (pointer-graph walker, but small).

## Summary
A pre-parsed tree of named values (objects, arrays, strings, booleans, integers, doubles, null), stored as pointers inside the rpak so the game does not parse text at load time. It is the binary form of the "RSON" text config files Respawn uses (rsx's `asset.h` comment says `'maps'/'scripts'`). The user extracts a text file (`.rson`) that is close to JSON without quotes and commas, or a JSON file (proposed).

## Where it lives
- rpak asset, FourCC `rson` in `asset.h:60`; the binding type is `'nosr'` (MSVC multi-char literal, the same four bytes reversed, `rson.cpp:281`), `headerAlignment = 8`. On disk the FourCC bytes are the ones `rpak.asset` already exposes as `*b"rson"`; verify the byte order against `stlt`/`subt` (which use the same trick, `'tbus'`).
- Data: header (0x10) plus a graph of nodes, value arrays and C strings in shared page data (`.mem`). No starpak, no streamed data.
- **Name not stored.** `SetAssetNameFromCache()` only (guid → name from the rsx cache DB, `asset.h:383`); if the name is not in the cache the asset keeps its guid name. Names in real paks: unknown (probably script/config paths with an `.rson` extension). Export file name `<filename>.rson`, folder `rson/` (or the parent of the full asset path when `exportPathsFull`).
- Proposed respak volume name: `<name>.rson`, or `<guid>.rson` when no name is known (project.md rules for guid-only names).

## Versions and detection
| asset version | header | rsx |
|---|---|---|
| 1 | `RSONAssetHeader_v1_t`, 0x10 bytes | parsed |
| other | — | `LoadRSONAsset` returns early without setting extra data; `PreviewRSONAsset` and `ExportRSONAsset` then dereference a null `extraData` (crash) |

No other versions seen in rsx; version 0 or 2+ exist: unknown.

Proposed: `rpak.asset` kind `rson` → score 10 (container-provided kind); version 1 → `Version { major: 1, minor: 0 }`; others → `UnsupportedVersion`. No header-size check is done by rsx; respak may check `header_size >= 0x10` and treat a smaller one as `Corrupt`.

## Layout
Natural alignment (no pack pragma), little-endian, 64-bit pointers. `int` is signed 32-bit. Every pointer is a page pointer (relocated by the rpak volume); the asset header is relocated by the volume, nodes and value arrays are in `.mem` and carry relocated pointers too.

### `eRSONFieldType` (`rson.h:5-20`, bit flags in `type`)
| Value | Name | Meaning |
|---|---|---|
| 0x001 | RSON_NULL | no value; printed `null` |
| 0x002 | RSON_STRING | `values.string` is a `char*` page pointer to a NUL-terminated string |
| 0x004 | RSON_VALUE | "what" (rsx asserts, prints an empty line); unused? unknown |
| 0x008 | RSON_OBJECT | `values.valPtr` → array of nodes |
| 0x010 | RSON_BOOLEAN | `values.valueBool` (low byte) |
| 0x020 | RSON_INTEGER | `values.value` printed as signed 64-bit |
| 0x040 | RSON_SIGNED_INTEGER | same as above |
| 0x080 | RSON_UNSIGNED_INTEGER | `values.value` printed as unsigned 64-bit |
| 0x100 | RSON_DOUBLE | `values.valueFP` (IEEE 754 double) |
| 0x1000 | RSON_ARRAY | flag; the node is an array of the base type |
| 0x1008 | RSON_ARRAY_OF_OBJECTS | `RSON_ARRAY | RSON_OBJECT` |

The scalar switch masks with `type & 0x1ff`; `IsArray()` tests `type & 0x1000`; `IsArrayOfObjects()` tests `(type & 0x1008) == 0x1008`. Types are tested as a mask in `IsObject()`, but the printer uses an exact `switch`, so combined bits (e.g. 0x03) fall into `default` (assert, nothing written).

### `RSONNodeValue_t` (union, 0x8)
One 8-byte slot: reinterpreted as `RSONNodeValue_t*`/`void*`/`char*` (page pointer), `u64`, `f64`, or `bool` (byte 0). Which interpretation applies is decided by the owning node's `type`.

### `RSONAssetHeader_v1_t` (`rson.h:33-39`, 0x10) — the root node's data
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | type | root node type (`eRSONFieldType`); root is normally 0x8 or 0x1008 |
| 0x04 | 4 | i32 | valueCount | element count (objects: see below; arrays: number of `RSONNodeValue_t`) |
| 0x08 | 8 | union | values | pointer or inline value, as per `type` |

The root has no name (`name == nullptr` marks the root, `IsRoot()`).

### `RSONAssetNode_t` (`rson.h:56-72`, 0x28, in `.mem`)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr | name | page pointer to NUL-terminated key; null only for the root |
| 0x08 | 4 | i32 | type | `eRSONFieldType` |
| 0x0C | 4 | i32 | valueCount | element count |
| 0x10 | 8 | union | values | pointer/inline value |
| 0x18 | 8 | ptr | nextPeer | next sibling node (page pointer) or null |
| 0x20 | 8 | ptr | prevPeer | previous sibling; "unsure if ever written to disk" (not read by rsx) |

Size 0x28 follows from the field types (rsx has no `static_assert` on it); `nodes[i]` in the printer indexes the array with this stride.

### How the graph is read (from `R_ParseNodeValues` / `R_WriteNodeValue`)
- **Scalar node** (not array): value is in `values` itself.
- **Object node** (`type & 0x1ff == 0x8`, not array): `values.valPtr` → contiguous array of nodes; rsx loops `i in 0..valueCount`, and for each `nodes[i]` walks the `nextPeer` chain until null. **Verified on real data: `valueCount` counts chain heads and is 1; members are the `nextPeer` chain** (the chain nodes are physically contiguous).
- **Array of scalars** (`type & 0x1000`, not objects): `values.valPtr` → array of `valueCount` `RSONNodeValue_t` (8 bytes each); element interpretation by the node's base type (`type & 0x1ff`).
- **Array of objects** (`0x1008`): the array is `RSONNodeValue_t[valueCount]`; each element's `valPtr` is either null (empty `{}`) or points to the first node of an object, whose members are the `nextPeer` chain.
- Arrays of arrays: not handled by rsx.

## Verified on real data
- Every asset: version 1, header size 16. Root types seen: `0x1002` (array of strings), `0x8` (object), `0x1008` (array of objects), `0x1004` (empty array, `valueCount` 0).
- **Object `valueCount` is the number of chain heads, and it is always 1** (475 of 475 object nodes, root included): `values` points at the first member and the other members are reached through `nextPeer`. The members are laid out contiguously (stride 0x28), but because `valueCount` is 1 the `0..valueCount` loop plus chain walk emits each member once, so no duplicate output. Do not iterate `valueCount` nodes as a contiguous array.
- Scalar nodes have `valueCount` 1. Array `valueCount` is the element count.
- Types in use: `0x2` string (2395), `0x20` integer (940), `0x100` double (455), `0x10` bool (152), `0x8` object, arrays `0x1002`, `0x1008`, `0x1020` (arrays of non-negative integers, e.g. colour triples `0x59 0x4D 0x3F`), `0x1040` (one array of `-1`s), scalar `0x80` (one value, `0xD83A90851A8EF513`, above `i64::MAX`), and `0x1004` (twelve **empty** arrays; `RSON_VALUE` appears only as the element type of an empty array). No `0x1` null, `0x1100` or scalar `0x40` was seen. The three integer flags look like a range tag: `0x20` fits a non-negative `i64`, `0x40` is negative (stored sign-extended in the 8-byte slot), `0x80` is above `i64::MAX`.
- `prevPeer` was 0 in all 4619 nodes. All strings (names and values) are 7-bit ASCII.
- **Null pointers inside page data are all-zero** (`{page 0, offset 0}`), not `{-1, -1}` (e.g. the `values` slot of an empty object/array, the last `nextPeer`). Only the asset header, after relocation, uses `u64::MAX` for null. A reader of node fields must treat the pair `(0, 0)` as null; a real pointer to page 0 offset 0 does not occur here.
- Pointers in node memory are `{u32 page, u32 offset}`; only the header's own `values` is already a `.mem` offset.
- The FourCC read through `rpak.asset` is `rson`.
- Example contents: a store-offer table (`enum`, `pc`, `ps4`, `xb1`, `pc_offer`, ... strings; root `0x1008`), audio reverb presets (`emitter_controllers`/`listener_controllers`), an audio bus scale table, a list of ping/category names, and a list of font-button tokens such as `%$vgui/fonts/buttons/button_a%` (root `0x1002`).

## Data outside the header
Only page-data blocks pointed to by the graph (nodes, value arrays, name and string bytes). No streaming, no compression. The size of strings is the NUL; array and object sizes come from `valueCount`.

## Dependencies
None. rsx does not resolve references inside an `rson`. (`WriteRSONDependencyArray` in the same file is a helper that other asset exporters — `model.cpp:578-579`, `animrig.cpp:235` — use to write `rigs:`/`seqs:` guid lists in RSON text syntax into their dependency files; it is not part of this asset's decoding.) Whether string values in real data name other assets: unknown.

## Decoding
1. Check asset kind `rson` and version 1; read the 0x10-byte header → synthetic root node `(name = null, type, valueCount, values)`.
2. `parse(node, depth)`: resolve pointers through the volume (`mem_offset_of` / `read_mem`), bounds-check every one (rsx checks none) and guard against cycles (a visited-set or a depth limit; a malformed `nextPeer` loop would hang rsx).
3. If `type & 0x1000`: read `valueCount` slots of 8 bytes at `values`.
   - Array of objects: for each slot, null → empty object; else read nodes along the `nextPeer` chain, parse each as a member.
   - Otherwise: decode each slot with the base type (`type & 0x1ff`): null, string (`cstr`), bool (byte 0 != 0), signed (`i64`), unsigned (`u64`), double (`f64`).
4. Else by `type & 0x1ff`: scalar from `values`; object: read `valueCount` nodes at `values` (stride 0x28) and walk each `nextPeer` chain (matches rsx; dedupe by node address in respak so chained contiguous nodes are not emitted twice, decide at the gate).
5. Emit the tree. Names are page-pointer C strings (encoding: unknown, UTF-8 assumed).

Pseudo-text printer rules in rsx (`R_ParseNodeValues`), indentation = one `\t` per level:
```
<indent><name>:            // omitted for the root
  scalar    -> " <value>\n"                    // same line as the key
  object    -> "\n" (if named) <indent>"{\n" members at indent+1 <indent>"}\n"
  array     -> "\n" (if named) <indent>"[\n" elements "<indent>]\n"
               scalar element:  "<indent>\t<value>\n"
               object element:  "<indent>{\n" members at indent+2 "<indent>}\n"
               root array of objects: brackets omitted (elements only)
```
Values: strings are written raw (no quotes, no escaping); booleans `true`/`false`; integers decimal; doubles with default `ostream` formatting (6 significant digits, so lossy); null → `null`.

## Output
rsx `ExportRSONAsset` (`rson.cpp:57-94`): the text above as `<export>/rson/<filename>.rson` (or `<export>/<asset parent path>/<filename>.rson` with `exportPathsFull`). The same text is the preview. The `setting` argument is ignored (single format).

Proposed respak: kind `rson`, `Metadata` (name, guid; no dependencies) and either:
- a lossless JSON target: `{ "key": value, ... }` for objects, JSON arrays for arrays, strings/booleans/numbers/null native, `u64` above 2^53 and doubles kept exact (number or string: decide), or
- the rsx-compatible text writer (round-trip text → binary is out of scope).

Keys inside an object may repeat in principle (a node chain has no uniqueness rule); JSON output should therefore use an ordered list of `[key, value]` pairs or arrays-of-members in the lossless form, unless the data is checked to be unique. A root-level array of objects is a JSON array of objects.

## rsx quirks
- Only version 1 handled; any other version is silently skipped at load and crashes later (null `extraData` in preview/export). Return `UnsupportedVersion` instead.
- No pointer, bounds, depth or cycle checks. Port with all of them.
- `RSON_VALUE` (0x4) asserts "rson type RSON_VALUE used", but real data uses it as the element type of empty arrays (`0x1004`, `valueCount` 0), so those must not hit the assert path; `RSON_INTEGER` and `RSON_SIGNED_INTEGER` are printed the same way (see *Verified on real data* for the apparent difference).
- `std::boolalpha` is set on the output stream by the first boolean and then stays on for the rest of the stream (only affects later bool output, harmless).
- Double printing uses default precision (6 significant digits): lossy export; do not copy.
- The printer does not quote or escape strings, so strings with newlines or `"` produce ambiguous text; names the same.
- Node `type` is matched with `switch(type & 0x1ff)`; any composite value is an invalid type → assert, nothing printed (output silently truncated in release).
- Object printing: the `i < valueCount` loop plus `nextPeer` chain walking is correct on real data because object `valueCount` is the number of chain heads (always 1).
- `prevPeer` is never read.
- `RSONAssetHeader_v1_t.valueCount` meaning for the root: when the root is a scalar it is irrelevant.
- Output is only the text; no indication of the root node type is stored in the file (the preview shows `Root node type: 0x%x`).

## Reuse in respak
- `crates/respak-respawn/src/assets/stlt/` and `assets/subt/` are the closest models: `detect_kind`, `rpak_asset(&source, *b"rson")`, `asset.read_header()`, `asset.mem_offset_of(slot)` / `asset.read_mem(at, len)`, `Reader` (LE fields), the `cstr` helper (needs a larger or shared limit), `Error::corrupt`, `ErrorKind::UnsupportedVersion`, `FormatId::new("rson").with_version(Version::new(1))`, `Data` + `Metadata` impl shape, `json.rs` encoder pattern.
- `stgs` (`assets/stgs/`) already walks nested page-pointer structures and writes JSON (`json.rs`): use as the model for tree walking and a JSON encoder.
- New: `RsonValue` tree type (Null, Bool, Int(i64), UInt(u64), Double(f64), String, Object(Vec<(String, RsonValue)>), Array(Vec<RsonValue>)), node walker with cycle/depth guard, JSON and text encoders, handler registration.

## Tests
- Synthetic header v1: `type = 0x8`, `valueCount = 2`, `values` → two nodes in mem (stride 0x28): `{name:"a", type:0x2, values→"x"}` and `{name:"b", type:0x10, values=1}`; expect `a: x`, `b: true`. Chain variant: one head node with `nextPeer` → second node.
- Array of scalars (`0x1002`, three strings), array of integers (`0x1020`), array of doubles (`0x1100`), array of objects (`0x1008`) with one null element, root array of objects (no brackets in text output).
- Scalars at the root: `type = 0x20` with inline value; `type = 0x1` null.
- Negative signed integer (`0x40`, `values = 0xFFFF_FFFF_FFFF_FFFE`) → `-2`; unsigned (`0x80`) same bits → large positive.
- Errors: version 0 / 2 → `UnsupportedVersion`; pointer past the mem length → `Corrupt`; `nextPeer` cycle → `Corrupt`; invalid type bits (0x3) → `Corrupt`; null `values` with non-zero `valueCount` → `Corrupt`; empty object (`valueCount = 0`).
- Real: `paks/Win64/common_early.rpak` (14 `rson`, incl. guid `0x35F0A27DA4165BAD` `0x1008` store offers with 182 elements, `0x44BD62869BBAB6E8` reverb presets with 64), `script_ui.rpak`, `script_client.rpak`, `ai_behaviors.rpak`, `mp_lobby.rpak`.

## Open questions
1. What are the contents in other paks/seasons? Only the 21 `rson` assets of `common_early`, `script_ui`, `script_client`, `ai_behaviors` and `mp_lobby` were walked; names are not stored, so only guids are known.
2. Output: lossless JSON, rsx-style text, or both? Duplicate keys: ordered pair list or map? (No duplicate-key check was run.)
3. Arrays of arrays and a non-null `prevPeer` were never seen; a `RSON_ARRAY_OF_OBJECTS` element with a null pointer (empty object) was not seen either.
