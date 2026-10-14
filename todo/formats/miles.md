# asrc, aevt (mbnk / mstr / mprj) · Miles audio banks, streams, sources, events
**rsx sources:** `game/audio/miles.{h,cpp}` (`CMilesAudioBank::ParseFile`, `ParseFromHeader`, `DiscoverStreamingFiles`, `IsValidSource`, `GetStreamingFileNameForSource`, `MilesBank_ParseSources<T>`, `MilesBank_ParseEvents`), `game/audio/source.{h,cpp}` (`MilesSource_v*_t`, `ReadAudioStream`, `DecodeAudioDataForSource`, `ExportAudioSourceAsset`), `game/audio/event.{h,cpp}`, `game/audio/event_action.cpp`, `game/audio/actions/action_play.cpp`, `game/audio/project.{h,cpp}`, `game/audio/wavefile.h`, `core/filehandling/{mbnk,load,list}.cpp`, `thirdparty/rad_lzb_simple/rad_lzb_simple.c`, `core/features.h` · **Games / versions:** Titanfall 2 (bank v13), Apex Legends (bank v28–v49, see the table) · **Priority hint:** high value (every game sound), M for sources (+ the codecs in [miles-codecs.md](miles-codecs.md)), S–M extra for events (JSON only, partly understood).

Codec decoding (Bink Audio `BCF1`, RAD Audio `RADA`) is **not** in this file: see [miles-codecs.md](miles-codecs.md). This file ends where the encoded stream is handed to a decoder (§Decoding, step 5).

## Summary
Respawn games use RAD Game Tools' **Miles Sound System**. A sound bank is split over several loose files in one directory:
- `<bank>.mbnk` (magic `CBNK`): the bank. Tables of **sources** (one encoded sound each: name, sample rate, where its bytes are), **events** (what the game triggers: named action lists that pick sources, set controllers, run other events), markers, graphs, a string table.
- `<bank>_stream.mstr`, `<bank>_stream_patch_<n>.mstr`, `<bank>_<language>.mstr`, `<bank>_<language>_patch_<n>.mstr` (magic `CSTR`): the audio bytes of the sources. Non-localised sources live in `_stream`, voice lines in one file per language; patches add files with a higher patch index.
- `audio.mprj` (Apex) / `titanfall_2.mprj` (TF2) (magic `CPRJ`): the Miles project: controllers, buses, the language list. rsx uses it only for its UI.

rsx shows two asset kinds from a bank: `asrc` (audio source, exportable to WAV or Opus) and `aevt` (audio event, preview only, export disabled). A user converts sources to `wav`; events would become a JSON description (which sources an event can play, controllers it sets, events it runs).

## Where it lives
- **Loose files, not in an rpak.** rsx loads a `.mbnk` given on the command line, from the file dialog, or every `.mbnk` of a directory given as input (`core/filehandling/load.cpp:15-29`, `:95-96` → `HandleMBNKLoad`, `core/filehandling/mbnk.cpp:10-33`). The `.mstr` files and the `.mprj` are looked up **in the same directory as the `.mbnk`**; rsx assumes nothing else about the install layout.
- Apex install layout (community knowledge, **not encoded in rsx**, verify on a real install): `audio/ship/general.mbnk`, `audio/ship/general_stream.mstr`, `audio/ship/general_stream_patch_<n>.mstr`, `audio/ship/general_<language>.mstr` (+ `_patch_<n>`), `audio/ship/audio.mprj`. Titanfall 2 location: unknown (rsx expects `titanfall_2.mprj` next to the bank, `miles.cpp:380`).
- **FourCC** (`game/asset.h:83-84`): `ASRC = MAKEFOURCC('a','s','r','c')`, `AEVT = MAKEFOURCC('a','e','v','t')`; the type bindings register the same values as MSVC multi-char literals `'crsa'` (`source.cpp:577`) and `'tvea'` (`event.cpp:542`). rsx shows them as `asrc` / `aevt` with version = bank version (`miles.cpp:180`, `:230`).
- **Names** are stored in the bank (string table), never brute-forced. rsx also hashes each name with `RTech::StringToGuid` to get a lookup GUID (`miles.cpp:179`, `:229`); the GUID is **not** stored in any file.
- **Container file name** that rsx shows/lists (`core/filehandling/list.cpp:14` writes `type,guid,container,name`):
  - source: the `.mstr` file it lives in, built by `GetStreamingFileNameForSource` (§Decoding step 3), e.g. `general_stream_patch_3.mstr`;
  - event: `<bank stem> + ".mbnk"` (`miles.cpp:232`), where the bank stem is the **first string of the string table** (`GetBankStem`, `miles.h:559`), not the file name on disk.
- Proposed respak volume (see §Output): `audio/ship/general.mbnk/<source name>.asrc`, `audio/ship/general.mbnk/<source name>.<lang>.asrc` for localised sources, `audio/ship/general.mbnk/<event name>.aevt`.

## Versions and detection
All structures are little-endian. Magic values in rsx are MSVC multi-character literals compared against a `u32` read from the file: `'ABCD'` = `0x41424344`, so the bytes on disk are reversed (`'CBNK'` → bytes `K N B C`). The on-disk byte strings below are derived from that rule; check them on a real file.

| File | Magic (u32 value) | Bytes on disk | Version field | rsx check |
|---|---|---|---|---|
| `.mbnk` | `'CBNK'` = `0x43424E4B` | `4B 4E 42 43` ("KNBC") | `i32` at +4 | `miles.cpp:363-369` |
| `.mstr` | `'CSTR'` = `0x43535452` | `52 54 53 43` ("RTSC") | `u16` at +4, must be `2` | `miles.cpp:102` |
| `.mprj` | `'CPRJ'` = `0x4350524A` | `4A 52 50 43` ("JRPC") | `i32` at +4 | `project.cpp:42-48` |
| codec header in `.mstr` | `'BCF1'` = `0x42434631` / `'RADA'` = `0x52414441` | `31 46 43 42` / `41 44 41 52` | see miles-codecs.md | `source.cpp:73-86` |

**Bank versions.** rsx dispatches on the version field only (`miles.cpp:240-350`); the game is implied by it. The history comment is `miles.h:293-311` (v48/v49 dates from `miles.cpp:310`, `:327`).

| Bank version | Game / season | Header layout | Source layout | Language table | Events parsed | rsx |
|---|---|---|---|---|---|---|
| 11 | TF2 tech test | — | — | — | — | unsupported (`default: return false`) |
| 13 | TF2 release | `v13` | `v13` | `LANG_R2` (11) | yes (uncompressed) | `miles.cpp:244-261` |
| 28 | Apex release → S2.x | `v28` | `v28` | `LANG_R5_9` (9) | no | `:262-286` |
| 32 | S2.x → S3.1 | `v28` | `v28` | 9 | no | idem; comment: "valid for sources and the header but any other data has not been checked" |
| 33, 34, 36, 38 | S3.1 → S7.2 | `v28` | `v28` | 9 | no | idem |
| 39, 40, 42, 43, 44, 45, 46 | S8.0 → S24.1 | `v45` | `v39` | `LANG_R5_10` (10) | no | `:287-309` |
| 48 | S27.1 (build 2025_12_05) | `v45` | `v48` | 10 | no | `:310-326` |
| 49 | S30.0 (build 2026_07_29) | `v49` | `v49` (= `v48` layout) | 10 | yes (LZB compressed) | `:327-344` |
| anything else (29–31, 35, 37, 41, 47, ≥50, <0) | — | — | — | — | — | `return false`; logged "unimplemented file version" |

