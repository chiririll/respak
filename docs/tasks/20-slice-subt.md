# 20 · Slice: subtitles (`subt`) → table (CSV / JSON) and TXT
**Status:** in-progress (built; real-file check done; demo pending) · **Depends on:** 07, 08 (rpak volume), `stlt` as the model · **See:** project.md §5.5, §6, §12, [`../formats/subt.md`](../formats/subt.md)

## Goal
A subtitle table opens from a language pak and converts to JSON, CSV or TXT:
```
respak convert paks/Win64/subtitles_russian.rpak/<asset>.subt --to '{"type":"json"}'
respak convert paks/Win64/subtitles_russian.rpak/<asset>.subt --to '{"type":"csv"}'
```
`discover` already lists the asset as kind `subt` (rpak.asset, version **1**, guid `0x0770D3ABD7E3E286`, checked 2026-10-18 on `subtitles_russian.rpak`, user-approved run). It has no targets yet.

## Decided already (user, 2026-10-18)
- **Output:** JSON (lossless) **and** CSV and TXT, as rsx does.
- **Real file:** `paks/Win64/subtitles_russian.rpak` may be checked with `discover --depth 1` using `respak.toml`. Any other Steam run still needs permission.

## Proposed defaults (agree or change at the gate)
- **Versions:** 0 and 1, one header (0x18 bytes) and one code path as in rsx; anything else → `UnsupportedVersion`. `Version { major: v, minor: 0 }`, score 10 from the `rpak.asset` kind.
- **Entries:** skip `hash == 0`; **keep table order** (no sorting); an option `sort` = `"table"` (default) | `"hash"` can come later.
- **Colour tag:** `<clr:R,G,B>` parsed and stripped. A malformed tag (no `>`, fewer than three numbers, value outside 0..255) keeps the whole string as text with colour white and adds a `Warning`, instead of an error. `<delay>` / `<len>` stay in the text.
- **Bounds:** header counts and every `stringOffset` are checked against the mem length → `Corrupt`; null pointer with a non-zero count → `Corrupt`; `stringCount` is a hint only. Strings must be valid UTF-8, otherwise lossy decode plus a `Warning`.
- **Name:** volume name is the guid (`0x<GUID>.subt`, as `discover` shows today). The language comes from the pak stem `subtitles_<language>` and is put in the metadata only when `string_to_guid("subtitles/subtitles_<language>.rpak")` equals the guid.
- **JSON shape (superseded by the sketch: generic `table.json`):** `{ "language": "russian"|null, "guid": "0x…", "entries": [ { "hash": "0x…", "color": [r,g,b], "text": "…" } ] }`. `hash` as a hex string (as rsx CSV, and it stays exact in any JSON reader).
- **CSV (superseded by the sketch: generic `table.csv`):** rsx layout, header `"hash","color","subtitle"`, row `<hash lowercase hex>,"R,G,B",<text>`, but **with RFC 4180 escaping** (quote doubled, newlines kept inside the quoted field). That differs from rsx, whose rows break on `"`.
- **TXT:** text only, one line per entry as rsx. Embedded newlines are written as the two characters `\n` so the line count stays equal to the entry count.
- **Metadata:** `Metadata` with kind `subt`, guid, no dependencies.

## Proposed scope
- Plugins: `respak-respawn` (module `assets/subt/`) and `respak-formats` (`table.rs`). No change in core or `dtbl`, no plugin-to-plugin dependency.
- Interfaces: see the Sketch section below.
- Reuse: `detect_kind`, `rpak_asset`, `read_header`, `mem_offset_of`, `read_mem`, the `stlt` `Reader`, `cstr` (the 512-byte cap is too small for long lines, so the cap becomes a parameter or the helper moves up to a shared place), `string_to_guid`.

## rsx review
Source: `subtitles.{h,cpp}` (details in subt.md). Small and self-contained.

**Problems** (kept out of the port): no bounds checks on `entryCount` / `stringOffset`; `strchr('>') + 1` on null and ignored `sscanf` result for malformed colour tags; CSV/TXT without escaping; strings that borrow the pak's memory; `substr(10)` on a short stem; an entry with `hash == 0` lost silently (kept: indistinguishable from an empty slot, documented).

**Behaviour changes against rsx** (for the demo comparison): CSV is escaped; TXT escapes embedded newlines; malformed tags are text + warning instead of undefined behaviour; JSON added.

## Sketch (rewritten 2026-10-18 after "use Table and universal approaches"; compiles, bodies are `todo!()`)
- `respak-formats/src/table.rs` (new, universal): `TableCsvEncode` (`table.csv`, RFC 4180) and `TableJsonEncode` (`table.json`), both registered with `from_cap::<dyn Table>` like `png` from any `Bitmap`. Exported with `table_csv`.
- `respak-respawn/src/assets/subt/` (`mod.rs`, `read.rs`, `text.rs`, empty `tests.rs`): `SubtHeader::parse`, `SubtEntry`, `parse_color_tag`, `SubtAsset: Data + Metadata + Table` (columns `hash` string `0x…`, `color` vector, `subtitle` string), `SubtRead`, `SubtTxtEncode` (`subt.txt`, the only subtitle-specific text target).
- Not yet registered in the plugins or re-exported from `lib.rs` (at build, as in 16/18/19); only dead-code warnings fire.
- **`dtbl.csv` stays as it is.** Its doc promises rsx's CSV byte for byte (it is repak's input format), so escaping there would break that. `table.csv` is the fixed, escaped CSV for everything; `dtbl.json` could later become `table.json` (backlog, not touched here).
- Changes from the first proposal: the JSON is the generic table JSON (no `language` / `guid`, they stay in `Metadata`), and the CSV header is `hash,color,subtitle` with the colour as `r;g;b` instead of rsx's `R,G,B` (a comma would force quoting). Say if you want the rsx cell form.

