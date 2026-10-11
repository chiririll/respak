# 07 · Slice: open an rpak and list its assets
**Status:** done · **Depends on:** 06 · **See:** PROJECT.md §5.4, §6, §12

## Goal
```
respak discover paks/Win64/common.rpak --depth 1
```
lists the pak's assets: path (name or GUID), kind and version. It covers every pak version rsx supports. This is the first container, and it tests the riskiest part of the design: a container as an OpenDAL service, plus virtual paths that go into it.

## Porting from rsx
The pak code comes from rsx (`C:\Users\chiririll\Projects\rsx`, `src/game/rtech/cpakfile.{h,cpp}` and the codecs). Review it first, then port it, following PROJECT.md §12. The review covers at least:
- pak headers and asset entries per version; version detection → the `rpak` score function;
- body compression and the codecs → how far a volume can decompress as a stream (§6) instead of loading the whole pak;
- pointer relocation → whether it supports the `.mem` scheme (§6), which slice 08 depends on;
- GUID hashing of asset paths.

## rsx review
(Done by a subagent sweep of rsx; the gate was delegated by the user. Paths are under rsx `src/game/rtech/`; H = `cpakfile.h`, C = `cpakfile.cpp`, U = `utils/utils.cpp`.)

**What it does.** rsx reads the u16 at offset 4 and supports pak versions 6, 7, 8 (C:206-245). Common prefix: magic `RPak`, version, flags (`1<<8` RTech, `1<<9` Oodle, `1<<15` zstd), created time, crc. The header is 0x58 bytes (v6, v7) or 0x80 (v8) and is never compressed; field offsets are in `respak-respawn/src/pak.rs` (H:259-372). After it comes the body, compressed or not: patch block (only if `patchCount > 0`), starpak path lists, segments (0x10), pages (0xC), pointer table (8), assets (0x48 for v6/v7, 0x50 for v8), guid refs (8), dependents (4), external refs, v8 unknown block, then the page data back to back (H:436-586, C:248-345). Starpak refs in assets: `-1` none, low 12 bits index, rest the offset (H:1175). Asset kind is a 4-byte tag in file order. Compression: RTech is one stream with the output written at `headerSize` (U:42-123, 125+); Oodle `cmpSize` counts only the body (C:800-842); zstd is refused (C:843-849). Pointers are `{page, offset}`; resolved by indexing page starts (C:366-402). GUID = `StringToGuid`, a SWAR hash with a case-folding `& 0xDF` (U:2431-2553).

**Problems (not ported).** Whole-file reads and a double allocation (C:143, C:809); no bounds checks on counts, page indices or segment indices (C:370, C:393, C:899-906); Oodle failure falls back to "uncompressed" (U:2269-2274), success judged by one u64 compare (C:817-826), `int` positions overflow at 2 GiB, and the decoded length is never checked (C:829); the RTech decoder is decompiled pointer code with 8-byte over-reads (U:72, U:216); `cmpSize` means different things for RTech and Oodle; `LoadNonPatched` skips only one patch file header (C:261); v6 magic is never checked; global state (`g_assetData`, cache db manager); pak buffers are rewritten in place.

**Fit.** Header/tables: `pak.rs` (bounds-checked cursor, `Corrupt` for any inconsistency, saturating sizes). Version: the `rpak` score function (magic 10, supported version +2, extension +1); an unknown version is `UnsupportedVersion` when read. Body: `Body::Raw` (ranged reads) or `Body::Rtech` (decompressed once, in memory; documented cost). GUID: `guid.rs`. Typed entries: `rpak.asset` (guid, kind, version, header size, page pointers). Behaviour changes against rsx: patched paks, zstd and Oodle are `UnsupportedVersion` (Oodle needs a licensed codec the host would supply; none ships); decoded length is checked; every count is validated before any allocation.

**Pointer relocation vs the `.mem` scheme (the "stop and ask" check).** Supported for unpatched paks: pages are concatenated, so a flat decompressed address space is just `page_base[index] + offset` (`Tables::page_bases`). Slice 08 can rewrite pointers to `.mem` offsets in one pass over the sorted pointer table. Patched paks need lower paks' pages and are out of scope.

