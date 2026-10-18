# subt · Subtitles (per-language subtitle string table)
**rsx sources:** `game/rtech/assets/subtitles.h` (`SubtitlesAssetEntry_t`, `SubtitlesAssetHeader_v0_t`, `SubtitlesEntry`, `SubtitlesAsset`), `game/rtech/assets/subtitles.cpp` (`LoadSubtitlesAsset`, `PostLoadSubtitlesAsset`, `PreviewSubtitlesAsset`, `ExportTXTSubtitlesAsset`, `ExportCSVSubtitlesAsset`, `ExportSubtitlesAsset`, `InitSubtitlesAsset`), `game/asset.h:61, 142, 216` (FourCC, colour, export folder `subtitles`), `core/filehandling/validation.cpp:140-161` (known versions, expectations), `core/main.cpp:89, 161` · **Games / versions:** Apex Legends; asset versions 0 and 1, same header (which seasons: unknown) · **Priority hint:** low–medium (localised subtitle text for audio events), **S**.

## Summary
A table of subtitle lines for one language. Each entry is a 32-bit hash (of the subtitle/sound id, unconfirmed) and a NUL-terminated string. A string may start with a colour tag `<clr:R,G,B>`. There is exactly one `subt` asset per language pak (`subtitles_<language>.rpak`). The user extracts it as a text/CSV/JSON table of `hash, colour, text`.

## Where it lives
- rpak asset, FourCC `subt` (`asset.h:61`); binding `.type = 'tbus'` (MSVC multi-char literal, `subtitles.cpp:263`), `headerAlignment = 8`.
- Data: only the header and the page data it points to (entries array and string block). No starpak: validation says no starpak, no opt starpak, no data page expected (`validation.cpp:152-159`). The pointed-to arrays live in the shared `.mem` pages.
- **Name not stored.** rsx derives it from the pak file name: pak stem `subtitles_<language>` (e.g. `subtitles_english`), `language = stem.substr(10)`, name `subtitles/subtitles_<language>.rpak` (`subtitles.cpp:30-32`); it asserts `StringToGuid(name) == asset guid` (`:34`), so the name is verifiable. Added to the name cache with `SetAssetName(name, true)`.
- Proposed respak volume name: `subtitles/subtitles_<language>.rpak.subt`? The guid-derived name only works when the pak file name is known; fall back to the guid when `string_to_guid` of the derived name does not equal the guid. See Open questions.

## Versions and detection
| asset version | header | rsx |
|---|---|---|
| 0 | `SubtitlesAssetHeader_v0_t`, 0x18 bytes | parsed |
| 1 | same struct, 0x18 bytes (`DECLARE_ASSET_VERSION_HEADER_SIZE(SUBT, 1, sizeof(..._v0_t))`) | parsed identically |
| other | — | load: `assertm` + return without extra data; post-load returns; preview prints "not currently supported" |

Differences between 0 and 1: none known in rsx (same case label, same struct). Whether version 1 adds anything unread (for instance the entries are laid out differently): unknown.

Proposed: `rpak.asset` kind `subt` → score 10 (container-provided kind, like `stlt`/`wrap`); version 0 or 1 → `Version { major: v, minor: 0 }`; others → `UnsupportedVersion`. Confirmation: the asset entry's `header_size` equals 0x18 (it does by `validation.cpp`; do not require it).

## Layout
Natural alignment (no pack pragma), little-endian. `int` is signed 32-bit. Pointers are page pointers (relocated by the rpak volume).

### `SubtitlesAssetHeader_v0_t` (`subtitles.h:11-19`, 0x18)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | i32 | stringCount | number of strings; only used as a capacity hint and an assertion bound (`parsed.size() <= stringCount`) |
| 0x04 | 4 | i32 | entryCount | number of `SubtitlesAssetEntry_t` in `entries` (includes empty slots, see below) |
| 0x08 | 8 | ptr | entries | page pointer to the entry array |
| 0x10 | 8 | ptr | strings | page pointer to the start of the string block |

