# locl · Localization (per-language string table)
> **Verified against** Apex `paks/Win64/localization_italian.rpak` (depot 1172471; 39,465 strings): header layout (0x38), version, entry layout, the table structure, the hash function, UTF-16 strings. `localization_english` could not be read.
**rsx sources:** `game/rtech/assets/localization.h` (`LocalizationEntry_t`, `LocalizationHeader_t`, `LocalizationAsset`), `game/rtech/assets/localization.cpp` (`LoadLocalizationAsset`, `PostLoadLocalizationAsset`, `ExportLocalizationAsset`, `InitLoclAssetType`), `game/asset.h:62, 143, 217` (FourCC, colour, export folder `localization`), `core/utils/utils_general.h:103` (`EscapeString`), `core/main.cpp` (registration), `docs/assets/rpak/Localization.md` · **Games / versions:** Apex Legends (rsx doc: "does not exist in Titanfall 2"); one header, no version check (asset version values: unknown) · **Priority hint:** medium (all UI text for a language), **S**.

## Summary
A hash table of localisation keys → UTF-16 strings for one language. Each entry is a 64-bit hash of the Source-engine localisation token (for example `spray_heart_01` → `fff4877635538a03`, "My Heart!") and an offset into a UTF-16 string block. One `locl` asset per language, in its own pak (`localization_<language>.rpak`, e.g. `localization_english`). The user extracts a key/value text file (Valve KeyValues style) or JSON.

## Where it lives
- rpak asset, FourCC `locl` (`asset.h:62`); binding type `'lcol'` (MSVC multi-char literal, the four bytes reversed, `localization.cpp:96`), `headerAlignment = 8`.
- Data: only the header and the page data it points to (file name string, entries array, UTF-16 string block). No starpak, no streaming.
- **Name is stored in the header:** `fileName` (page pointer to a C string, e.g. `localization_english`). rsx names the asset `localization/<fileName>.locl` (`localization.cpp:15`); it does not compare it with the asset guid. Export file `<fileName>.locl` in `<export>/localization/` (or the parent of the full asset path with `exportPathsFull`).
- Proposed respak volume name: `localization/<fileName>.locl`; fall back to the guid when `fileName` is null/empty/invalid. Verified: the asset guid is `string_to_guid("localization/<fileName>.rpak")` (the `.locl` form does not match), so the name can be checked against the guid.

## Versions and detection
rsx has no `version()` switch: every `locl` asset is read as `LocalizationHeader_t` (0x38). Real data: asset version 1, header size 0x38 (`static_assert`, `localization.h:24`).

Proposed: `rpak.asset` kind `locl` → score 10 (container-provided kind); `Version { major: asset_version, minor: 0 }` (unknown values; accept whatever the entry says as long as `header_size >= 0x38`, else `Corrupt`). Unknown variant is not detectable by rsx; reject `header_size < 0x38` with `UnsupportedVersion`.

## Layout
Natural alignment, little-endian, 64-bit pointers. `int` is signed 32-bit, `size_t` is 8 bytes. Pointers are page pointers (relocated by the rpak volume).

### `LocalizationHeader_t` (`localization.h:13-22`, 0x38)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | ptr | fileName | page pointer to NUL-terminated ASCII name (`localization_<language>`) |
| 0x08 | 4 | i32 | numStrings | number of strings = number of entries with non-zero hash (verified); not used by rsx |
| 0x0C | 4 | i32 | f | number of primary hash buckets (65,536 = 0x10000 in the sample); entries `[0, f)` are buckets indexed by `hash & 0xFFFF`, entries `[f, numEntries)` are the overflow area |
| 0x10 | 8 | u64 | numEntries | number of `LocalizationEntry_t` in `entries` (includes empty slots) |
| 0x18 | 8 | ptr | entries | page pointer to the entry array |
| 0x20 | 8 | ptr | strings | page pointer to the UTF-16 string block (`wchar_t*`) |
| 0x28 | 16 | u8[16] | unk28 | unknown, unused |

### `LocalizationEntry_t` (`localization.h:4-11`, `#pragma pack(push,1)`, 0x10, array of `numEntries`)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 8 | u64 | hash | hash of the localisation token; **0 = empty slot** (skipped) |
| 0x08 | 4 | i32 | stringStartIndex | index in `wchar_t` units (not bytes) of the string from `strings` |
| 0x0C | 4 | u32 | unkC | **index of the next entry in this bucket's collision chain** (verified), `0xFFFFFFFF` = end of chain / empty slot |

`numEntries` is the size of a chained hash table (primary buckets + overflow area, see *Verified on real data*): entries in file order are the table order, not the source order.

