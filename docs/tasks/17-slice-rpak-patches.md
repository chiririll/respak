# 17 · Slice: rpak patches
**Status:** in-progress (built and tested; demo pending) · **Depends on:** 07, 08 (rpak listing and volume) · **See:** project.md §6, §11, §12, `crates/respak-respawn/src/pak.rs`

## Goal
A patched pak opens like an unpatched one. The volume of `common.rpak` (or `common(NN).rpak`) lists and reads the final assets:
```
respak discover paks/Win64/common(03).rpak --depth 1
respak convert paks/Win64/common(03).rpak/<asset> --to …
```
Today `PakHeader::parse` rejects `patch_count > 0` with `UnsupportedVersion`.

## rsx review (`cpakfile.{h,cpp}`, `patchapi.cpp`, `core/filehandling/rpak.cpp`; v7 and v8 only, v6 has no patches)
**Nothing is merged except page bytes.** The top patch `X(NN).rpak` carries the complete final tables (segments, pages, pointers, assets, guid refs, dependents, starpak paths). Only the bytes of pages `0..patchPageCount` are missing; they are rebuilt from the lower files by an edit script.

**Patched file body** (inside the compressed region; the header stays plain):
```
PakPatchDataHdr_t { i32 patchDataStreamSize; i32 patchPageCount }   8
PakPatchFileHdr_t[patchCount] { i64 cmpSize; i64 dcmpSize }         16 each (rsx never reads them; cross-check)
u16 patchFileIndex[patchCount]                                      0 = base file (no suffix), n = "(nn)"
… tables as today (TOTAL counts of the final pak) …
patch edit stream (patchDataStreamSize)
page data of pages [patchPageCount .. numPages) only
```
**Chain:** lower file name = `{stem}{idx==0 ? "" : "(%02d)"}.rpak` beside the top file. Stem = name cut at the first `(`. Each lower file is decompressed on its own; the combined buffer F = `top body | idx[0] body | idx[1] body | …` (lower headers dropped).

**Edit stream:** Huffman table A (depth 6, 64 entries: command + code length), table B (depth 8, 256 entries: exponent + code length), a u64 whose low 24 bits give the offset of the replacement data S, then an LSB-first bit stream. Per command: read a command from A; for commands 0-3 read an exponent from B and `N = (1<<e) + read(e)`; otherwise `N = {3, 7, 6}[cmd-4]`.

| cmd | effect |
|---|---|
| 0 | copy N from F |
| 1 | skip N of F |
| 2 | insert N from S (F untouched) |
| 3 | overwrite: N from S, F advances N |
| 4 / 5 | 1 byte from S, then copy 3 / 7 from F |
| 6 | 2 bytes from S, then copy 6 from F |

