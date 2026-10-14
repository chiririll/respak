# vpkf · Respawn VPK (Titanfall / Titanfall 2 / Apex `_dir.vpk`)
**rsx sources:** `game/vpk/vpk.h` (constants, structs, `CVPKFile`, `CVPKPackage`), `game/vpk/vpk.cpp` (`CVPKPackage::ProcessAssets`, `ParseFromFile`, `LoadVPKFileAsset`, `PreviewVPKFileAsset`, `InitVPKFileAssetType`), `core/filehandling/valvepak.cpp` (`HandleVPKLoad`), `core/filehandling/load.cpp:27-28, 104-105, 295`, `game/asset.h:89, 234`, `core/main.cpp:112, 184` · **Games / versions:** Respawn VPK 2.3 only · **Priority hint:** high value (scripts, maps, loose models/materials of R1/R2/Apex), **M–L**: directory parsing is S, but extraction is entirely new work (rsx does not extract, see Summary) and needs an LZHAM decoder.

> **Scope warning.** rsx only **lists** a VPK directory. It never opens the `_NNN.vpk` archives, never reads file data, never decompresses, and exports nothing (`LoadVPKFileAsset` is empty, `vpk.cpp:111-114`; the type binding has no export function, `vpk.cpp:165-179`). Everything this spec says about archive data, compression and preload bytes beyond the struct fields is **not in rsx** and is marked "unverified / community knowledge". The research was cut short by the coordinator: those parts need checking against a real file or another reference at the gate.

The `wrap`, `wepn` and `impa` kinds that `asset.h:64-67` groups under "vpk" are **rpak assets**, not VPK entries (see [wrap.md](wrap.md), [wepn.md](wepn.md), [impa.md](impa.md)). `wrap` is the rpak "wrapped filesystem", Apex's successor to VPK. It reuses the VPK file-type table from `vpk.h:16-23` (`wrap.cpp:170-172`).

## Summary
Respawn games ship loose Source-style files (`.bsp`, `.nut`/`.gnut` scripts, `.mdl`, `.vmt`/`.vtf`, `.txt`, `.res`, …) in Valve Pak files modified by Respawn: a directory file `<name>_dir.vpk` holding the file tree, plus numbered archives `<name>_NNN.vpk` holding the data. Respawn's version (2.3) differs from Valve's v1/v2: each file has a list of **data chunks**, each chunk with its own offset and compressed/decompressed size (LZHAM compressed in Titanfall, unverified). A user wants to extract files as-is (`.bsp` → hand to the [bsp](bsp.md) reader, text → text).

## Where it lives
- Loose files on disk, e.g. `vpk/englishclient_mp_common.bsp.pak000_dir.vpk` + `vpk/client_mp_common.bsp.pak000_000.vpk` (naming is **community knowledge, not in rsx**; verify on an install).
- rsx loads any path with extension `.vpk` given as input (`load.cpp:27-28` → `HandleVPKLoad`, `load.cpp:104-105`); its file dialog filters `*_dir.vpk` (`load.cpp:295`). It does not check the `_dir` suffix: an archive `_NNN.vpk` given directly fails the magic check.
- **FourCC:** `AssetType_t::VPKF = MAKEFOURCC('v','p','k','f')` (`asset.h:89`); `CVPKFile::GetAssetType` returns the MSVC literal `'fkpv'` (`vpk.h:62`), binding `.type = 'fkpv'` (`vpk.cpp:170`). Export folder name `"vpk"` (`asset.h:234`), unused (no export).
- **Names** are stored (the directory tree), never hashed or brute-forced. rsx builds `<path>/<name>.<ext>`, or `<name>.<ext>` when the path is empty or a single space (`vpk.cpp:40`). rsx's "GUID" for an entry is its **CRC32** (`vpk.h:63`, `vpk.cpp:61`), not a hash of the name.
- rsx container file name: `"n/a"` (`vpk.h:70`).

## Versions and detection
| magic (u32 LE) | major | minor | rsx |
|---|---|---|---|
| `0x55AA1234` (bytes `34 12 AA 55`) | 2 | 3 | parsed (`vpk.h:4-6`, `vpk.cpp:87-101`) |
| `0x55AA1234` | anything else (Valve v1, Valve v2, …) | | rejected: "Invalid file version" |

