# bpk · Bluepoint pak (`bpwf`)
**rsx sources:** `core/filehandling/bpk.cpp` (`HandleBPKLoad`), `game/bluepoint/bp_pakfile.h` (`bpkhdr_v6_t`, `bpkfile_v6_t`, `bpkpatch_t`, `CBluepointPakfile`, `CBluepointWrappedFile`), `game/bluepoint/bp_pakfile.cpp` (`CBluepointPakfile::ParseFromFile`, `LoadBluepointWrappedFileAsset`, `ExportBluepointWrappedFileAsset`), `core/filehandling/load.cpp:25, 102`, `core/utils/utils_general.h:31-32` (`SWAP32`), `game/asset.h:86-87` · **Games / versions:** Titanfall 1 / Apex ports by Bluepoint (Xbox 360 `XBAR` archives); only archive version 6 is handled · **Priority hint:** low-medium; **M** (container + LZX decompression; the compression library is the blocker).

## Summary
A `.bpk` is a Bluepoint "XBAR" (Xbox Archive) pak: a big-endian archive of files addressed by a SHA-1-prefix hash of their name, stored as chunks, optionally LZX (Xbox XMemCompress) compressed. rsx treats each file as a `bpwf` ("Bluepoint wrapped file") asset and exports it as a raw file. A user would convert a `.bpk` into a volume of its member files (names unknown, `0x<hash>` unless a name cache provides them).

## Where it lives
- **Loose file** with extension `.bpk` (`load.cpp:25`; also accepted by the Steam depot filter `steam_load.cpp:28`). Not an rpak asset.
- Magic: the first 4 bytes are the ASCII bytes `X B A R` (`BP_PAK_ID = MAKEFOURCC('X','B','A','R')`, compared as read without a swap). The rest of the header is **big-endian** (`SWAP32`).
- `bpwf` is rsx's FourCC for the member asset (`BPWF`, `asset.h:87`; type id `'fwpb'` / `0x66777062`, bytes `b p w f`). It is not stored anywhere in the file.
- Member name: **not stored.** A member has only the 8-byte `hash` (first 8 bytes of the SHA-1 of the filename). rsx's placeholder name is `bpkfile/0x<GUID hex>` (`bpk.cpp:46`); `LoadBluepointWrappedFileAsset` then calls `SetAssetNameFromCache`, replacing it with an original string from the cache DB (`g_cacheDBManager.TryGetEntry(guid)`) if that GUID is known. Name resolution is therefore by lookup table / brute force, not stored.

## Versions and detection
- `bpkhdr_short_t { id, version }` is read first. Version (big-endian i32) 6 = `BP_PAK_VER_R1` is the only one handled. Version 0 ("previously `0` (MGS)", comment in header) is mentioned and rejected (`assertm "version 0 currently unsupported"`). Anything else → `ParseFromFile` returns false / assert.
- Proposed score (`blob → bpk`): bytes `58 42 41 52` ("XBAR") → 10; `be_u32(4) == 6` → +2; extension `bpk` → +1. `XBAR` with another version → `UnsupportedVersion`. Mapping `Version { major: 6, minor: 0 }`.
- Platform note: the format is big-endian (Xbox 360). Whether PC/other platform variants exist: unknown.

## Layout
All fields **big-endian** i32 unless stated (rsx byte-swaps in place; the `hash` words are **not** swapped, they are copied as raw little-endian ints).

### `bpkhdr_v6_t` (`bp_pakfile.h:77`, 0x24 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | bytes | id | `XBAR` |
| 0x04 | 4 | i32 | version | 6 |
| 0x08 | 4 | i32 | fileCount | number of members |
| 0x0C | 4 | i32 | unk | unknown |
| 0x10 | 4 | i32 | chunkSize | maximum size of one chunk (also the LZX window size) |
| 0x14 | 4 | i32 | chunkCount | number of chunks in the archive |
| 0x18 | 4 | i32 | dataOffset | file offset of the chunk data |
| 0x1C | 4 | i32 | dataSize | size of the chunk data (not used by rsx) |
| 0x20 | 4 | i32 | patchCount | number of patched paks |