### String format
`wchar_t` on Windows = **UTF-16LE** code units, 2 bytes each. A string starts at `strings + stringStartIndex * 2` and ends at the first 0x0000 code unit. No markup is parsed by rsx; strings may contain Source-style placeholders and escapes (unverified).

## Verified on real data
`localization_italian.rpak`, asset `0xCD60B7D231C1D5C9`:
- **Asset version 1**, entry header size **0x38** (56). `fileName` = `localization_italian`, `numStrings` = 39,465 = the number of non-empty entries (so `numStrings` *is* the string count), `f` = **65,536** (0x10000), `numEntries` = 75,338, `unk28` all zero.
- **The hash is `RTech::StringToGuid`** (respak's `string_to_guid`) of the token, case-insensitive like asset paths. `string_to_guid("spray_heart_01")` = `0xFFF4877635538A03` and that entry reads "Il mio cuore!" (rsx's example).
- **Table layout (confirmed): a chained hash table with a primary area and an overflow area.** The first `f` = 65,536 slots are buckets: a non-empty entry in slot `i < f` always has `hash & 0xFFFF == i` (29,663 of 29,663). The remaining `numEntries - f` = 9,810 slots are an overflow area holding collisions (9,802 used). **`unkC` is the index of the next entry of the chain**, `0xFFFFFFFF` ends it; every chain link in the file (9,802) points to an entry with the same low 16 bits, and all 9,802 overflow entries are reachable from a primary slot. Empty slots are `hash 0, stringStartIndex 0, unkC 0xFFFFFFFF`. To enumerate, scan all `numEntries` slots and skip `hash == 0` (as rsx does); the chains are only needed for lookup.
- Entry order is therefore table order: not sorted by hash and not by string index. No duplicate hash.
- `strings` pointer = `entries` pointer + `numEntries * 16` (the string block follows the entry array directly). `stringStartIndex` is in UTF-16 code units, strings are NUL-terminated; all 39,465 decode as valid UTF-16LE with no unpaired surrogate.
- **The asset guid is `string_to_guid("localization/<fileName>.rpak")`**, not `.locl` (`localization/localization_italian.rpak` matches, `.locl` does not). The `.rpak` suffix is the same convention as `texture/<name>.rpak`. Use that for name verification.
- String content: real newlines (4,541 strings), positional placeholders `%s1`..`%s4`, `%1`, a literal `%%`, input-action tokens in percent signs (`%use%`, `%attack%`, `%jump%`, `%ping%`, `%chat_wheel%`), `%$rui/...%` icon references and `%scriptCommand...`. Nothing else needs special handling; keep strings verbatim.

## Data outside the header
None beyond the pointed-to `fileName`, `entries` and `strings` blocks, all in page data. No compression. The size of the string block is not stored; the end of a string is its NUL code unit.

## Dependencies
None. Keys are hashes referenced by scripts/UI (not resolved by rsx). Output needs no other asset. For linking with other assets (project.md §5.5), none.