- rsx sets each entry's version to `{2, 3}` (`vpk.cpp:42`).
- **Proposed score (`file → vpk`)**: `u32le(0) == 0x55AA1234` → 10; `u16le(4) == 2 && u16le(6) == 3` → +2; `16 + dirTreeSize <= len` → +2; extension `vpk` → +1. Magic but other version → `UnsupportedVersion` (Valve VPKs are explicitly not planned, PROJECT.md §2). `Version { major: 2, minor: 3 }`. Header prefix: 16 bytes.
- An archive `_NNN.vpk` has no header (unverified): it scores `None` and is opened only through its directory.

## Layout
All little-endian, `#pragma pack(1)` (`vpk.h:25, 53`).

### `VPKDirHeader_s` (`vpk.h:26-35`, 16 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | magic | `0x55AA1234` |
| 0x04 | 2 | u16 | majorVer | 2 |
| 0x06 | 2 | u16 | minorVer | 3 |
| 0x08 | 4 | u32 | dirTreeSize | bytes of the tree after the header (rsx **never reads it**) |
| 0x0C | 4 | u32 | sigSize | comment "dunno what this means"; unused |

### Directory tree (`vpk.cpp:6-67`), starts at 0x10
Three nested levels of NUL-terminated strings, each level ended by an empty string:
```
loop:                       // extensions
  ext = cstr; if ext == "" break
  loop:                     // paths of this extension
    path = cstr; if path == "" break
    loop:                   // files of this path
      name = cstr; if name == "" break
      VPKEntryBlock_s       (8 bytes)
      loop:                 // data chunks
        VPKDataChunk_s      (30 bytes)
        u16 term            // 0xFFFF = last chunk; any other value = another chunk follows
```
Path `" "` (single space) = root (`vpk.cpp:40`). Terminator constant `VPK_CHUNK_TERM = UINT16_MAX` (`vpk.h:7`), checked at `vpk.cpp:57`. Which non-`0xFFFF` value appears between chunks (community: `0`) is unknown.

### `VPKEntryBlock_s` (`vpk.h:37-42`, 8 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | fileCRC | CRC32 of the file (algorithm/scope unverified: whole decompressed file assumed) |
| 0x04 | 2 | u16 | preloadSize | preload bytes. rsx ignores them; where they are stored (inline after the chunks in the tree, as in Valve VPK?) is **unknown**. Not skipped by rsx, so rsx assumes 0. |
| 0x06 | 2 | u16 | archiveIdx | archive number `NNN` for all chunks of this file. **rsx reads but drops it** (not stored in `CVPKFile`). Valve's `0x7FFF` = "in the dir file" convention: unknown for Respawn. |

### `VPKDataChunk_s` (`vpk.h:44-52`, 30 bytes, packed)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | loadFlags | shown in the preview only (`vpk.cpp:145`); meaning unknown |
| 0x04 | 2 | u16 | texFlags | shown in the preview only; meaning unknown |
| 0x06 | 8 | u64 | dataOffset | offset in the archive (`_NNN.vpk`), unverified |
| 0x0E | 8 | u64 | cmpSize | stored size |
| 0x16 | 8 | u64 | dcmpSize | size after decompression |

### File type table (`vpk.h:9-23`)
`VPKFileType_e { UNKNOWN = 0, BSP = 1, TEXT = 2 }`; by extension: `.bsp` → BSP; `.txt .nut .gnut .res .ent` → TEXT; rest UNKNOWN. rsx uses it only to choose a preview. Shared with `wrap`.

## Data outside the header
Not handled by rsx. Unverified expectations to confirm at the gate:
- File data = concatenation of its chunks in order; each chunk read from `<stem>_<archiveIdx:03>.vpk` at `dataOffset`, `cmpSize` bytes, where `<stem>` is the dir file name with `_dir.vpk` removed **and** the language prefix (`english`, …) dropped (community knowledge).
- `cmpSize == dcmpSize` → stored; `cmpSize < dcmpSize` → **LZHAM** (Titanfall used LZHAM "alpha", dictionary 2^20, community knowledge). rsx has **no LZHAM code** (`thirdparty/` contains binka, directxtex, doctest, imgui, libopusenc, miniaudio, oodle, qrcodegen, rad_lzb_simple, radaudio, rapidjson, tek-steamclient, valvefilevdf; none is LZHAM).
- LZHAM reference: `richgel999/lzham_codec` (C++, MIT; the "alpha" bitstream used by Respawn is a different release, `lzham_alpha`, also MIT). A pure-Rust LZHAM decoder: **unknown** whether one exists that decodes the alpha bitstream; FFI bindings to the C code may be the fallback. Open question.
- Whether Apex VPKs differ (Apex ships few VPKs; most loose files moved to `wrap` assets in rpaks): unknown.