### Tables after the header (in this order, contiguous)
| Start | Entry | Count | Meaning |
|---|---|---|---|
| 0x24 | `bpkfile_v6_t`, 0x1C bytes | fileCount | file entries |
| 0x24 + 0x1C*fileCount | i32 | fileCount | per-file unknown value (swapped, otherwise unused) |
| next | i32 | chunkCount | compressed (stored) size of each chunk |
| next | `bpkpatch_t`, 0x50 bytes | patchCount | patch entries (only present if `patchCount != 0`) |

Per the header comment, "there is data after patches in paks with patches, patchdata?" — unknown.

### `bpkfile_v6_t` (`bp_pakfile.h:11-30`, 0x1C bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | i32[2] | hash | first 8 bytes of SHA-1(filename); **not byte-swapped**. rsx combines `hash[0]`, `hash[1]` as the 64-bit guid (`hash[0]` = low dword) |
| 0x08 | 4 | i32 | unk_8 | "flags?" ("weird guy"); unknown |
| 0x0C | 4 | i32 | chunkStart | index of the file's first chunk |
| 0x10 | 4 | i32 | decompressedSize | uncompressed file size |
| 0x14 | 4 | i32 | dataStartOffset | offset (from file start) of the file's first chunk data |
| 0x18 | 4 | i32 | dataEndOffset | end offset of the file's chunk data |

Derived: `IsCompressed = decompressedSize != dataEndOffset - dataStartOffset`; `compressedSize = IsCompressed ? end - start : decompressedSize`.

### `bpkpatch_t` (`bp_pakfile.h:32-47`, 0x50 bytes)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 64 | char[64] | filename | `BP_PAK_FILENAME_SIZE`; name of a patched pak |
| 0x40 | 16 | i32[4] | unk_40..4C | unknown (swapped) |

Patches are parsed but not otherwise used by rsx.

## Data outside the header
- **Chunk data** starts at `dataOffset`; chunk `i` is `chunkSize[i]` stored bytes, chunks are contiguous (`pChunkData`, `ParseFromFile`).
- File `f` consists of `chunkCount_f = decompressedSize > maxChunkSize ? decompressedSize / maxChunkSize + 1 : 1` consecutive chunks starting at `chunkStart` (rsx formula, see quirks).
- **Compression:** Microsoft Xbox XMemCompress **LZX** (`XMEMCODEC_LZX`), parameters `Flags = 0`, `WindowSize = chunkSize`, `CompressionPartitionSize = 524288` (512 KiB). A single decompression context is created per file and each chunk is decompressed with it in order (the LZX stream state carries across chunks). rsx uses the proprietary `xcompress.lib` (`XB_XCOMPRESS`, Windows only).
- No starpak, no embedded sub-blobs known.