**Proposed score functions.**
- `mbnk`: `u32le(0) == 0x43424E4B` → 10; version in the table above → +2, and `u32le(8)` (fileSize) equal to the probe length → +2 (unverified that it always is: open question). Magic matches but version unknown → `UnsupportedVersion` (§5.3). `Version { major: bank version, minor: 0 }`; the layout variant is a pure function of `major` (table), so `minor` stays 0. Header prefix needed: `0xC8` bytes (largest header, v45).
- `mstr`: `u32le(0) == 0x43535452` → 10, `u16le(4) == 2` → +2; other version → `UnsupportedVersion`. `Version { major: 2, minor: 0 }`. Prefix `0x14`.
- `mprj`: `u32le(0) == 0x4350524A` → 10; `version == 0x2E (46)` → +2, others `UnsupportedVersion` (rsx parses only 0x2E, `project.cpp:62`). `Version { major: 46, minor: 0 }`.
- `asrc` / `aevt`: read edges from the typed entries the mbnk volume hands out (`miles.source`, `miles.event`); score 10 because the container provides the kind.

## Layout
`OffsetPtr_t` (`miles.h:12-18`) is an 8-byte field holding a **byte offset from the start of the file** (the runtime replaces it with a pointer); the upper 4 bytes are 0 on disk (comment: "the offset is actually 32-bit"). Fields called `ofs` below are `OffsetPtr_t` unless typed otherwise. `unk`/`gap` fields are unknown to rsx.

### MBNK common prefix (`MilesBankHeaderShort_t`, `miles.h:313-318`)
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 4 | u32 | magic `'CBNK'` |
| 0x04 | 4 | i32 | version |
| 0x08 | 4 | u32 | fileSize |

### MBNK header v13 (`MilesBankHeader_v13_t`, `miles.h:321-372`, size 0xC0, from RoyalBlue's Kilometer)
| Off | Size | Type | Field | Used by rsx |
|---|---|---|---|---|
| 0x00 | 12 | | common prefix | |
| 0x0C | 4 | i32 | bankMagic | no |
| 0x10 | 8 | ofs | bankName | no |
| 0x18 | 8 | u64 | nextBank (runtime) | no |
| 0x20 | 8 | u64 | sharedState (runtime) | no |
| 0x28 | 8 | u64 | bankLoader (runtime) | no |
| 0x30 | 8 | ofs | loadedStreamFiles ("runtime only?") | no |
| 0x38 | 8 | ofs | loadedLanguageStreamFiles ("runtime only?") | no |
| 0x40 | 8 | ofs | sourceNames | no |
| 0x48 | 8 | ofs | **sources** (array of `MilesSource_v13_t`) | yes |
| 0x50 | 8 | ofs | localisedSources | no |
| 0x58 | 8 | ofs | markers | **no** (see quirks) |
| 0x60 | 8 | ofs | **eventNames** (array of `EventName_s`) | yes |
| 0x68 | 8 | ofs | **eventData** | yes |
| 0x70 | 8 | ofs | **strings** (string table) | yes |
| 0x78 | 8 | ofs | **graphData** | yes (event preview) |
| 0x80 | 8 | ofs | sourceSelectors | no |
| 0x88 | 8 | ofs | filters | no |
| 0x90 | 8 | ofs | unk_90 | no |
| 0x98 | 4 | u32 | unk_98 | no |
| 0x9C | 4 | u32 | **localisedSourceCount** | yes |
| 0xA0 | 4 | u32 | **sourceCount** | yes |
| 0xA4 | 4 | u32 | patchCount | no |
| 0xA8 | 4 | u32 | **eventCount** | yes |
| 0xAC | 4 | u32 | stringsSize | no |
| 0xB0 | 4 | u32 | dword_B0 | no |
| 0xB4 | 4 | u32 | graphDataSize | no |
| 0xB8 | 4 | u32 | unkSize | no |
| 0xBC | 4 | u32 | **buildTag** | yes |

### MBNK header v28 (`MilesBankHeader_v28_t`, `miles.h:374-411`, size 0xC0) — versions 28–38
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 12 | | common prefix |
| 0x0C | 4 | i32 | bankMagic |
| 0x10 | 4 | u32 | unk_10 |
| 0x14 | 4 | u32 | unk_14 |
| 0x18 | 8 | u64 | reserved_project |
| 0x20 | 16 | | unk_20 |
| 0x30 | 8 | ofs | unk_offset_30 |
| 0x38 | 8 | ofs | unk_offset_38 |
| 0x40 | 8 | ofs | unk_offset_40 |
| 0x48 | 8 | ofs | **sources** (array of `MilesSource_v28_t`) |
| 0x50 | 8 | ofs | localisedSourceOffset (reserved) |
| 0x58 | 8 | ofs | unk_offset_58 |
| 0x60 | 8 | ofs | eventNames (read, events not parsed for these versions) |
| 0x68 | 8 | ofs | eventData (idem) |
| 0x70 | 8 | ofs | **strings** |
| 0x78 | 8 | ofs | graphData |
| 0x80 | 8 | ofs | unk_offset_80 |
| 0x88 | 8 | ofs | unk_offset_88 |
| 0x90 | 4 | u32 | unk_90 |
| 0x94 | 4 | u32 | **localisedSourceCount** |
| 0x98 | 4 | u32 | **sourceCount** |
| 0x9C | 4 | u32 | patchCount |
| 0xA0 | 4 | u32 | eventCount |
| 0xA4 | 12 | | gap_a4 |
| 0xB0 | 4 | u32 | **buildTag** |
| 0xB4 | 12 | | gap (named `gap_a8` in rsx) |

### MBNK header v45 (`MilesBankHeader_v45_t`, `miles.h:437-486`, size 0xC8) — versions 39–46 and 48
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 12 | | common prefix |
| 0x0C | 4 | i32 | bankMagic |
| 0x10 | 4 | u32 | **buildTag** |
| 0x14 | 4 | u32 | bankHash (stored, unused) |
| 0x18 | 8 | ofs | bankNameOffset (unused) |
| 0x20 | 8 | u64 | reserved_project (0 on disk) |
| 0x28 | 16 | | unk28 (0) |
| 0x38 | 8 | ofs | unk_offset_38 |
| 0x40 | 8 | ofs | unk_offset_40 |
| 0x48 | 8 | ofs | unk_offset_48 ("SourceTableOffset") |
| 0x50 | 8 | ofs | **sources** ("SourceEntryOffset"; array of `MilesSource_v39_t`, or `v48_t` for bank v48) |
| 0x58 | 8 | ofs | localisedSourceOffset (null on disk) |
| 0x60 | 8 | ofs | unk_offset_60 |
| 0x68 | 8 | ofs | eventNames |
| 0x70 | 8 | ofs | eventData |
| 0x78 | 8 | ofs | unk_offset_78 |
| 0x80 | 8 | ofs | **strings** |
| 0x88 | 8 | ofs | graphData |
| 0x90 | 8 | ofs | unk_offset_90 |
| 0x98 | 8 | ofs | unk_offset_98 |
| 0xA0 | 4 | u32 | unk_a0 |
| 0xA4 | 4 | u32 | **localisedSourceCount** |
| 0xA8 | 4 | u32 | **sourceCount** ("unlocalised source count") |
| 0xAC | 4 | u32 | patchCount (comment: bank needs `<bank>_stream_patch_1.mstr` … `_patch_(patchCount-1).mstr` and the language equivalents; rsx does not use it) |
| 0xB0 | 4 | u32 | eventCount |
| 0xB4 | 20 | | unk_b4 … unk_c4 |