## Dependencies
None. Archives are companion files opened lazily (PROJECT.md §6); a missing one → `MissingDependency { item: <entry path>, file: <archive name> }`.

## Decoding
```
open(dir file):
  h = read(0..16); check magic/version (score)
  tree = read(16 .. 16 + dirTreeSize)       // one ranged read; never the archives
  parse tree as in Layout, bounds-checked against tree.len() (rsx doesn't check)
  entries[path] = { crc, preload_size, archive_idx, chunks: Vec<(offset, cmp, dcmp, load_flags, tex_flags)> }
read(entry, range):
  map range onto chunks by cumulative dcmp sizes
  for each touched chunk:
    stored  → ranged read of the archive, only the sub-range
    LZHAM   → read whole chunk (cmpSize), decompress to dcmpSize, slice
  preload bytes: unknown (see Open questions)
```

## Output
rsx: nothing exported; preview shows a table of chunks (index, loadFlags, texFlags, cmpSize, dcmpSize) (`vpk.cpp:116-163`).

Proposed respak:
- Format `vpk` (respawn crate): `from("file", VpkRead).extensions(["vpk"]).provides::<dyn Container>()`. The volume is a plain file tree: `englishclient_mp_common.bsp.pak000_dir.vpk/maps/mp_common.bsp`, `…/scripts/vscripts/foo.nut`. Default `entry` (`file { volume, path }`) is enough; a typed entry `vpk.entry { crc, archive, chunks }` only if a handler needs flags.
- `stat` from the tree (sum of `dcmpSize`), `list` from the tree; `read` as in Decoding. Cost: opening = header + tree; reading = the touched chunks only (a compressed chunk whole, documented).
- Files then go through normal detection (e.g. `.bsp` → `bsp`, text as is). An `extract` of the whole volume is just a glob copy.
- Optional: check `fileCRC` (CRC32) after reading a whole file → `Corrupt` on mismatch.

## rsx quirks
- Reads the whole `_dir.vpk` into memory (`vpk.cpp:78`); parses without any bounds check and ignores `dirTreeSize` (`vpk.cpp:16-66`).
- `archiveIdx` and `preloadSize` are dropped; files are not extractable.
- Uses the CRC32 as GUID into the global asset list (`vpk.cpp:61`): CRC collisions and duplicate files collide.
- `HandleVPKLoad` returns (stops loading all remaining files) when one path doesn't exist (`valvepak.cpp:17-18`); failures are `bool`, plus `assertm`.
- `headerAlignment = 4` in the binding is meaningless for a non-pak asset.
Drop all of these; keep the struct layouts, magic, version check and tree encoding.

## Reuse in respak
- Volume pattern: `starpak.rs` (`VolumeSource`, `volume_operator`, `Container`), core `volume.rs`.
- New: tree parser, archive name mapping, chunk-to-range mapping, LZHAM decoder (codec edge `compressed{lzham} → blob` per PROJECT.md §6, or called directly like RTech).
- Downstream: [bsp](bsp.md) for `.bsp`, [source-mdl](source-mdl.md) for `.mdl`.

## Tests
- Synthetic: header `34 12 AA 55 02 00 03 00 <treeSize> 00000000` + tree `"nut\0" "scripts/vscripts\0" "foo\0" <crc,0,0> <chunk 0, 0, 5, 5> FF FF "\0" "\0" "\0"` and an `_000.vpk` with 5 bytes; check list/stat/read and root path `" "`.
- Multi-chunk file (term value `0x0000` between chunks), version 2.2 → `UnsupportedVersion`, Valve v1 → `UnsupportedVersion`, truncated tree → `Corrupt`, missing archive → `MissingDependency`.
- Real (no Steam runs without permission): Titanfall 2 `vpk/englishclient_mp_common.bsp.pak000_dir.vpk`; compare the listing with rsx's.

## Open questions
1. Archive file naming (language prefix, `_NNN` width) and `archiveIdx == 0x7FFF` meaning: verify on a real install.
2. Where preload bytes live, and whether any Respawn VPK has `preloadSize != 0`.
3. Compression detection (`cmpSize < dcmpSize` → LZHAM?) and the exact LZHAM variant/parameters; pure-Rust decoder vs FFI vs porting `lzham_alpha` (MIT). Size depends on this.
4. Meaning of `loadFlags`, `texFlags`, `sigSize`; what value separates chunks.
5. Does Apex still use VPK 2.3 for anything worth extracting?
6. CRC32 check on read: yes/no.