### `SubtitlesAssetEntry_t` (`subtitles.h:5-9`, 0x8, array of `entryCount`)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 4 | u32 | hash | key of the line; **0 = empty slot** (skipped) |
| 0x04 | 4 | i32 | stringOffset | byte offset of the string from `strings` |

`entryCount` is probably the size of a hash table (open addressing) and `stringCount` the number of occupied slots; the file order of the entries may therefore be the table order, not the source order. Not confirmed (see Open questions).

### String format
NUL-terminated at `strings + stringOffset`; encoding not stated by rsx (it prints the bytes as is; UTF-8 assumed, other languages unverified). Optional prefix:
- `<clr:R,G,B>` where R, G, B are `%i` integers (0..255 expected); text starts after the first `>` (`:73-77`). Detected with `strncmp(str, "<clr:", 5)`.
- No prefix → colour defaults to `(255, 255, 255)`.
- Source comments mention further tags `<delay>` and `<len>` ("todo regex ?", `:71-72`). They are not parsed and stay in the text. Whether they exist in real data: unknown.

## Data outside the header
None beyond the pointed-to `entries` and `strings` blocks, both in page data. No streaming, no compression. The size of the string block is not stored; the end of a string is its NUL.

## Dependencies
None. Nothing is looked up by guid. The `hash` values are presumably keys that other assets (audio events, scripts) use to find the line; rsx never resolves them. Output does not need any other asset.

## Decoding
1. Check the asset kind `subt` and version 0/1; read the 0x18-byte header.
2. Resolve `entries` (needs `entryCount * 8` bytes of `.mem`) and `strings` page pointers. Null pointers with a non-zero count → `Corrupt`.
3. For `i in 0..entryCount`: read `{hash, stringOffset}`; if `hash == 0` skip.
4. `s = strings + stringOffset`; bounds-check against the mem length (rsx does not); read up to the NUL.
5. If `s` starts with `<clr:`: parse three integers; text = after the first `>`. If the tag is malformed (no `>`, fewer than three numbers) rsx is undefined (see quirks): respak keeps the whole string as text and colour white, or errors (decide at the gate).
6. Emit `(hash, [r, g, b], text)` in table order; optionally sort by hash for stable output.
7. Language: from the pak name (`subtitles_<language>`), else unknown; checked by `string_to_guid("subtitles/subtitles_<language>.rpak") == guid`.