## Decoding
1. Check asset kind `locl`; read the 0x38-byte header.
2. Resolve `fileName` (C string), `entries` (`numEntries * 16` bytes) and `strings` through the volume. Null `entries` with `numEntries > 0` → `Corrupt`. `fileName` null → fall back to guid naming.
3. For `i in 0..numEntries`: read `{hash, stringStartIndex, unkC}` (16-byte stride, unaligned reads); skip `hash == 0`.
4. `p = strings + stringStartIndex * 2`; bounds-check `stringStartIndex >= 0` and `p` within the mem; read u16 units up to the 0x0000 terminator (rsx does not check; it can read past the block). Decode UTF-16LE → UTF-8; unpaired surrogates: replace with U+FFFD (rsx's `WideCharToMultiByte` drops/replaces them: unspecified).
5. Insert into the map keyed by `hash`; on a duplicate hash rsx keeps the first (`unordered_map::insert`). Respak: keep the first and warn, or error (decide).
6. Emit the entries; sort by hash for stable output (rsx order is the unspecified `unordered_map` order).

## Output
rsx `ExportLocalizationAsset` (`localization.cpp:49-89`): a single format, text in Valve KeyValues style, `ofs` in binary mode (no CRLF translation):
```
"<fileName>"
{
\t"<hash lowercase hex, no leading zeros, no 0x>" "<escaped utf-8 string>"
...}
```
- Header line `"<fileName>"\n{\n`, one `\t"<hash>" "<text>"\n` per entry, closing `}` without a trailing newline.
- Escaping (`EscapeString`): `\t`, `\n`, `\r`, `"`, `\\` as backslash sequences; NUL dropped; non-printable ASCII as `\x<2 hex digits>` (note: `std::hex` is applied but `setw(2)` is placed before `\x`, so the byte may print without padding in some cases); bytes above 0x7F copied through unchanged as multibyte sequences.
- Hex formatting uses `std::hex`, which is sticky for `ofs`; keys have no padding, so `0x0a` prints `a`.

Proposed respak: kind `locl`, `Metadata` (name, guid, no dependencies); JSON target (lossless, includes empty-string entries and `unkC` only if it turns out meaningful):
```json
{ "name": "localization_english", "guid": "0x...", "entries": [ { "hash": "5df85afa428ecca8", "text": "Most Knockdowns" } ] }
```
(`hash` as 16-digit lowercase hex string, since JSON numbers lose 64-bit precision.) An optional rsx-compatible KeyValues text target can reuse the escaping above.

## rsx quirks
- No version check or bounds check on `stringStartIndex`, `numEntries` or the string end. Port with checks.
- `wideString.length()` stops at the first NUL, so embedded NULs cannot occur; fine.
- `WideCharToMultiByte(CP_UTF8...)` Windows-only; the output buffer is `length*4` bytes, enough for UTF-8; return value is not checked (0 on error → empty string). Not to be ported; use a UTF-16 decoder with an explicit error policy.
- `unordered_map` insert keeps the first value for a duplicate hash and gives an unspecified output order; exports are not diff-stable between runs.
- Entry with `hash == 0` and a valid string is silently lost (indistinguishable from empty).
- `std::hex` is left on for `ofs`; irrelevant because nothing else is printed afterwards.
- `\x` branch of `EscapeString` writes `\x` after the `setw(2)`, so padding can land in front of the backslash (cosmetic).
- `numStrings`, `f`, `unkC`, `unk28` are never used.
- The root key in the export is `fileName`; the file name on disk is also `fileName` (+ `.locl`), so an odd `fileName` (with path separators or dots) would change the output path through `replace_extension`.
- `LocalizationAsset` is constructed by the header's raw pointers, so the preview/export depend on the pak staying loaded.

## Reuse in respak
- `crates/respak-respawn/src/assets/subt/` is the closest model (hash-table of `{hash, offset}`, empty slot = 0, string block via a page pointer, `Metadata` + JSON encoder, bounds checks, `Corrupt`/`UnsupportedVersion`). Same helpers: `detect_kind`, `rpak_asset(&source, *b"locl")`, `asset.read_header()`, `asset.mem_offset_of(slot)`, `asset.read_mem(at, len)`, `Reader`, `cstr` for `fileName`.
- `crates/respak-respawn/src/guid.rs` `string_to_guid` for the optional name check.
- New: `LoclAsset` (name, entries: `(u64 hash, String text)`), UTF-16LE decoder (std `char::decode_utf16` suffices), reader, JSON (and optional KeyValues) encoder, handler registration.

## Tests
- Synthetic: header with `numEntries = 4`, entries `{0x5df85afa428ecca8, 0, 0}`, `{0, 0, 0}` (skipped), `{0xfff4877635538a03, 6, 0}`, `{0x1, 12, 0}`; string block UTF-16LE `"Hi\0Bye\0..."` (indexes in wchar units). Cases: hash 0 skipped; index multiplied by 2; non-ASCII (Cyrillic, CJK) and a surrogate pair (emoji) decode to correct UTF-8; unpaired surrogate handling; duplicate hash (first wins / warn); `stringStartIndex` negative or beyond the mem → `Corrupt`; missing terminator at the end of the block → `Corrupt`; null `entries` with `numEntries > 0` → `Corrupt`; empty table (`numEntries = 0`); `header_size < 0x38` → `UnsupportedVersion`; odd `strings` alignment.
- Escaping: `"`, `\`, tab, newline, control byte `0x01`.
- Real: `paks/Win64/localization_italian.rpak` (hash claim `string_to_guid("spray_heart_01") == fff4877635538a03`, `numStrings` == non-zero hashes and the chain structure all hold); a non-Latin language is still to be checked.

## Open questions
1. What `unk28` is (all zero in the sample; the field is 16 bytes).
2. Output: JSON only, or also the rsx-compatible KeyValues text?
3. Behaviour on unpaired surrogates and duplicate hashes (replace/warn/error): none of either occurs in `localization_italian` (0 lone surrogates, 0 duplicate hashes), so only a policy decision is left.
4. Only `localization_italian.rpak` could be read (the Steam login expired before the other languages' chunks were cached); `localization_english` and the non-Latin languages were not checked, so UTF-16 handling of CJK/Arabic and the entry counts per language are unverified.
