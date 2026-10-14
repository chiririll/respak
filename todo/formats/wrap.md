# wrap · Wrapped file (rpak-embedded loose file)
**rsx sources:** `game/rtech/assets/wrap.h` (flags, `WrapAssetHeader_v1_t`, `WrapAssetHeader_v7_t`, `WrapAsset` ctor), `game/rtech/assets/wrap.cpp` (`LoadWrapAsset`, `GetStreamedDataForWrapAsset`, `GetWrapAssetData`, `Wrap_GetPathWithSwappedExtensions`, `Wrap_StripVMExtensionFromPath`, `PostLoadWrapAsset`, `ExportWrapAsset`, `InitWrapAssetType`), `game/vpk/vpk.h:9-23` (file type table), `game/rtech/utils/utils.cpp:2252-2296` (`RTech::DecompressStreamedBuffer`, Oodle), `game/rtech/cpakfile.h:1232-1256` (starpak stream entry), `core/utils/exportsettings.h:56`, `core/features.h:10`, `core/main.cpp:95, 167` · **Games / versions:** Apex Legends; asset versions 1 and 7 (which seasons: unknown) · **Priority hint:** high value (every Apex script, `.bsp` and its lumps, `.ent`, `.res`, `.txt` lives here), **S–M**.

## Summary
**An rpak asset, not a VPK entry.** `asset.h:64-67` lists `wrap` under a "vpk" comment because it replaces VPK: Apex wraps loose game files (scripts, maps, configs) into rpak assets ("wrapped filesystem is the successor of vpk", `wrap.cpp:170`). Each asset is one file: a path, a size, and data in the pak or in a starpak, optionally Oodle compressed. The user extracts it as the original file.

## Where it lives
- rpak asset, FourCC `wrap` (`asset.h:65`); binding `.type = 'parw'` (MSVC literal, `wrap.cpp:352`), `headerAlignment = 8`.
- Data: page data (`data` pointer) or streamed in the mandatory starpak, falling back to the optional one (`wrap.cpp:55-73`).
- **Name stored** (v7): `path + skipFirstFolderPos` (`wrap.cpp:45-49`), added to rsx's name cache. v1 has no path → GUID only.
- Proposed respak volume name: `<name>.wrap`, e.g. `common.rpak/scripts/vscripts/foo.gnut.client.wrap`? See Open questions; the stored string can carry a "VM extension" (`.client`, `.server`, `.ui`) after the real file name (`wrap.cpp:143-151`).

## Versions and detection
rsx switches on the pak asset entry version (`wrap.cpp:24-43`):
| asset version | header | rsx |
|---|---|---|
| 1 | `WrapAssetHeader_v1_t`, 0x10 bytes | parsed |
| 7 | `WrapAssetHeader_v7_t`, 0x28 bytes | parsed |
| other | — | `assertm` + return without extra data (later calls crash) |

Proposed: `rpak.asset` kind `wrap` → score 10 (container-provided kind); version 1 or 7 → `Version { major: v, minor: 0 }`; others → `UnsupportedVersion`. Optional confirmation: `header_size` from the asset entry equals 0x10 / 0x28 (unverified that it always does).

## Layout
Natural alignment (no pack pragma). Pointers are page pointers (relocated by the rpak volume).

### `WrapAssetHeader_v1_t` (`wrap.h:8-13`, 0x10)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | size | data size (stored = decompressed; v1 is never compressed or streamed in rsx) |
| 0x04 | 4 | u32 | unk_4 | unused ("never used and we don't wanna accidently make a large file") |
| 0x08 | 8 | ptr | data | page pointer to the file bytes |

### `WrapAssetHeader_v7_t` (`wrap.h:15-31`, 0x28)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr | path | page pointer to a NUL-terminated path |
| 0x08 | 8 | ptr | data | page pointer to the bytes (non-streamed) |
| 0x10 | 4 | u32 | hash | unused by rsx (probably a path hash: unknown) |
| 0x14 | 4 | u32 | cmpSize | stored size |
| 0x18 | 4 | u32 | dcmpSize | file size |
| 0x1C | 2 | u16 | pathSize | length of the path up to the real file name, **without** the VM extension |
| 0x1E | 2 | u16 | skipFirstFolderPos | offset in `path` where the name used by rsx starts (first folder dropped) |
| 0x20 | 2 | u16 | fileNamePos | offset of the file name in `path`; unused |
| 0x22 | 2 | u16 | flags | see below |
| 0x24 | 2 | u16 | unk4 | unused |
| 0x26 | 2 | u8[2] | unk5 | unused |

Flags (`wrap.h:4-6`): `0x1` COMPRESSED, `0x4` PERMANENT ("seems to"), `0x10` STREAMED.