### MBNK header v49 (`MilesBankHeader_v49_t`, `miles.h:489-536`, size 0xA0) — version 49
Note the 32-bit offsets and the swapped count order.
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 12 | | common prefix |
| 0x0C | 4 | i32 | bankMagic |
| 0x10 | 4 | u32 | **buildTag** |
| 0x14 | 4 | u32 | bankHash |
| 0x18 | 1 | u8 | bankIdx |
| 0x19 | 3 | | gap |
| 0x1C | 4 | u32 | **eventCount** |
| 0x20 | 12 | u32[3] | unk_20 |
| 0x2C | 4 | u32 | **eventDataOffset** (file offset, "compressed event data") |
| 0x30 | 4 | u32 | unkOffset |
| 0x34 | 4 | u32 | **sources** (file offset; array of `MilesSource_v49_t`) |
| 0x38 | 4 | u32 | **sourceCount** (unlocalised) |
| 0x3C | 4 | u32 | **localisedSourceCount** |
| 0x40 | 8 | | gap_40 |
| 0x48 | 8 | ofs | namePtr (unused) |
| 0x50 | 8 | ofs | unk_50 |
| 0x58 | 8 | ofs | **audioMarkers** (array of `MilesAudioMarker_t`) |
| 0x60 | 8 | | gap_60 |
| 0x68 | 8 | ofs | **eventNames** |
| 0x70 | 8 | ofs | **strings** |
| 0x78 | 8 | ofs | **graphData** |
| 0x80 | 24 | | gap_80 |
| 0x98 | 8 | ptr | reserved_memoryBank (runtime) |

### Total source entry count (`miles.h:668`, same for every version)
```
langCount  = len(language table for this version)       // see "Language tables"
entryCount = header.sourceCount + header.localisedSourceCount * (langCount - 1)
```
The source array holds `entryCount` consecutive entries. The formula implies that `header.sourceCount` already includes one language's localised sources (probably English) and the other languages follow; rsx does not document the order. **Depends on the hard-coded language count**, see quirks / open questions.

### Source entries (`game/audio/source.h`)
`MilesSource_v13_t` (`source.h:16-46`, 0x58 bytes; comment "unverified offsets" from 0x18):
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 16 | | gap0 |
| 0x10 | 4 | u32 | nameOffset (into string table) |
| 0x14 | 2 | u16 | sampleRate |
| 0x16 | 2 | u16 | bitRate (unused) |
| 0x18 | 1 | u8 | channelCount (unused, unverified) |
| 0x19 | 2 | | gap |
| 0x1B | 1 | u8 | markerCount |
| 0x1C | 19 | | gap |
| 0x2F | 1 | | (padding) |
| 0x30 | 4 | u32 | streamHeaderSize |
| 0x34 | 4 | u32 | sampleCount |
| 0x38 | 8 | u64 | streamHeaderOffset |
| 0x40 | 8 | u64 | streamDataOffset |
| 0x48 | 4 | u32 | markerOffset |
| 0x4C | 4 | u32 | unk_4C |
| 0x50 | 2 | i16 | languageIdx (`-1` = not localised) |
| 0x52 | 2 | i16 | patchIdx |
| 0x54 | 4 | | gap |

`MilesSource_v28_t` (`source.h:49-75`, 0x58 bytes):
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 12 | | gap0 |
| 0x0C | 2 | u16 | languageIdx (`0xFFFF` = not localised) |
| 0x0E | 2 | u16 | patchIdx |
| 0x10 | 4 | u32 | nameOffset (string table) |
| 0x14 | 2 | u16 | sampleRate |
| 0x16 | 2 | u16 | bitRate |
| 0x18 | 1 | | unk |
| 0x19 | 1 | u8 | markerCount |
| 0x1A | 1 | u8 | channelCount (unused) |
| 0x1B | 21 | | gap |
| 0x30 | 4 | u32 | streamHeaderSize |
| 0x34 | 4 | u32 | sampleCount |
| 0x38 | 8 | u64 | streamHeaderOffset |
| 0x40 | 8 | u64 | streamDataOffset |
| 0x48 | 4 | u32 | markerOffset |
| 0x4C | 4 | u32 | unkMinusOne |
| 0x50 | 8 | | gap_end |

`MilesSource_v39_t` (`source.h:78-104`, 0x48 bytes) — banks 39–46:
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 4 | u32 | nameOffset (string table) |
| 0x04 | 2 | u16 | languageIdx |
| 0x06 | 2 | u16 | patchIdx |
| 0x08 | 4 | u32 | unk8 |
| 0x0C | 2 | u16 | sampleRate |
| 0x0E | 2 | u16 | bitRate |
| 0x10 | 1 | | unk |
| 0x11 | 1 | u8 | markerCount |
| 0x12 | 8 | | gap |
| 0x1A | 2 | u16 | bpm (0 = none) |
| 0x1C | 4 | | gap |
| 0x20 | 4 | u32 | streamHeaderSize |
| 0x24 | 4 | u32 | sampleCount |
| 0x28 | 8 | u64 | streamHeaderOffset |
| 0x30 | 8 | u64 | streamDataOffset |
| 0x38 | 4 | u32 | markerOffset |
| 0x3C | 4 | u32 | unkMinusOne |
| 0x40 | 8 | | gap |

`MilesSource_v48_t` / `MilesSource_v49_t` (`source.h:107-134`, 0x50 bytes; v49 is an empty subclass, same layout):
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 8 | u64 | nameOffset (v48: special, see below; v49: string table) |
| 0x08 | 2 | u16 | languageIdx |
| 0x0A | 2 | u16 | patchIdx |
| 0x0C | 4 | u32 | unk8 |
| 0x10 | 2 | u16 | sampleRate |
| 0x12 | 2 | u16 | bitRate |
| 0x14 | 1 | | unk |
| 0x15 | 1 | u8 | markerCount |
| 0x16 | 8 | | gap |
| 0x1E | 2 | u16 | bpm |
| 0x20 | 4 | | gap |
| 0x24 | 4 | u32 | streamHeaderSize |
| 0x28 | 8 | u64 | sampleCount |
| 0x30 | 8 | u64 | streamHeaderOffset |
| 0x38 | 8 | u64 | streamDataOffset |
| 0x40 | 4 | u32 | markerOffset |
| 0x44 | 4 | u32 | unkMinusOne |
| 0x48 | 8 | | gap |

What rsx keeps of a source (`MilesSource_t`, `source.h:136-193`): nameOffset, streamDataOffset, streamHeaderOffset, sampleCount, streamHeaderSize, languageIdx, patchIdx, bpm (0 for v13/v28), sampleRate, markers. `channelCount` and `bitRate` are **not** kept; channels come from the codec header.

### Source names (`miles.cpp:164-175`)
- v13, v28, v39 layouts, v49: `name = cstring(strings + nameOffset)`.
- v48 only: the source names have their own string block **directly after the source array**:
  ```
  nameBlock = sources + 0x50 * entryCount
  name      = cstring(nameBlock + (src.nameOffset - sources[0].nameOffset))
  ```

### Markers (`MilesAudioMarker_t`, `miles.h:57-62`, 16 bytes)
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 4 | u32 | nameOffset (string table) |
| 0x04 | 4 | u32 | framePosition (sample frame) |
| 0x08 | 8 | | unk |

A source's markers are `markers[src.markerOffset / 16 + j]` for `j < src.markerCount` (`miles.cpp:151-162`). rsx reads them **only when the bank's marker pointer is set, which only the v49 `Construct` does** (`miles.h:731`); see quirks.

### String table and bank stem
`strings` points to NUL-terminated strings addressed by byte offset. The string at offset 0 is the **bank stem** (e.g. `general`), used to build `.mstr` names (`miles.h:558-559`). rsx never bounds-checks offsets (v13 has `stringsSize` at 0xAC; other versions: the size is unknown).

