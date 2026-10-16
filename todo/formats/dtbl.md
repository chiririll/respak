# dtbl · Datatable
**rsx sources:** `game/rtech/assets/datatable.{h,cpp}` · **Games / versions:** Apex Legends, asset versions 0 and 1 (1.1 by pak date) · **Priority hint:** S

## Summary
A typed table (columns with names and types, rows of fixed stride). Users convert it to CSV (the repak input format) and JSON.

## Where it lives
rpak asset, FourCC `dtbl` (rsx registers `'lbtd'`, file order), header alignment 8, extension `dtbl`. No starpak data. The name comes from the guid name cache, not stored.

## Versions and detection
- asset version 0 → V0 (`Version 0.0`)
- asset version 1 → V1 (`1.0`), or V1_1 (`1.1`) when the pak `createdTime` is strictly between `0x1d692d897275335` (2020-09-25) and `0x1da975b0a106ef2` (2024-04-25); `RpakAsset.pak_created_time` carries it
- anything else → `UnsupportedVersion`. Score 10 on `asset.entry.kind == *b"dtbl"`.

## Layout
Little-endian; `ptr` is a page pointer (relocated, a `.mem` offset in respak).

Header V0 (0x20): 0x00 i32 numColumns · 0x04 i32 numRows · 0x08 ptr columns · 0x10 ptr rows · 0x18 i32 rowStride · 0x1C pad.
Header V1 / V1_1 (0x28): same first 0x18 bytes, 0x18 u8[8] unknown, 0x20 i32 rowStride, 0x24 pad.

Column V0 and V1 (0x10): 0x00 ptr name (C string) · 0x08 i32 type · 0x0C i32 rowOffset.
Column V1_1 (0x18): 0x00 ptr name · 0x08 u8[8] unknown · 0x10 i32 type · 0x14 i32 rowOffset.

Types (in-row size): 0 bool (1) · 1 int (4) · 2 float (4) · 3 vector (12, 3 × f32) · 4 string (8, ptr) · 5 asset (8, ptr) · 6 asset_noprecache (8, ptr).

Rows: `rows + rowStride * i`; a cell is at `row + column.rowOffset`. The stride is from the header, not computed.

## Data outside the header
All in the rpak pages (rows, columns, strings). No streamed data, no compression.

## Dependencies
None. Asset-typed cells are plain strings, not guids.

## Decoding
1. Pick the layout from asset version and pak date; check `header.len()` matches.
2. Bounds-check `columns + numColumns * colSize`, `rows + numRows * rowStride` and every `rowOffset + typeSize <= rowStride` against the `.mem` length (rsx has no checks; `Corrupt`).
3. Read the names (C strings) and cells in column-record order.
4. String cells: follow the pointer, read a C string; a first byte of `0xf` means DFS stripped the data → text `!!DATA EXCLUDED!!`.

## Output
`Table` capability (new, in core): named columns with a type, rows of typed cells. Converters:
- `dtbl.csv`, **exactly like rsx** (cpp:154-252): line 1 the quoted column names; one line per row; bool `true`/`false`; int decimal; float with C++ ostream default formatting (6 significant digits, `%g`); vector `"<x,y,z>"` quoted; string, asset, asset_noprecache quoted with no escaping; the last line the quoted type names (`"bool"`, `"int"`, `"float"`, `"vector"`, `"string"`, `"asset"`, `"asset_noprecache"`) with no trailing newline. This is the repak input format.
- `dtbl.json`: columns with name and type, rows as arrays (new, rsx has none). Floats round-trip exact.

## rsx quirks
Kept: the lossy float format, no quote escaping, the missing final newline, the `!!DATA EXCLUDED!!` text (all for repak compatibility). Dropped: no bounds checks, and an unknown column type (rsx writes a short line) becomes `Corrupt`.

## Reuse in respak
`RpakAsset::read_header`, `mem_offset_of`, `crate::txtr::read_name` for C strings, `matl.rs` pointer helpers, `asqd.rs` as the module shape. New: the `Table` trait in `respak-core/src/capability/table.rs` (empty today), a `PakBuilder::dtbl` test helper.

## Tests
Synthetic: V0, V1 and V1_1 layouts; each type; stripped string; zero rows; short header and unknown version (`Corrupt` / `UnsupportedVersion`); out-of-range pointers; CSV golden text. Real files: a pak with `dtbl` (ask before any Steam run).

## Open questions
Decided with the user: CSV like rsx, JSON added, stripped strings as rsx.

## Implemented (2026-10-16)
`respak-respawn/src/dtbl.rs` (read) and `dtbl_text.rs` (`dtbl.csv`, `dtbl.json`); `Table` capability filled in core. JSON is `{"columns": [{"name", "type"}], "rows": [[...]]}`, one row per line, vector `[x,y,z]`, non-finite float `null`. A null string pointer reads as an empty string (rsx would crash). The name is `None` (no guid name cache yet). NaN prints as `nan` / `-nan` in the CSV, which may differ from MSVC (`-nan(ind)`). Not yet run on a real pak.