## Tests (proposed, to agree at the gate)
- Synthetic v0 and v1 pak (`PakBuilder::subt`): 4 slots, one empty, colour tag parsed and stripped, plain string white, table order kept; v1 same bytes → same result; version 2 → `UnsupportedVersion`.
- Failures: `stringOffset` past the mem, `entryCount` past the mem, null `entries` with count > 0 → `Corrupt`; empty table (`entryCount = 0`) → empty list.
- Colour tag: `<clr:1,2>`, `<clr:1,2,3` without `>`, `<clr:300,0,0>`, `<clr:a,b,c>` → text kept + warning; tag in the middle of the text is not a tag.
- Text: non-ASCII UTF-8, invalid UTF-8 (lossy + warning), embedded newline and quote through CSV / TXT / JSON.
- Name: guid of `subtitles/subtitles_english.rpak` gives language `english`; other guid or other pak stem → `null`.
- Random bytes never panic (header and string block).
- End to end (`integration/tests/subt.rs`): `convert … --to table.json|table.csv|subt.txt` on a synthetic pak, output equals the expected bytes.
- Real (user-approved for this file only): `subtitles_russian.rpak` → JSON; check Cyrillic text, entry count against `stringCount`, name check of the guid `0x0770D3ABD7E3E286`. Needs a `convert` run through Steam: ask first.

## Built (2026-10-18)
- `respak-formats::table`: `to_csv` / `to_json`, `TableCsvEncode` (`table.csv`) and `TableJsonEncode` (`table.json`) registered with `from_cap::<dyn Table>`. 3 unit tests (quoting, one row per JSON line, `null` for non-finite floats, empty table).
- `respak-respawn::assets::subt`: `SubtHeader::parse`, `parse_color_tag`, `SubtAsset` (`Data` + `Metadata` + `Table`), `SubtRead`, `SubtTxtEncode`; registered as `subt` / `subt.txt`. 3 unit tests plus 10 tests in `crates/respak-respawn/tests/subt.rs` (order and empty slots, v0 = v1 and v2 unsupported, empty table, five corrupt cases, unterminated string, malformed tags, UTF-8, language check, TXT, CSV/JSON through the table).
- `PakBuilder::subt` / `subt_raw` in `respak-respawn-testing`.
- Totals: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass. The tests were written together with the code, not seen failing first (the sketch's `todo!()` bodies were replaced in one pass).
- Decisions while building: a line whose offset is past the end of `.mem` is `Corrupt`; the string window read is `farthest offset + 64 KiB`, so one ranged read serves all lines; the language is taken from the asset key's pak name.
- Follow-up done (user, 2026-10-18): `dtbl.json` now uses the generic table JSON (`respak_formats::table_json`; the old code moved into `respak-formats/src/table.rs` unchanged, so the output is byte for byte the same and the dtbl tests pass). The first generic JSON went through `serde_json::Value` and turned `0.1f32` into `0.10000000149011612`; caught while moving, fixed by taking dtbl's serializer. `dtbl.csv` stays rsx's unescaped CSV on purpose. The formats graph test lists `table.csv` / `table.json`.
- `docs/cli.md` has no target list for these formats (only the options-bearing ones), so nothing was added.

## Real-file check (2026-10-18, user-approved, `subtitles_russian.rpak`, asset version 1, all from the chunk cache)
`convert … --to '{"type":"table.json"|"table.csv"|"subt.txt"}'` wrote `out/subt/0x0770D3ABD7E3E286.{json,csv,txt}` with no errors or warnings.
- **157,037 lines**, all with Cyrillic text intact (UTF-8). TXT has one row per line; no line holds a real newline.
- **Colour:** every line is white. This pak has no `<clr:…>` tags, so the tag parser is only tested on synthetic data.
- **Other tags kept in the text:** `<delay:0.1><len:11.8>` (540 lines with `<delay:`, 564 with `<len:`), `<cr>` (4), `<low>` (12).
- **Hashes are not unique:** 157,032 distinct values, 5 of them used by two lines with different text (different speakers). That is about what 32-bit collisions among 157k keys give, so `hash` is a key that can collide, not an id. Table order, no sorting.
- **Name check:** `string_to_guid("subtitles/subtitles_russian.rpak")` is `0x0770D3ABD7E3E286`, the guid `discover` shows (now a unit test).
- **Not checked:** `stringCount` against the number of lines (the header is not exposed in the output), the comparison with rsx's CSV, other languages (non-Latin CJK).

## Leaves out
Sorting option, `<delay>` / `<len>` parsing, linking `hash` to `aevt` / `asrc`, writing a `subt` back, a UI preview, the v0 vs v1 difference (none known).

## Demo
The user converts `subtitles_russian.rpak/….subt` to JSON and CSV and compares the line count and a few lines with rsx's export.
