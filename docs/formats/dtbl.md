# dtbl · Datatable
**rsx sources:** `game/rtech/assets/datatable.{h,cpp}` (`DatatableAssetHeader_v0_t`, `_v1_t`, `DatatableAssetColumn_v0_t`, `_v1_1_t`, `GetDTBLVersion`, CSV export) · **Games / versions:** Apex Legends, asset versions 0 and 1 (layout variants 0, 1, 1.1) · **Priority hint:** built (S). This spec describes `crates/respak-respawn/src/assets/dtbl/`.

## Summary
A typed table: named columns, rows of fixed stride. Exported as CSV (rsx's repak-input format) or JSON. Also a name source (its string cells are asset paths).

## Where it lives
rpak asset, FourCC `dtbl`, extension `dtbl`. The name is not stored (`Metadata::name` is `None`); it is found by guid (the `names` command uses the cells through `NameSource`). No streamed data.

## Versions and detection
`dtbl::variant(asset_version, pak.created_time)`: version 0 -> `0`; version 1 -> `1.1` if the pak `createdTime` is inside the open range (0x01D692D897275335, 0x01DA975B0A106EF2) (2020-09-25 to 2024-04-25 in rsx), else `1`; any other version -> `UnsupportedVersion`. Score 10 for kind `dtbl` when the variant is known (otherwise no match). Format id `dtbl@0` / `dtbl@1` / `dtbl@1.1`.

## Layout
Little-endian; pointers are page pointers.

Header v0 (0x20): `0x00 i32 numColumns`, `0x04 i32 numRows`, `0x08 ptr columns`, `0x10 ptr rows`, `0x18 i32 rowStride`, `0x1C` padding.
Header v1 / v1.1 (0x28): same through `0x17`, `0x18` 8 bytes unknown, `0x20 i32 rowStride`, `0x24` padding.
A header whose length differs from the variant's, or a negative count / stride, is `Corrupt`.

Column v0 / v1 (0x10): `0x00 ptr name`, `0x08 i32 type`, `0x0C i32 rowOffset`. Column v1.1 (0x18): `0x00 ptr name`, `0x08` 8 bytes unknown, `0x10 i32 type`, `0x14 i32 rowOffset`.

Types: 0 bool (1 byte), 1 int (i32), 2 float (f32), 3 vector (3 x f32, 12 bytes), 4 string (ptr, 8), 5 asset (ptr), 6 asset_noprecache (ptr). Other value: `Corrupt`. A cell is at `rows + row*stride + rowOffset`; `rowOffset + size > rowStride` is `Corrupt`.

## Data outside the header
All in `.mem` pages: column records, the row block (`numRows * rowStride`), NUL-terminated strings (at most 1 MiB, must end inside `.mem`). No compression, no stream.

## Dependencies
None followed (`dependencies()` is empty). Asset-path cells are only exposed through `NameSource` (every non-empty string / asset / asset_noprecache cell).

## Decoding
Parse the header, read and validate the column block, read each name through its pointer slot (an all-zero slot is an empty string), read the row block, decode the cells. A string whose first byte is `0x0F` reads as `!!DATA EXCLUDED!!` (`DATA_EXCLUDED`). Out-of-range blocks and strings are `Corrupt`. Strings are lossy UTF-8.

## Output
`Table` capability (`Column{name, ty}`, `Cell::{Bool, Int, Float, Vector, String}`), `Metadata`, `NameSource`.
- `dtbl.csv` (no options): rsx text byte for byte: a line of quoted column names; one line per row (bool `true` / `false`, int, float as C++ `%g` with 6 digits (`format_g`), vector `"<x,y,z>"`, string / asset quoted, no escaping); a last line of quoted type names with no trailing newline.
- `dtbl.json` (no options): `{"columns":[{"name","type"}],"rows":[[cell,...]]}`, one row per line; floats keep f32 precision; vector `[x,y,z]`; a non-finite float is `null`.

## rsx quirks
No bounds checks (respak: `Corrupt`). The V1.1 column layout is picked by pak date, not by asset version. CSV strings are not escaped, so a quote in a cell breaks the file (kept for compatibility). The unknown bytes (v1 header `0x18`, v1.1 column `0x08`) are ignored.

## Reuse in respak
`rpak.asset`, `rmdl::reader`, `Table` / `NameSource` capabilities (`respak-core`), the `names` command in `respak-cli`.

## Tests
`dtbl/tests.rs`: variant selection incl. date boundaries, both header layouts, bad length / negative count. `tests/dtbl.rs` (synthetic `PakBuilder::dtbl`): all three layouts read the same table, CSV exact text, JSON, no rows, unsupported version, corrupt headers / pointers / columns. `respak-cli/tests/names.rs` uses a dtbl cell as a name source. Gap: no real-pak test.

## Open questions
1. Real-pak check of the v1.1 date window and of the v1 unknown bytes. 2. Other `dtbl` versions (Titanfall 2): unsupported. 3. Escaping in CSV / JSON beyond rsx parity.