Derived (ctor, `wrap.h:44-63`):
- `shouldDecompress = cmpSize < dcmpSize`
- `skipSize = shouldDecompress ? 0 : cmpSize - dcmpSize` ("typically 2")
- `isCompressed = (flags & 1) && shouldDecompress` (flag set but cmp ≥ dcmp → not compressed)
- `isStreamed = flags & 0x10`
- `assert(cmpSize != dcmpSize)`: rsx doesn't know the equal case.

File types (`vpk.h:9-23`): `.bsp` BSP; `.txt .nut .gnut .res .ent` TEXT; else UNKNOWN. Extension taken after stripping the VM extension (`wrap.cpp:163-172`).

## Data outside the header
`GetWrapAssetData` (`wrap.cpp:75-118`):
```
wrapSize = isCompressed ? cmpSize : dcmpSize
if isStreamed:
    e = mandatory starpak entry of the asset (offset,size); if none: optional entry; if none: fail
    bytes = starpak[e.offset + skipSize .. + wrapSize]
else:
    buf = data
    if !isCompressed && (flags & 0x5) == 0x5: buf += 2       // build failure leftover, permanent only
    bytes = buf[0 .. wrapSize]
if isCompressed: bytes = Oodle decode to dcmpSize
size = dcmpSize
```
Compression: **Oodle** only (rsx passes `eCompressionType::OODLE`, never reads a codec field). respak: `oodle.rs` (`oozextract`).

## Dependencies
None in rsx. `.bsp` wraps are parsed with `CBSPData::PopulateFromPakAsset` in post-load (`wrap.cpp:176-186`), which fetches its lumps (other wrap assets, e.g. `*.bsp.0005.bsp_lump`) from the pak: see [bsp.md](bsp.md).

## Decoding
1. Parse v1/v7 header from `rpak.asset`; compute derived fields (above).
2. Name (v7): read C string at `path`, take from `skipFirstFolderPos`.
3. Data per the pseudocode; streamed → `.stream`, else `.opt` entry of the rpak volume, ranged read `skipSize .. skipSize + wrapSize`.
4. Oodle decode when `isCompressed`; check the output length is `dcmpSize` (`Corrupt` otherwise).
5. TEXT type: drop one trailing NUL if present (`wrap.cpp:236-239`).

## Output
rsx `ExportWrapAsset` (`wrap.cpp:191-273`): writes to `<export>/wrap/<name>`; with the default setting (`useOrigScriptExportExtensions = false`, `exportsettings.cpp:8`; UI toggle commented out, `render.cpp:318`) the VM extension is moved before the real extension: `foo.gnut` + `.client` → `foo.client.gnut` (`wrap.cpp:121-141`). UNKNOWN/TEXT → raw bytes (TEXT minus trailing NUL). BSP → `CBSPData::Export` (re-assembled BSP, see bsp.md).

Proposed respak: format `wrap` from `rpak.asset`, `provides::<dyn Blob>()` + `Metadata` (name, guid). Writing the blob gives the original file. The output name rule (swapped VM extension or original) is a target option. A `.bsp` wrap could additionally be `Container`/forwarded to `bsp` (later, bsp slice).

## rsx quirks
- Oodle call uses `bufSize = dcmpSize` as the input length while the input buffer holds `cmpSize` bytes: over-read (`utils.cpp:2261-2291`). On decode failure the compressed bytes are returned as is, with `dcmpSize` as size. respak: errors.
- Unknown version → no extra data, later null deref.
- Non-streamed path ignores `skipSize`, applies a fixed `+2` only for compressed+permanent; streamed path applies `skipSize`. Keep both behaviours, list them in the review; unclear if they are the same thing.
- Export returns `true`; BSP export depends on `HAS_BSP_SUPPORT` (`features.h:10`, true).
- Name added to the global cache; global setting for the extension swap.

## Reuse in respak
`rpak.asset` (header, `.mem`, `.stream`, `.opt`), `oodle.rs`, `Blob`/`Metadata` capabilities, `string_to_guid` for named paths.

## Tests
- Synthetic v7: path `"scripts/vscripts/foo.gnut.client"`, pathSize = len(`scripts/vscripts/foo.gnut`), skipFirstFolderPos = 8; stored data; compressed data (Oodle fixture from task 11c); streamed with `skipSize = 2` (cmp = dcmp + 2); flags 0x5 with cmp > dcmp (+2 path); v1; version 3 → `UnsupportedVersion`.
- Real (no Steam runs without permission): Apex `common.rpak` / `mp_rr_*.rpak` scripts and `.bsp_lump`s; compare bytes with rsx's export.

## Open questions
1. Volume entry naming: keep the stored name (`foo.gnut.client`) + `.wrap`, or the swapped name?
2. `cmpSize == dcmpSize` case (rsx asserts): treat as stored?
3. Is the `+2` / `skipSize` prefix a 2-byte header worth decoding?
4. Which games/seasons use v1 vs v7; meaning of `hash`, `unk4`, `unk5`, PERMANENT.