The first stretch (the top's own pages, `GetContainedPageDataSize` bytes) is an implicit command 0. Output order is pages `P..N-1` first, then `0..P-1` (P = `patchPageCount`); rotated into index order it is exactly respak's flat `.mem` (`page_base[page] + offset` stays valid). rsx's per-segment aligned allocation and header repacking are engine memory-layout details: not ported.

**patch_master.rpak** (`ptch` asset, `PatchAssetHeader_t`: `i32 unk; i32 patchCount; ptr pakNames; ptr pakPatchVersions`) maps `common.rpak` → top patch number. It only decides which `(NN)` file is the top.

**Do not copy:** the dead patch branch in `LoadNonPatched`; bare-bool results (`ProcessNextPage`, `PatchFunc`); the 100-iteration retry loop; unchecked command index and buffer bounds; `numBytesToSkip` honoured by commands 0 and 2 only; globals (`m_patchMasterEntries`, `m_pakLoadStatusMap`); the CRC "already loaded" map (double loads when the base is opened first); assuming one header size and version across the chain.

## Changes in respak (built)
1. `pak.rs`: drop the rejection; parse the patch block; `tables_len` grows by it; `page_data_offset = tables_len + stream size`; only pages `[P..N)` are file-resident (fix the "pages fit" check); a `has_patches` flag; no direct `header_range` read for patched paks.
2. Chain loader: resolve and decompress lower files through the volume's operator (same directory).
3. Edit-stream interpreter: typed, bounds-checked, `Result`, unknown command → `Corrupt`.
4. `ptch` parser and the redirect policy.

## Gate answers (user, 2026-10-15)
1. Lazy reads stay: the final `.mem` is a `Blob` with ranged reads. The edit script is parsed once into a map (destination range → source range in a chain file or in the replacement data), and a ranged read follows the map into the lower files' blobs. Only the script and the tables are held in memory.
2. patch_master redirect: yes (absent file = no redirect).
3. A lower file opens as itself.
4. A missing chain file is `MissingDependency { file }` on first read only.
5. Real files: Steam commands allowed when needed. Apex app `1172470`, depot `1172471`. Downloaded to `out/patches/paks/Win64/` (gitignored `out/`): `patch_master.rpak` (2468 bytes, v8, holds a `vers` and a `Ptch` asset) and `common(01).rpak` (5.97 MB, v8, patchCount 1). The base `common.rpak` is not downloaded yet.

## Open questions (need a real patched pak)
- Order of `patchFileIndex[]` (`[0,1,2]` vs `[2,1,0]`) decides which body follows the top's in F; rsx comments only.
- After the whole stream, F and S must both be consumed to exactly their ends: use it as the integrity check.
- Do commands 1, 3-6 ever span header-page gaps (where rsx would mishandle the skip)? The flat model decides.
- `PakPatchFileHdr_t` sizes vs the lower files' real sizes: usable as validation?
- Duplicate GUIDs across the chain: the top's table is authoritative in rsx.

## Built
- `pak.rs`: patch block parsed (`PatchBlock`), `patch_count > 0` no longer rejected; `header_range` is now in `.mem` coordinates.
- `patch.rs` (new): Huffman tables, bit reader, the seven commands, `PageMap` (runs from output ranges to sources; `locate` rotates `.mem` order into load order). Malformed input is `Corrupt`, never a panic.
- `rpak.rs`: `open_body` shared by the top file, the chain and `patch_master`; `Inner::read_mem` is the one way to read pages; the chain is opened and the map built on the first page read (`OnceCell`).
- `RpakRead::new(patches)`, `RespawnPlugin::ignore_patches`, CLI `--no-patches` (docs/cli.md).
- `testing.rs`: `build_patch_file`, `PatchSpec`, `EditScript` (an edit-stream encoder with 3-bit codes).

## Tests (written by me on your "implement": adjust freely)
`crates/respak-respawn/tests/patch.rs`, synthetic: every command (0-6) rebuilds a page; every `.mem` sub-range equals the expected bytes; asset headers in the rebuilt and the stored page; listing works without the lower files, a page read then says `MissingDependency` with the file name; lower file naming (`(02)` index beside `(03)`); bad streams (no commands, past the files, cut, tables cut, empty, unknown command) are `Corrupt`; `patchPageCount` over the page count is `Corrupt`; `patch_master` redirect, entries that do not apply (other name, 0, missing file, junk master); `ignore_patches` opens the base and refuses the patch file.
Local, `-- --ignored` (`tests/rpak.rs`, needs `out/patches`): `patched_common_rpak_reads_all_its_pages`.

## Real-file result (Apex, `common.rpak` + `common(01).rpak`, 2026-10-15)
- `common.rpak` beside `patch_master.rpak` opens as `common(01).rpak` (180958 entries).
- The whole patched `.mem` (1.52 GB) reads without an error and all 180958 asset headers relocate (every pointer lands in a real page): the script and the map are right for this pair. Debug build: 103 s for the full read.
- Still unverified: the order of `patchFileIndex[]` with more than one lower file (this pak has one: `[0]`), and the cross-check against the block's `cmpSize` / `dcmpSize`.