## Scope
**Core:**
- `VolumeSource` (read-only: `stat`, ranged `read`, `list`) and the generic adapter that turns it into an `opendal::Operator` (API notes in PROJECT.md §11).
- Virtual path resolution: the longest real-file prefix is identified; if it is a `Container`, the rest is resolved with `entry(path)`; repeat. Real files win over container entries. Missing path → `NotFound`.
- `discover` with `depth` > 0.
- A mounted volume is kept for the whole `Engine` call (a plain map for now; the dedup middleware comes in slice 13).

**respak-respawn:**
- `rpak` read edge from `file`: score = magic + version → `Version` including the variant. Known format with an unknown version → `UnsupportedVersion`.
- `RpakVolume`: lists the assets, by name when known, otherwise by GUID; both paths resolve. It decompresses the body only as far as needed (§6). The codecs are ported from rsx. Oodle: check how rsx gets it and ask the user before shipping anything Oodle-related.
- GUID hashing of asset paths, ported.
- Typed `rpak.asset` entries (kind, version, header size, guid, …), with only the fields this slice reads. The `header` / `mem` files come in slice 08.
- Fixtures: a synthetic pak generated in the tests. **No game files in the repo.**

## Not in this slice
Asset bytes, `.mem`, pointer relocation, starpaks, globs, GUID dependencies.

## Stop and ask if
- The OpenDAL adapter turns out to be large or awkward. That is the signal this slice exists to catch.
- rsx's pointer relocation can't support the `.mem` scheme in §6 (check it during the sweep, since slice 08 depends on it).

## Demo
The user runs `discover` on real paks of each version and compares the lists with rsx.

## Result
Checked locally (not in the repo) against rsx's cached paks: `ui_main_menu.rpak` (v8, uncompressed, 1 `uiia`) and `ui.rpak` (v8, RTech, 86 MB, 1665 assets: ui 1629, txtr 11, uimg 9, shdr 6, matl 4, shds 4, font 2). `ui.rpak` decompresses to exactly `dcmpSize` in 0.4 s (release). The RTech decoder (`rtech.rs`) was ported by a subagent; two behaviours copied from rsx are position-dependent (the extended literal count is replaced near window edges) and are confirmed only by this one pak decoding with consistent tables. The GUID hash is **not yet verified** against real name/GUID pairs (rsx's cache db is empty here); slice 08 does it with texture names.

## Decisions (the user delegated the gate)
- License: respak becomes AGPL-3.0-only (PROJECT.md §11).
- Names are not known in this slice: the volume lists `0x<GUID>.<kind>`, and a path with a *name* (`texture/foo.txtr`) is hashed (`name` without the kind extension) and resolves. The hash port is checked only for its documented properties (case and slash insensitivity); it is not yet verified against known name/GUID pairs.
- The volume serves the asset header bytes as stored (pointers not relocated until slice 08).
- A compressed pak is decompressed whole into memory on first use (RTech is one stream). A bounded streaming decoder is on the backlog.
- Not supported (clear `UnsupportedVersion`): patch files (`patchCount > 0`), zstd, Oodle.
- Core: `VolumeSource` + `volume_operator` (~150 lines, no OpenDAL problems found), `Mediator::open`/`mount` with `Cx::mounts`, `discover --depth`, `Metadata::kind`, `Container::entry` default checks existence. The OpenDAL adapter was not awkward, so no stop was needed.

## Tests
Core (`tests/volume.rs`, `tests/containers.rs`): stat/read/list of the adapter, ranged reads reach the source as asked, read-only; virtual paths into nested containers, mounts reused, real files win, unlisted entries resolve, `NotFound` cases; `discover` depth 0/1/2 and paths inside containers.
Respawn (`tests/rpak.rs`): v6/v7/v8 detection and listing, header bytes and ranges served, named paths resolve, wrong kind/missing is `NotFound`, duplicate guids, unsupported variants say why, broken paks are `Corrupt`, tables expose starpaks/pages/asset fields; a local-only check against a real small pak.