## Output
rsx `ExportSubtitlesAsset` (`subtitles.cpp:215-255`): setting 0 `TXT`, setting 1 `CSV`. Folder `<export>/subtitles/` (or the full asset path's parent when `exportPathsFull`), file `<stem>.txt` or `<stem>.csv` with `stem = subtitles_<language>`.
- **TXT:** one subtitle text per line (colour tag stripped, hash dropped, no escaping; strings with embedded newlines break the line structure).
- **CSV:** header `"hash","color","subtitle"`; row `"<hash in lowercase hex without 0x>","R,G,B","<text>"`; text is not escaped (a `"` in the text breaks the row).
- UI preview: table of `0x<hash>` and text, text drawn in the entry colour.

Proposed respak: format `subt` from `rpak.asset`, a new `Table`-like/structured capability or `provides::<dyn Json>()` + `Metadata` (name, kind `subt`, guid; no dependencies). JSON shape:
```json
{ "language": "english", "guid": "0x...", "entries": [ { "hash": 3735928559, "color": [255,255,255], "text": "..." } ] }
```
(`hash` as number or `"0xdeadbeef"`: decide; rsx CSV prints lowercase hex.) A plain `.txt`/`.csv` target can be added later; the JSON keeps all fields, so it is the lossless one.

## rsx quirks
- Version check is `case 0: case 1:` only; no check on `entryCount`/`stringCount`/offsets; `strings + stringOffset` is not bounds-checked. Keep checks in respak.
- `sscanf_s` return value is ignored. With a malformed `<clr:` tag the stack defaults 255 stay (partial parse keeps the parsed components); `strchr(entryStr, '>') + 1` is `nullptr + 1` when there is no `>` (crash/garbage). Not to be ported.
- `parsed` pointers (`subtitle`) point into the pak's memory, so the CSV/TXT writers depend on the pak staying loaded.
- `assertm(parsed.size() <= stringCount)`: `stringCount` is only a sanity bound; it is not required to equal the number of non-zero hashes. Treat as a warning at most.
- Empty slots are found only by `hash == 0`; an entry with `hash == 0` and a valid string is silently lost (cannot be told apart).
- TXT export drops hashes and colours, so it cannot be turned back into the asset. CSV and TXT do no escaping.
- `language = stem.substr(10)` assumes the pak name starts with `subtitles_` (10 chars) and throws `std::out_of_range` on a shorter stem (only if the stem is shorter than 10). The `assertm` on the guid fires in debug builds when the pak is renamed.
- Export `bool` is the result of the single write; the extension is replaced by `replace_extension` on a stem that has no dots, so a language with a dot would lose its tail.

## Reuse in respak
- `crates/respak-respawn/src/assets/stlt/` is the closest model: `detect_kind(input.source, *b"subt")`, `rpak_asset(&source, *b"subt")`, `asset.read_header()`, `asset.mem_offset_of(slot)` and `asset.read_mem(at, len)` for the two pointers (the asset header is relocated by the volume, `Pointers { relocated: true }`), `Reader` for little-endian fields, the `cstr` helper (`stlt/read.rs:39`; currently `pub(super)`, 512-byte cap — strings may need a different limit or a shared helper), `mem_len` bounds checks, `Error::corrupt`, `ErrorKind::UnsupportedVersion`, `FormatId::new("subt").with_version(Version::new(v))`, the `Data` + `Metadata` impl shape and the `json.rs` encoder pattern (`StltJsonEncode`).
- `crates/respak-respawn/src/guid.rs` `string_to_guid` for the name check.
- `dtbl` (`assets/dtbl/`) is a reference for a table-style output (`text.rs` for a CSV-like target), but there is no shared code needed.
- New: `SubtAsset` (language, entries), reader, colour tag parser, JSON (and optional CSV/TXT) encoder, handler registration in `assets/mod.rs`/`lib.rs`.

## Tests
- Synthetic: v0 header with `entryCount = 4`, entries `{0xAAAA0001, 0}`, `{0, 0}` (empty slot, skipped), `{0xAAAA0002, 14}`, `{0xAAAA0003, 30}`; string block `"<clr:255,0,10>Hi\0Plain\0..."`. Cases: colour tag parsed and stripped; no tag → white; hash 0 skipped; version 1 same bytes → same result; version 2 → `UnsupportedVersion`; `stringOffset` beyond the mem length → `Corrupt`; malformed tags (`<clr:1,2>`, `<clr:1,2,3` without `>`); null `entries` with `entryCount > 0` → `Corrupt`; empty table (`entryCount = 0`); non-ASCII UTF-8 text.
- Name check: guid of `subtitles/subtitles_english.rpak` accepted, a different guid falls back to guid-only naming.
- Real (no Steam runs without permission): `subtitles_english.rpak` and one non-Latin language (russian, japanese, chinese); compare entries with rsx's CSV export; check the occupied count against `stringCount`.

## Open questions
1. What is `hash`: hash of the audio event / sound name, or a script id? Which function (CRC32, `RTech::StringToGuid` truncated, other)? It decides whether `Dependency`-style links to `aevt`/`asrc` are possible.
2. What is different in asset version 1 versus 0 (same struct in rsx)?
3. Are `entries` an open-addressing hash table (empty slots with `hash == 0`, `entryCount > stringCount`)? Keep the table order or sort by hash?
4. Are `<delay>` and `<len>` tags present in real data and what do they mean? Keep the text raw, or expose them as fields?
5. Encoding of the strings in non-Latin language paks (UTF-8 vs other)?
6. Volume entry naming when the pak name is unknown or not `subtitles_<language>`; keep `subtitles/subtitles_<language>.rpak`?
7. Output: JSON only, or also rsx-compatible CSV/TXT targets?