## Dependencies
None. Members are independent; no cross-asset references are known. Names come from the rsx name cache (`rpak.guid`-like lookup by 64-bit hash, but the hash is SHA-1-prefix of the name, not rtech's guid hash). Which path strings hash to which members: unknown without a name list.

## Decoding
```
open(bytes):
  require bytes[0..4] == "XBAR"; version = be_i32(4); require 6
  hdr = parse header (be)
  files   = hdr.fileCount x bpkfile_v6 at 0x24
  unk     = hdr.fileCount x i32 after files
  csizes  = hdr.chunkCount x i32 after unk
  patches = hdr.patchCount x bpkpatch after csizes
  cur = hdr.dataOffset
  for i in 0..chunkCount: chunk[i] = {off=cur, size=csizes[i]}; cur += csizes[i]
for file f:
  guid = (hash[1] << 32) | hash[0]                 // raw LE words
  name = cache.lookup(guid) or format("bpkfile/0x{:X}", guid)
  n = decompressedSize > chunkSize ? decompressedSize / chunkSize + 1 : 1
  out = []
  ctx = f.IsCompressed ? lzx_context(window = chunkSize, partition = 524288) : none
  for i in 0..n:
    c = chunk[f.chunkStart + i]
    out += ctx ? lzx_decompress(ctx, c) : raw(c)
  truncate/expect out.len == decompressedSize
```
Open decoding detail: how a non-compressed file's last chunk is sized; rsx copies `chunk.dataSize` and trusts `decompressedSize` for the buffer (overflow risk if they disagree).

## Output
rsx (`ExportBluepointWrappedFileAsset`): writes each member's decompressed bytes to `<export>/<name>` (or `<export>/bpkfile/<stem>/<name>` when `exportPathsFull` is off); extension `.bin` is added when the name has none. Without `XB_XCOMPRESS` compressed members are exported **still compressed** (raw chunk concatenation) into a buffer of the decompressed size. No preview, no JSON.
Proposed respak: format `bpk` providing `Container` (volume of members named `<cache name or 0x<hash>>`, with `.bin` fallback), reading the (compressed) chunks lazily, plus an LZX decoder (a Rust LZX/XMemCompress implementation must be written or found; `lzxd`-style decoders are for Microsoft CAB/WIM LZX and differ in framing, so compatibility with XMemCompress is **unverified**). Capabilities: `Container` (existing in `respak-core/src/capability/`). No JSON shape.

## rsx quirks
- `ParseFromFile` mutates the loaded buffer in place (`hdr->swap()`); the id check is done unswapped against a LE-built FourCC (works because it is ASCII).
- Only version 6 handled; the `unk` table, `dataSize`, `unk_8`, and the patch table are parsed and ignored.
- `HandleBPKLoad`: on a missing path it `return`s (abandoning remaining files) and leaves the progress bar pending; the loop over assets is inside the `BP_PAK_VER_R1` branch, the progress counter is incremented once per pak.
- `m_chunkCount` formula adds one chunk when `decompressedSize` is an exact multiple of `chunkSize` (e.g. exactly 2 chunks → computes 3), potentially reading one chunk too many (into the next file); exactness vs. real data **unverified**.
- Per-file chunk walk assumes chunks of one file are contiguous and in order; the debug `assertm(filedata == firstChunk->data)` is the only check that `dataStartOffset` and `chunkStart` agree.
- Output buffer of `decompressedSize` is written by `memcpy_s(curpos, chunk->dataSize, ...)` with no bound check against the buffer end.
- `GetAssetVersion().majorVer == 0` path asserts "currently unsupported" (MGS-era archives, per comment).
- Windows-only `xcompress.lib` dependency; with `XB_XCOMPRESS` undefined the exporter silently emits compressed bytes.
- File names exist only through the name cache DB.
Keep: header/table layout, chunk model, LZX parameters. Drop: in-place swap, formula bug (verify first), silent compressed fallback (should be an error).

## Reuse in respak
Container/volume pattern (`starpak.rs`, core `volume.rs`), `rpak.guid`-style hash lookups only if a name list is wired in (hash type differs: SHA-1 prefix). New: XBAR parser, LZX decoder (the main work), name cache hookup.

## Tests
- Synthetic (all big-endian): header `XBAR`, version 6, fileCount 1, unk 0, chunkSize 0x10, chunkCount 2, dataOffset = 0x24 + 0x1C + 4 + 8, dataSize 0x14, patchCount 0; one file entry with hash `{0x11223344, 0x55667788}`, chunkStart 0, decompressedSize 0x14, dataStartOffset = dataOffset, dataEndOffset = dataOffset + 0x14 (uncompressed, so `IsCompressed == false`); chunk sizes `{0x10, 0x04}`; chunk data 20 bytes. Expect guid `0x5566778811223344` and the 20 bytes back, name `bpkfile/0x5566778811223344`.
- Edge: version 0 or 7 → `UnsupportedVersion`; wrong magic; `decompressedSize` exactly a multiple of `chunkSize` (check the chunk-count formula); `patchCount > 0`; truncated tables/chunks (`Corrupt`); `chunkStart` out of range.
- Compressed case needs a real sample (no LZX encoder fixture here).
- Real (no Steam runs without permission): any `.bpk` from the Bluepoint-ported Titanfall/Apex builds; which files exist in the shipped depots is not established by rsx.

## Open questions
1. Which games ship `.bpk` and are they all version 6 (Xbox 360 big-endian)? Support for version 0 (MGS) is out of scope?
2. LZX: write a decoder compatible with XMemCompress (framing, chunk-state carry-over, `CompressionPartitionSize` meaning) or use a third-party crate; verification needs real data.
3. Name resolution: where does the hash→name table come from in respak (rsx has a cache DB)?
4. Meanings of header `unk`, per-file `unk_8`, the per-file unknown i32 table, `dataSize`, and what follows the patch table.
5. Chunk-count formula: is the exact-multiple case an rsx bug?