### Language tables (`miles.cpp:246-250`, `:272-275`, `:295-298`)
- `LANG_R2` (v13): `english, french, german, spanish, italian, japanese, polish, portuguese, russian, tchinese, mspanish` (11)
- `LANG_R5_9` (v28–38): `english, french, german, spanish, italian, japanese, polish, russian, mandarin` (9)
- `LANG_R5_10` (v39–49): `english, french, german, spanish, italian, japanese, polish, russian, mandarin, korean` (10)

`languageIdx` (from the source and from the `.mstr` header) indexes this table. After parsing, if the `.mprj` loads and its language count differs, rsx replaces the table with the project's (`miles.cpp:392-397`), but by then all names are already built (quirks).

### MSTR header (`MilesStreamHeader_t`, `miles.h:65-75`, 0x14 bytes; "it seems that this struct has never changed")
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 4 | u32 | magic `'CSTR'` |
| 0x04 | 2 | u16 | version (must be 2) |
| 0x06 | 2 | u16 | languageIdx (`0xFFFF` = the non-localised `_stream` file) |
| 0x08 | 4 | u32 | **streamDataOffset**: base of all sources' data in this file |
| 0x0C | 4 | i32 | patchIdx |
| 0x10 | 4 | u32 | buildTag (must equal the bank's buildTag) |

Nothing else of the `.mstr` layout is known to rsx: a source's codec header and data are addressed by the offsets in the bank (§Decoding).

### Events (`EventName_s`, `miles.h:77-81`, 8 bytes; array of `eventCount` at `eventNames`)
| Off | Size | Type | Field |
|---|---|---|---|
| 0x00 | 4 | u32 | nameOffset (string table) |
| 0x04 | 4 | u32 | dataOffset (relative to `eventData`) |

Event data at `eventData + dataOffset` (`miles.cpp:207-223`):
- v13: the action list starts directly (uncompressed, no size header; "Event data became compressed with version 10.0.29 (May 9, 2018)").
- v49 (and presumably every version after 13, unverified since rsx parses events only for 13 and 49): `u16 decompressedSize`, `u16 compressedSize`, then `compressedSize` bytes. If the sizes are equal the bytes are the raw action list, else RAD LZB (§Decoding, events).

**Action list:** actions follow each other; each starts with a header that gives its size in dwords (header included); the action with `isLastAction` set ends the list (`event.cpp:56-129`).

`EventActionBase_v13_s` (`event.h:19-24`, 2 bytes, v13):
| Off | Bits | Field |
|---|---|---|
| 0x00 | 0–3 | actionType |
| 0x00 | 4–7 | isLastAction (non-zero = last) |
| 0x01 | 8 bits | dataSizeDwords (u8) |

`EventActionBase_s` (`event.h:26-41`, 4 bytes, v49):
| Off | Bits | Field |
|---|---|---|
| 0x00 | 0–3 | actionType |
| 0x00 | 4 | isLastAction |
| 0x00 | 5–7 | pad |
| 0x01 | u8 | unk_1 (a count: number of play states in action 0, controller names in action 0xB) |
| 0x02 | u16 | dataSizeDwords |

Action types (`event.h:6-17`): `0` play, `1` (unused in r5 as of S30), `2` unk (has an event-like name), `8` run events, `9` set controller value, `0xA` unk, `0xB` referenced controllers, `0xC` unk, `0xD` controller count. Seen in files: 0, 2, 8, 9, 10, 11, 13.

Action layouts (offsets from the start of the action, header included; v49 unless stated):
- **Action 8, run events** (`event.h:255-261`): `0x04 u32 eventCount`, `0x08 u32 b`, `0x0C u32 eventNameOffset[eventCount]` (string table, names of other events).
- **Action 8, v13** (`event.h:247-253`; rsx offsets are +2 after its header upgrade): `0x08 u32 eventNameOffset` (one event only). rsx's preview reads the v49 struct for every version, see quirks.
- **Action 9, set controller value** (`event.h:264-271`): `0x04 f32 duration`, `0x08 f32 delay`, `0x0C f32 newValue`, `0x10 i32 controllerNameOffset` (string table), `0x14 i32 unk`.
- **Action 0xB, referenced controllers** (`event.h:273-276`): `0x04 u32 controllerNameOffset[unk_1]`.
- **Action 0xD** (`event.h:278-282`): `0x04 u8 unk`, `0x05 u8 controllerCount`.
- **Action 2** (`event.h:235-244`): `0x04 u32 someNameOffset` (`0xFFFFFFFF` = none; looks like an event name, starts with `/general/`), `0x08 u8 flags`, `0x09 u8`, `0x0A i16`, `0x0C u32`, `0x10 f32`, `0x14 u32`.
- **Action 0, play, v49** (`EventAction_0_s`, `event.h:201-233`, 0x9C bytes): `0x20 u32 unkBankOffset_20`, `0x34 u8 unkCount_34`, `0x36 u16`, `0x3C GraphValue pitch`, `0x40 GraphValue volume`, `0x44 f32`, `0x48 f32`, `0x4C GraphValue (flag 0x200)`, `0x50 GraphValue (flag 0x400)`, `0x6C i32 unkGraphOffset`, `0x72 u16`, `0x74 u16`, `0x76 u16 sourceStatesOffset` (dwords from action start), `0x78 u16 stateSelectorRefsOffset` (rsx: `unkDwordOffset_78`, dwords), `0x7A u16 sourceSelectorOffset` (dwords), `0x7C u16`, `0x7E u16`, `0x80 u32 graphFlags`, `0x84` 24 unknown bytes. Everything else unknown.
- **Action 0, play, v13** (`EventAction_0_v13_s`, `event.h:149-199`, packed; offsets here are in the **original** v13 action, i.e. rsx's field names): `0x3A` 4-bit playRouteCount + 4 bits, `0x3B u8 metaContentCount` (0 in r2), `0x3C i32 sourceSelectorOffset`, `0x40 f32`, `0x44 i32 filterOffset`, `0x50 GraphValue initialOcclusion (flag 0x2)`, `0x54 startDelay (0x20)`, `0x58 pitch (0x4)`, `0x5C volume (0x1)`, `0x60 (0x40)`, `0x64 (0x80)`, `0x68 (0x200)`, `0x6C (0x8)`, `0x70 (0x10)`, `0x74 i32 duckingNameOffset`, `0x7C u8 dynamicExtraDataSize`, `0x88 u32 graphFlags`, `0x8C` dynamicExtraData. Floats at 0x04–0x20 and the rest: unknown meaning. rsx does not parse v13 play states (call commented out, `action_play.cpp:9`).
- `GraphValue` (`event.h:45-49`): 4 bytes, either an `f32` value or an `i32` byte offset into the bank's `graphData`; it is a graph offset when the field's bit is set in `graphFlags` (`ACT_GRAPHFLAG_VOLUME = 1<<0`, `PITCH = 1<<2`, `4C = 1<<9`, `50 = 1<<10`, `event.h:59-62`).

**Play states and source selectors (v49 only, `event.cpp:131-281`).** This is how an event references sources.
`PlayActionState` (`event.h:65-87`, 80 bytes): `0x00 i32 nameOffset` (string table, e.g. "NewState", "State 1"), `0x04 u8 selectorRefCount` (rsx `unkCount_4`), `0x06 u8 selectorRefBase` (rsx `unsigned___int86`), `0x08 u8 extraA`, `0x09 u8 extraB`, `0x0E u16 flagsE` (rsx `wordE`), `0x12 u16 flags12` (rsx `word12`), `0x40 i32 dword40`, `0x44 i32 dword44`, rest unknown.
Selector reference (`struct_v1`, `event.h:89-94`, 36 bytes): `0x00 i32 selectorOffset` (`-1` = none), 32 unknown bytes.
`SourceSelector_s` (`event.h:96-109`): `0x00` bits 0–2 type (0 = a source, else a list), bits 3–6 unk; `0x01 u8 weight`; `0x02 u16 childCount`; `0x04 i32 sourceIndex` (rsx `unk_4`; `-1` = "(no source)"); `0x0C u16 childOffsets[childCount]` (dwords from the **root** selector). Bytes 0x08–0x0B: unknown.

### Graph data (`miles.h:135-291`, event preview only)
At `graphData + offset`: `MilesValueGraph_s` = `u8 numPoints, u8 unk, u16 unk, u32 baseControllerNameOffset` (`0xFFFFFFFF` = none), then `f32 x[numPoints]`, then `numPoints` segments `MilesGraphSegment_s` (20 bytes: `f32 start, f32 end, f32 paramA, f32 paramB, u8 type, 3 pad`). Segment types: 0 linear, 1 bezier-like (handles at angles `paramA/paramB`), 2 constant, 3–6 curves (sqr, dB, level-dB, cube; bit 0x10 = inverted), sampling code at `miles.h:157-245`. Only needed if respak exports graphs (not proposed).

### MPRJ header (`MilesProjectHeader_v46_s`, `project.h:108-162`) — project version 0x2E only
| Off | Type | Field | Off | Type | Field |
|---|---|---|---|---|---|
| 0x00 | u32 | magic `'CPRJ'` | 0x80 | ofs | unk_80 |
| 0x04 | i32 | version (0x2E) | 0x88 | ofs | unk_88 |
| 0x08 | u32 | fileSize | 0x90 | ofs | functions |
| 0x0C | u32 | hash | 0x98 | ofs | **languages** (`i32[languageCount]` string offsets) |
| 0x10 | ofs | **controllers** | 0xA0 | ofs | unk_A0 |
| 0x18 | ofs | **strings** | 0xA8 | ofs | unk_A8 |
| 0x20 | ofs | graphData | 0xB0 | ofs | convolutionSettings |
| 0x28 | ofs | **buses** | 0xB8 | ofs | convolutionSizes |
| 0x30 | ofs | busNames | 0xC0–0xD8 | ofs | unk_C0, C8, D0, D8 |
| 0x38 | ofs | busIndices | 0xE0 | u32 | convolutionSettingsCount |
| 0x40 | ofs | **busExports** (`i32 pairs {nameOffset, busIdx}`) | 0xE4 | u32 | unkCount_E4 |
| 0x48 | ofs | filters | 0xE8 | u32 | **languageCount** |
| 0x50 | ofs | appliedDucking | 0xEC | u32 | voiceLimitCount |
| 0x58 | ofs | duckNames | 0xF0 | u32 | duckCount |
| 0x60 | ofs | ducks | 0xF4 | u32 | **controllerCount** |
| 0x68 | ofs | bankNames | 0xF8 | u32 | **busCount** |
| 0x70 | ofs | unk (rsx `unk_68`) | 0xFC | 2 | gap |
| 0x78 | ofs | voiceLimitNames | 0xFE | u8 | hasAllBusNames |
| | | | 0xFF | u8 | hasAllUnkNames |
| | | | 0x100 | u8 | exportedBusCount |
| | | | 0x101 | u8 | unkCount_101 (more fields follow, unknown) |
Comment: offsets after `buses` "may be inaccurate as they are based on the titanfall 2 version". Controllers `MilesController_v46_s` (`project.h:20-32`, 0x30: `u32 nameOffset, f32 min, f32 max, f32 default, 0x2B i8 type`), buses `MilesBus_v46_s` (`project.h:49-78`, 0x80). `MilesController_v13_s` (0x1C) exists but no v13 project is parsed. Language names: `strings + languages[i]`.

## Data outside the header
- **All bank data is in the `.mbnk`**, addressed by file offsets; no pointer relocation beyond "offset from file start". rsx reads the whole `.mbnk` (`FileSystem::ReadFileData`, `miles.cpp:358`).
- **Audio bytes are in the `.mstr` files**, per source: a codec header block and a data block (§Decoding). Not compressed by the container; the codec is the compression.
- **Event action lists** (v49, likely all > 13) are compressed with **RAD LZB simple** (`thirdparty/rad_lzb_simple`), sizes in a 4-byte prefix. No Oodle / zstd / RTech anywhere in Miles.
- The `.mprj` is a separate optional file.

## Dependencies
- `.mbnk` → `.mstr`: by **file-name convention** in the bank's directory (step 3 below), checked against the `.mstr` header (`buildTag`, `languageIdx`, `patchIdx`). Needed to decode a source; not needed to list sources or events.
- `.mbnk` → `.mprj`: `audio.mprj` (bank version > 13) or `titanfall_2.mprj` (≤ 13) in the same directory (`miles.cpp:379-397`). Optional; a failed load is logged and ignored.
- Event → sources: by **source index** (`SourceSelector_s.sourceIndex`); event → events: by name (action 8); event → controllers: by name (actions 9, 0xB). All inside one bank.
- No rpak GUIDs. rsx's `RTech::StringToGuid(name)` GUID is only rsx's lookup key. Maps onto respak linking (§5.5) as: the `asrc` reader asks for the `.mstr` it needs (ranged reads on the host operator); the `aevt` JSON encoder resolves source indexes within the same bank. No `rpak.guid` involvement.

GUID hash, if respak wants rsx-compatible GUID paths (`respak_respawn::string_to_guid`, `crates/respak-respawn/src/guid.rs`, ported from `game/rtech/utils/utils.cpp:2548`):
```
fn string_to_guid(s):
    bytes = s up to the first NUL
    h = 0; i = 0
    loop:
        word[0..4] = for k in 0..4: (b = bytes[i+k] if present else 0; b = '/' if b == '\\'; b & 0xDF)
        has_end = any of bytes[i..i+4] missing
        v = (u64(u32_le(word)) * 0x0FB8C4D96501) >> 24          // wrapping
        if has_end: return h*0x0633D5F1 + v - 0x00AE502812AA7333*len(bytes)   // wrapping
        h = h*0x0633D5F1 + v; h ^= h >> 61; i += 4
```

## Decoding
### Bank (`CMilesAudioBank::ParseFile`, `miles.cpp:352-400`)
1. Read the file. `magic != 'CBNK'` → not a bank; `version < 0` → fail; version not in the table → `UnsupportedVersion`.
2. Pick header/source layout and language table by version; read `buildTag`, `sourceCount`, `localisedSourceCount`, `eventCount`, and the offsets of `sources`, `eventNames`, `eventData`, `strings`, `graphData`, (v49) `audioMarkers` (`Construct`, `miles.h:662-733`). `entryCount` as in §Layout.
3. **Streams present** (`DiscoverStreamingFiles`, `miles.cpp:85-127`): rsx opens **every** `*.mstr` in the directory, reads its 20-byte header, skips it unless `magic == 'CSTR' && version == 2 && buildTag == bank.buildTag && patchIdx <= 31`, then sets bit `patchIdx` in `streamStates` (if `languageIdx == 0xFFFF`) or in `localisedStreamStates[languageIdx]`.
4. **Sources** (`MilesBank_ParseSources<T>`, `miles.cpp:129-189`): for each of the `entryCount` entries: decode the layout; drop it (count as "invalid") unless `IsValidSource` (`miles.cpp:64-83`):
   ```
   if src.languageIdx != 0xFFFF:
       require localisedStreamStates has src.languageIdx and bit src.patchIdx set
   require streamStates bit src.patchIdx set          // also for localised sources (quirk)
   ```
   then read markers, the name, and the stream file name (below). The asset version is the bank version.
5. **Events** (`MilesBank_ParseEvents`, `miles.cpp:191-237`, only v13 and v49, behind `HAS_MILES_EVENTS = true`, `core/features.h:11`): for each `EventName_s`: name from the string table, data pointer and sizes as in §Layout. Actions are parsed lazily on preview.
6. Load the `.mprj` (optional), possibly replace the language table.

### Stream file name of a source (`GetStreamingFileNameForSource`, `miles.cpp:47-62`)
```
name = bankStem                                     // string table offset 0, e.g. "general"
name += (src.languageIdx != 0xFFFF) ? "_" + languages[src.languageIdx] : "_stream"
name += (src.patchIdx != 0) ? "_patch_" + decimal(src.patchIdx) + ".mstr" : ".mstr"
// general_stream.mstr, general_stream_patch_2.mstr, general_english.mstr, general_french_patch_1.mstr
path = bank directory / name
```

### Source payload → decoder (`DecodeAudioDataForSource`, `source.cpp:57-286`; reader `ReadAudioStream`, `source.cpp:19-43`)
1. Open the `.mstr` at `path`; read its 20-byte header (rsx does **not** re-check magic/buildTag here).
2. **Codec header block** = `mstr[src.streamHeaderOffset .. src.streamHeaderOffset + src.streamHeaderSize]` (absolute file offsets).
3. **Codec id** = `u32le` of the first 4 bytes of that block: `0x52414441` (`'RADA'`, RAD Audio, decoder type `MILES_DECODER_RADA = 6`) or `0x42434631` (`'BCF1'`, Bink Audio, `MILES_DECODER_BINKA = 2`) (`source.cpp:73-86`, `miles.h:5-6`). Anything else → "Unsupported audio format". Comment: RAD Audio from Apex S24 onwards, Bink Audio before (and TF2).
4. **Codec data block** starts at the absolute offset `mstrHeader.streamDataOffset + src.streamDataOffset` (`source.cpp:123`). Its length is `codecFileSize - src.streamHeaderSize` (`source.cpp:131`), where `codecFileSize` is the total size of the encoded "file" as stated **in the codec header**. rsx reads it as `u32` at offset 0x18 of the opened decoder's state: for Bink that slot is explicitly filled with the `BCF1` header's `fileSize` (`u32` at header +0x10, `miles_bcf.cpp:6-17`, `:147`); for RAD Audio the state is the opaque `RadAContainer` and rsx's comment-free read presumably hits `RadAFileHeader.file_size` (`u64` at header +0x18, `thirdparty/radaudio/rada_file_header.h:18-47`; open question). respak should read the size from the codec header itself (miles-codecs.md).
5. **Hand-off:** the decoder sees one logical byte stream = header block ‖ data block (the reader serves the header bytes first, then seeks to the data block, `source.cpp:23-42`). It is given:
   - codec id (step 3);
   - the header block (`streamHeaderSize` bytes) for metadata parsing (`ASI_stream_parse_metadata`, `source.cpp:113`), which returns **channels, sample rate, frame count** and decoder sizes. These values, **not** the bank's `sampleRate` / `sampleCount`, are used for output;
   - the data byte range (step 4).
   The block-by-block decode loop, de-interleaving (RADA output padded to `maxSamplesPerDecode` per channel, Bink not) and sample format (Bink: 16-bit PCM, `decodeFormat = 0`; RADA: f32, `decodeFormat = 2`) are described in [miles-codecs.md](miles-codecs.md). Output is clamped to the codec's frame count.
6. The bank's `sampleRate` / `sampleCount` are used only for the UI duration; `bpm` and markers only for UI.

### Event actions (`MilesEvent_s::ParseActions`, `event.cpp:56-129`)
```
v13:  cursor = data
      loop: h = 2-byte header; size = h.dataSizeDwords * 4
            action = bytes[cursor .. cursor+size]; cursor += size
            if h.isLastAction != 0: break
v49:  raw = (decompSize == compSize) ? data[0..decompSize]
                                     : lzb_decode(data[0..compSize], decompSize)   // must consume exactly compSize
      cursor over raw with the 4-byte header, same loop
```
rsx copies a v13 action into a buffer 2 bytes larger and writes a 4-byte generic header in front, so rsx's v13 field offsets are "+2 from their name" (`event.h:146-148`, `event.cpp:68-81`). respak should keep the v13 bytes as they are and use the original offsets given in §Layout.

RAD LZB simple decoder (`rr_lzb_simple_decode`, `rad_lzb_simple.c:227-434`), byte-wise reading (rsx's fast path copies speculatively in 4/8-byte words; the result is the same):
```
excess(cp):                                  // up to 5 bytes
    b = next; if b < 192: return b
    v = b;  b = next; v += b << 6
    if b >= 128: b = next; v += b << 13
        if b >= 128: b = next; v += b << 20
            if b >= 128: b = next; v += b << 27
    return v

lzb_decode(comp, rawLen):
    if len(comp) == rawLen: return comp
    out = []
    loop:
        c = next; lrl = c & 0xF; mlc = c >> 4
        if lrl == 15: lrl += excess()
        if lrl > 8 and len(out) + lrl >= rawLen:          // final literal run (encoder forces lrl >= 9 on it)
            if lrl == 9 (not escaped): lrl = rawLen - len(out)   // "false 9"
            out += next lrl bytes; break
        out += next lrl bytes
        if mlc < 15:
            off = u16le(next 2); ml = mlc + 4
        else:
            b = next
            if b & 0x80:                                  // short-offset (overlapping) match
                m = (b & 0x7F) >> 3; ml = m + 4; if m == 15: ml += excess()
                off = b & 7
            else:
                off = u16le(next 2); ml = 19 + (b & 0x7F); if (b & 0x7F) == 127: ml += excess()
        repeat ml: out.push(out[len(out) - off])          // byte-wise, overlap-safe
    require len(out) == rawLen; return out, bytes consumed
```
Note: the escaped-literal (`lrl == 15`) path also ends the stream when `len(out) + lrl >= rawLen`. With `lrl ≤ 8` there is no end check (the encoder never ends that way).

### Event → sources (v49 play action, `EventAction_0_s::GetSourceStates`, `event.cpp:227-281`)
```
state = action + 4 * sourceStatesOffset
for i in 0 .. header.unk_1:
    if state.flagsE & 1: break
    if (state.flags12 & 0x20) or state.dword44 == 0: break
    selectors = []
    if state.selectorRefCount != 0:
        name = string(state.nameOffset)
        if state.flags12 & 0x10: break        // state dropped
        if state.dword40 == 0: break          // state dropped
        ref = action + 4 * (stateSelectorRefsOffset + state.selectorRefBase)     // 36-byte records
        repeat selectorRefCount: if ref.selectorOffset != -1:
                                     selectors.push(parse(action + 4 * (sourceSelectorOffset + ref.selectorOffset)))
                                 ref += 36
    emit state { name (empty if no refs), selectors }
    state += ((state.flagsE & 0x100) ? 104 : 80) + 36 * (state.extraA + state.extraB)

parse(sel, root = sel):
    if sel.type == 0: leaf { sourceIndex = sel.sourceIndex (-1 = none), weight }
    else: list { type, weight, children = [parse(root + 4 * sel.childOffsets[k], root) for k < childCount] }
```
`sourceIndex` is an index into the bank's source entries (rsx looks it up in its list of *valid* sources, a bug, see quirks). Weights are relative among siblings. The meaning of list types ≠ 0 (random, sequential, …) is unknown.

## Output
**rsx:**
- `asrc` export (`ExportAudioSourceAsset`, `source.cpp:399-570`), setting `WAVE` or `Opus` (`source.h:3-13`):
  - path: export dir + (`exportPathsFull` global ? the name's parent path : `audio/`) + the name's file name + `.wav` / `.opus`;
  - WAV (`wavefile.h`): 44-byte RIFF header, `formatTag = 3` (IEEE float) when the decoder gives f32 (RADA), else `1` (PCM16, Bink); `blockAlign = dataSize / sampleCount`, `bitsPerSample = dataSize*8 / sampleCount / channels`; interleaved samples;
  - Opus via libopusenc: channels > 2 remapped from WAV to Vorbis order (`{3: 0,2,1}`, `{5: 0,2,1,3,4}`, `{6: 0,2,1,4,5,3}`, `{7: 0,2,1,5,6,4,3}`, `{8: 0,2,1,6,7,4,5,3}`), mapping family 1 for surround, fixed bitrates 160/224/256/320/384/448 kbit/s for 3–8 channels, tags TITLE/COMMENT/COPYRIGHT/GAME.
- `aevt` export: **disabled** (`ExportAudioEventAsset` returns `false`, `event.cpp:458-462`); an RSON writer for actions 2/8/9/0xB exists (`event_action.cpp`) but is only referenced from the commented-out code.
- Preview only: playback, markers, BPM, play-action source trees and pitch/volume graphs.

**Proposed respak:**
- Crate `respak-miles` (planned in `todo/tasks/14-backlog.md`), plugin id `miles`. Codecs: per [miles-codecs.md](miles-codecs.md) (same crate or a codec edge).
- Formats and edges:
  ```rust
  r.format("mbnk").label("Miles sound bank").from("file", MbnkRead).extensions(["mbnk"]).provides::<dyn Container>();
  r.format("mstr").label("Miles stream file").from("file", MstrRead).extensions(["mstr"]);   // header check only
  r.format("mprj").label("Miles project").from("file", MprjRead).extensions(["mprj"]);       // optional, languages
  r.format("miles.source");                // typed entry handed out by the mbnk volume
  r.format("miles.event");                 // typed entry handed out by the mbnk volume
  r.format("asrc").label("Miles audio source").from("miles.source", AsrcRead).extensions(["asrc"])
      .provides::<dyn Audio>().provides::<dyn Metadata>();
  r.format("aevt").label("Miles audio event").from("miles.event", AevtRead).extensions(["aevt"])
      .provides::<dyn Metadata>().to("aevt.json", AevtJsonEncode);
  r.format("aevt.json").label("Audio event (JSON)").extensions(["json"]);
  ```
  `asrc → wav` comes for free through the existing `Audio` capability edge (`crates/respak-formats/src/wav.rs`, 16-bit PCM; RADA's float output is quantised: open question). Opus: not proposed now.
- **mbnk volume** (implements `VolumeSource`, like `starpak.rs`):
  ```
  general.mbnk/
    <source name>.asrc                    non-localised source
    <source name>.<lang>.asrc             localised source (language from the table), e.g. <name>.english.asrc
    <event name>.aevt                     event (leading '/' of the stored name trimmed)
    0x<GUID>.asrc / 0x<GUID>.aevt         optional rsx-compatible GUID alias, not listed
  ```
  Each `.asrc` entry is a typed `miles.source { bank_version, name, language: Option<String>, patch, sample_rate, sample_count, bpm, markers: Vec<{name, frame}>, stream: file { host op, path of the .mstr }, header_range: Range<u64>, data_start: u64 }`. `data_start = mstr.streamDataOffset + src.streamDataOffset` needs the `.mstr` header, so it is resolved lazily by `AsrcRead` (one 20-byte ranged read per `.mstr`, cacheable). The entry's bytes (`stat`/`read` on the volume path) can be the codec header block, so the volume never reads audio data on listing. `.aevt` entries are `miles.event { bank_version, name, actions: raw bytes or a range + sizes }`.
- **`AsrcRead`** → `AsrcSource` (native type): opens the `.mstr` by convention (§Decoding), checks its header (`magic`, `version == 2`, `buildTag`, `languageIdx`, `patchIdx` against the source; mismatch → `Corrupt`, missing → `MissingDependency{item, file}`), reads the codec header block, gets the codec id, channels, rate, frames and total size from it, then implements `Audio::tracks()` (one track) and `samples()` by streaming ranged reads of the data block into the codec (`cx.blocking`, cancellation between blocks). `Metadata`: name, GUID, markers, bpm, language, stream file.
- **`aevt.json`** (proposal): `{ "name": "...", "actions": [ {"type": "play", "states": [ {"name": "...", "selectors": [ {"source": "<name>", "weight": 255} | {"list": <type>, "weight": n, "children": [...]} ]} ]}, {"type": "runEvents", "events": ["..."]}, {"type": "setControllerValue", "controller": "...", "duration": 0.0, "delay": 0.0, "value": 0.0}, {"type": "referencedControllers", "controllers": ["..."]}, {"type": "unnamed_<n>", "size": <bytes>} ] }`. The action type names follow rsx's `s_eventExportTypes` (`event.h:285-291`). Only for bank versions whose action layouts are known (13 partially, 49); others → `UnsupportedVersion`.
- Capabilities: `Container` (mbnk), `Audio` + `Metadata` (asrc), `Metadata` (aevt). No new core capability.

## rsx quirks
- **Whole-file read** of the `.mbnk` (`miles.cpp:358`). Banks are small compared to streams, so respak may do the same, documented in the doc comment (§6), or read header + source array + string table with ranged reads (string table size unknown except v13).
- **Scans every `.mstr` in the directory** and reads its header to build the "present" bitmasks (`miles.cpp:92-124`), matching only by `buildTag`, not by file name. Two banks of the same build in one directory can mark each other's streams as present. respak: derive the file name by convention and check that one file only, lazily, when a source is read.
- **Invalid sources are silently hidden** (`miles.cpp:144-149`); the only trace is the "invalid" count in the UI. respak: list every source; reading one whose stream is missing → `MissingDependency`. Whether to hide them in listings is an open question.
- `IsValidSource` also requires the **non-localised** patch bit for localised sources (`miles.cpp:79`), so a voice line in `general_english_patch_2.mstr` is hidden unless `general_stream_patch_2.mstr` exists too. Probably a bug; respak checks only the file the source lives in.
- `1 << patchIdx` with `patchIdx > 31` is UB in `IsValidSource` (only the `.mstr` side is guarded, `miles.cpp:110-113`).
- `GetLanguageNames()[languageIdx]` is unchecked (`miles.cpp:52`): an index past the hard-coded table is out of bounds. respak: `Corrupt` / fall back to `lang<idx>`.
- **The source count depends on a hard-coded language count** (`miles.h:668`). If a game update adds a language without a bank version bump, every source after the first language block is misread. The `.mprj` language list replaces the table **after** the sources and stream names are already built (`miles.cpp:392-397`), so it has no effect. respak: take the language count from the `.mprj` when it parses, else the table; open question.
- **Markers are read only for v49.** The other `Construct` overloads never set `audioMarkers`, and the member is never initialised (`miles.h:542`, `:656`), so `GetMarkers() != nullptr` (`miles.cpp:154`) reads an indeterminate pointer for v13–v48 (UB, may crash or read garbage). The v13 header does have a `markers` offset at 0x58 that is ignored. respak: markers for v49 only until v13/v45 marker tables are verified.
- **Event source selectors index the wrong list**: `ParsedSourceSelector::Construct` looks up `bank->GetSourceName(sourceIndex)` (`event.cpp:140`), but `sourceNames` contains only the *valid* sources (`miles.cpp:184`), so indexes shift once any source was dropped, and `.at()` throws past the end. respak: index the full source entry array.
- Event preview for action 8 always uses the v49 struct (`event.cpp:419`) and action 0xB too, even for v13 banks (their v13 header is upgraded to 4 bytes but the fields stay at their v13 offsets +2). Play preview: v13 reads graphs only; states only for v49 (`action_play.cpp:75-81`).
- Events are parsed only for v13 and v49; v28–v48 events (eventNames/eventData offsets are read) are ignored.
- `bankHash` is only stored for v45/v49 and never used; `bankMagic`, `fileSize`, `patchCount` never checked.
- The `.mstr` header is not re-validated when decoding (`source.cpp:62`).
- **Payload size comes from the decoder's internal state** (`*(uint32_t*)(container + 0x18)`, `source.cpp:131`): rsx's own Bink wrapper repurposes a field to make this work (`miles_bcf.cpp:147`), RADA relies on the opaque `RadAContainer` layout. respak: read the size from the codec header.
- The bank's `sampleRate` / `sampleCount` / `channelCount` are ignored for decoding; codec header values win. A mismatch is never reported.
- WAV header is filled with `long`/`unsigned long` fields (`wavefile.h`); fine on Windows (32-bit `long`), not portable. `blockAlign`/`bitsPerSample` are computed from data size, which is wrong if the decoder stopped early.
- Opus `GAME` tag: `version >= 13 ? "Apex Legends" : "Titanfall 2"` (`source.cpp:522`) labels TF2 (v13) banks as Apex.
- Global settings: `g_rsxSettings.exportPathsFull` (export layout), `g_audioPlayer` (preview), `HAS_MILES_EVENTS` compile flag, `MILES_NO_RADAUDIO` (builds without the RAD Audio decoder). None of these should be ported.
- Event container name is `stem + ".mbnk"` from the string table, not the actual file name (`miles.cpp:232`).
- Failures are `bool` + log lines everywhere; respak maps them to `UnsupportedVersion` / `Corrupt` / `MissingDependency`.

## Reuse in respak
- `respak_core::capability::{Container, Audio, Metadata}` (`crates/respak-core/src/capability/`), `respak_core::volume::{volume_operator, VolumeSource, VolumeEntry, VolumeStat}` (pattern: `crates/respak-respawn/src/starpak.rs`).
- `respak-formats` `wav` (`WavEncode`, `from_cap::<dyn Audio>`) for output; no change needed.
- `respak_respawn::string_to_guid` for rsx-compatible GUID aliases (dependency on `respak-respawn`, or move the 40-line hash to core: open question).
- Score helpers in core (`magic`, `u32le`, …).
- New: the `respak-miles` crate (mbnk/mstr/mprj readers, the mbnk volume, `asrc`, `aevt`, `aevt.json`), an LZB decoder (~80 lines, above), the codecs from [miles-codecs.md](miles-codecs.md).

## Tests
Synthetic fixtures (all little-endian, built in test code):
- **mstr header:** 20 bytes `52 54 53 43 | 02 00 | FF FF | <streamDataOffset u32> | <patchIdx i32> | <buildTag u32>`; variants: version 1 → `UnsupportedVersion`; `languageIdx = 0` (english).
- **mbnk v49:** 0xA0-byte header (magic, version 49, fileSize, buildTag, eventCount 1, sources at 0xA0, sourceCount 2, localisedSourceCount 1, strings/eventNames/eventData/audioMarkers offsets), then `2 + 1*(10-1) = 11` source entries of 0x50 bytes (one non-localised, the rest localised with languageIdx 0..9), a string table starting with `"general\0"`, one marker, one event whose data is `u16 n, u16 n, <uncompressed action list>` with an action 8 (last) naming another event. Check: entry names and language folders, stream names (`general_stream.mstr`, `general_english.mstr`, `general_korean_patch_1.mstr`), markers, GUIDs.
- **mbnk v48:** same with names in the block after the source array and `nameOffset` relative to `sources[0].nameOffset`.
- **mbnk v45 (v39 sources), v28, v13:** header + sources only; v13 with an uncompressed 2-byte-header action list (type nibble, last nibble, dword count).
- **Unknown version** (e.g. 47) → `UnsupportedVersion`; wrong magic → not scored.
- **Count overflow:** `entryCount * entrySize` past the file end → `Corrupt`; string offset out of range → `Corrupt`.
- **LZB:** round-trip fixtures: a stream with only a final literal run (`control = 0x09`, "false 9"), an escaped literal run (`lrl = 15` + excess bytes), a short match (`mlc < 15`), an overlapping match (`0xF?` control + excess1 byte with bit 7, offset 1 = run-length), a long match with `excesslow = 127` + excess. Hand-encode them from the format above (or port `LZB_Output` for tests only).
- **Source payload:** a fake `.mstr` with a `BCF1`/`RADA` header block at `streamHeaderOffset` and data at `streamDataOffset + src.streamDataOffset`; assert the ranges handed to the codec and that only those ranges are read (byte-counting operator).
- **Missing stream** → `MissingDependency { file: "general_stream_patch_1.mstr" }`; `.mstr` with a different `buildTag` → `Corrupt`.

Real files (local install only, **no Steam runs without permission**): Apex `audio/ship/general.mbnk` + its `.mstr` files and `audio.mprj` (current season: v49 + RADA; older dumps for v28–v46 + BCF1); Titanfall 2 bank (v13) if available. Compare source lists and WAVs against rsx.

## Open questions
1. ~~Source naming in the volume~~ **decided (user, 2026-10-14): `<name>.<lang>.asrc`** for localised sources (the earlier `<language>/<name>.asrc` proposal is dropped); non-localised sources stay `<name>.asrc`. Still unknown: are localised source names identical across languages? (unknown, likely). Do source names contain `/` or `\`? (unknown; rsx's `exportPathsFull` suggests they may have path parts).
2. Language count: trust the hard-coded per-version tables (rsx) or the `.mprj` `languageCount` when present? Which `.mprj` versions pair with banks other than 49? (rsx parses only project v0x2E.)
3. Order of the source array (which language block comes first; whether `header.sourceCount` includes the English localised sources): unknown, inferred from the count formula only.
4. ~~List or hide sources whose `.mstr` is missing~~ **decided (user, 2026-10-14): list every source.** Loading is lazy, so listing can't check that a stream exists; reading one whose `.mstr` is missing → `MissingDependency`. No `.mstr` directory scan, no `stat` at listing.
5. Markers for v13–v48: are the v13 `markers` offset (0x58) and v45 `unk_offset_*` the marker table? Unknown; rsx reads markers only for v49.
6. Event layouts for v28–v48: unknown (rsx doesn't parse them). Ship `aevt` for 13/49 only, or skip events in the first slice?
7. RADA total size: confirm that rsx's `container + 0x18` read equals `RadAFileHeader.file_size` (low 32 bits); respak should read it from the header either way.
8. Whole read of the `.mbnk` acceptable (banks are small?) or ranged reads with an unknown string table size? Is `fileSize` (+8) always the file length?
9. `wav` is 16-bit PCM; RADA decodes to f32. Accept quantisation or add a float option to `WavOptions`? Opus output: wanted?
10. GUID alias paths (`0x….asrc`) needed? If so, where does `string_to_guid` live (core or `respak-respawn`)?
11. Meaning of `bankMagic`, `bankHash`, `bankIdx`, `patchCount`, selector list types, play-state flags: unknown.
12. Titanfall 2 install location of banks and the TF2 tech test (v11): unsupported in rsx; drop.
