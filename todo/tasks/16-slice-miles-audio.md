# 16 · Slice: Miles banks → WAV
**Status:** in-progress (built and tested on synthetic data; real-file check and demo pending) · **Depends on:** 15 (the `Audio` capability and the `wav` edge) · **See:** PROJECT.md §5.4, §6, §12, [`../formats/miles.md`](../formats/miles.md), [`../formats/miles-codecs.md`](../formats/miles-codecs.md)

## Goal
Apex and Titanfall 2 sound banks open as volumes, and a source converts to WAV, on Windows and Linux, without RAD's libs or DLLs:
```
respak discover audio/ship/general.mbnk --depth 1
respak convert audio/ship/general.mbnk/<source name>.asrc --to '{"type":"wav"}'
```
(`--to` is required by the CLI.) Loose files, not an rpak: the `.mstr` streams are found next to the `.mbnk` by file-name convention.

## Decided already (user, 2026-10-14; recorded in the format docs)
- Localised sources are named `<name>.<lang>.asrc`; non-localised `<name>.asrc`.
- Every source is listed; a missing `.mstr` surfaces only when the source is read (`MissingDependency`). No directory scan, no `stat` while listing.
- RADA (`RADA`, Apex S24+): **header only**. `tracks()` reports the shape, `samples()` gives `UnsupportedVersion`.
- Multichannel audio exports as **one mono WAV per channel** (composite output, like `png.frames`). No channel mask.
- No RAD binaries, C code or DLLs in this slice. A third-party codec plugin for RADA is a later, separate decision (backlog).

## Gate answers (user, 2026-10-14)
1. BCF1: extend the `chiririll/nihav` fork's `binkaud.rs` (Bink Audio 2, more than 2 channels, raw-block entry point).
2. Events (`aevt`): out of this slice.
3. `mprj`: skipped; languages come from the per-version tables. Guard: an entry array that doesn't fit the file is `Corrupt`.
4. GUID aliases: skipped.
5. Real files: the latest Apex depot (`audio/ship/general.mbnk`, `general_stream.mstr`, one download, user-approved). Latest banks are RADA, so they cover bank/stream parsing and the RADA header only; BCF1 stays on synthetic fixtures until a pre-S24 or Titanfall 2 bank is available.

## Proposed scope (agreed)
- New crate `respak-miles`, plugin id `miles` (no dependency on other plugins).
- Formats: `mbnk` (container/volume), `mstr` (header check), `asrc` (→ `Audio` + `Metadata`).
- Codec BCF1 (Bink Audio) in Rust, then RADA header parse.
- Bank versions: 13, 28–46, 48, 49 as in miles.md's table; others → `UnsupportedVersion`.

## Gate questions
1. **BCF1 decoder home:** extend the `chiririll/nihav` fork's `binkaud.rs` (adds Bink Audio 2, more than 2 channels, a raw-block entry point; one Bink Audio implementation shared with `bik`) or write a standalone decoder in `respak-miles`? Recommended: the fork. Needs a decision on where the `bcf1` frame parser lives (`respak-nihav` next to `bik_decode.rs`, or `respak-miles` calling into it).
2. **Events (`aevt` / `aevt.json`):** in this slice or later? Recommended: later. Sources are the part users need ("every game sound"), events are JSON only and understood for banks 13 and 49 only.
3. **`mprj`:** skip for now (languages come from the per-version tables), or read it for the language list? Recommended: skip; open question 2 in miles.md stays open.
4. **GUID alias paths (`0x….asrc`):** wanted? Recommended: no.
5. **Real-file tests:** which banks do you have locally (Apex `general.mbnk` version, a pre-S24 dump with BCF1, Titanfall 2)? BCF1 needs a bank whose sources are `1FCB`.

## Sketch (built, compiles, bodies are `todo!()`)
New crate `crates/respak-miles` (added to `[workspace.dependencies]`; **not yet wired into the CLI**, that happens at build because registering the plugin changes `formats_prints_the_graph`). Only dead-code lints fire on the sketch (every body is `todo!()`); `respak-core` itself fails `clippy -D warnings` today on `len_without_is_empty` in `capability/blob.rs` (not touched).
- `lib.rs`: `MilesPlugin` registers `mbnk` (read from `file`, provides `Container`), `mstr` (read from `file`), `asrc` (no read edge: handed out by the volume; provides `Audio` + `Metadata`).
- `mbnk.rs`: `BankLayout::for_version` (the version table), `SourceEntry`, `Bank::parse` (whole file, `Corrupt` when the entry array doesn't fit), `MbnkSource: VolumeSource`, `MbnkData: Container` (its `entry()` hands out an `AsrcData`), `MbnkRead`.
- `mstr.rs`: `MstrHeader::parse`, `MstrRead` (header check only).
- `asrc.rs`: `AsrcData: Audio + Metadata + Data`; opens the `.mstr` next to the bank on the bank's own operator, checks its header, reads the codec header, picks `Codec::{Bcf1, Rada}`.
- `bcf1.rs`: `Bcf1Header`, `Bcf1Frame::parse` (sync `0x9999`, the `0xFFFF` form with the end trim), `seek_offsets` (u16 deltas → absolute). The DCT decoder comes from the fork's `binkaud.rs` at build time (the nihav dependency is added then, as in 15).
- `rada.rs`: `RadaHeader::parse` (header only).

## Tests (proposed, for you to agree or change)
Unit, synthetic fixtures built in test code, no game files:
- **mstr:** the 20-byte header parses; version 1 → `UnsupportedVersion`; wrong magic is not scored; `languageIdx = 0xFFFF` and `0`.
- **mbnk layouts:** v49 (11 entries = 2 + 1·9, languages `english…korean`, v49's 32-bit offsets), v48 (names in the block after the source array), v45 (v39 sources), v28, v13 each give the right names, language suffixes (`<name>.<lang>.asrc`), sample rate, patch and header range. An unknown version (47) → `UnsupportedVersion`; wrong magic is not scored.
- **mbnk guards:** an entry array past the end of the file, a string offset out of range, a truncated header → `Corrupt`, never a panic.
- **Volume:** `list` shows every source with no `.mstr` present; `stat` / `read` of an `.asrc` path; unknown path → `NotFound`; localised and non-localised names don't collide.
- **asrc open:** the `.mstr` name per source (`general_stream.mstr`, `general_english.mstr`, `general_korean_patch_1.mstr`); a missing `.mstr` → `MissingDependency { file }` on read only; a `.mstr` with another `buildTag`, language or patch → `Corrupt`; only the codec header and data ranges are read (byte-counting operator).
- **BCF1:** header parse (48000 → 1920 samples per block, 22050 → 960, 16000 → 480; version 3 → `UnsupportedVersion`; the v2 high word of the entry count ignored; `maxComp` high word → Bink Audio 2); seek deltas `[10, 300, 65535]` → `[0, 10, 310, 65845]`; frame parser (plain form, `0xFFFF` form with `valid_samples`, size over `maxComp`, missing sync → `Corrupt`, truncated last frame yields earlier frames then one error); silent block for 1, 2, 3 and 6 channels; trim of the last block gives exactly `sampleCount` frames.
- **RADA:** header parse (`sample_rate` enum 3 → 48000, 4 → `Corrupt`, more than 32 channels → `Corrupt`); `tracks()` reports the shape; `samples()` → `UnsupportedVersion`.
- **End to end (synthetic):** a fake bank + fake `.mstr` with a BCF1 silent source → `convert … --to wav` gives the expected header and zero samples; cancelling mid-stream gives `Cancelled`.
- Random bytes after a valid header never panic.

Real files (env vars, skipped with a message when unset, never committed):
- Latest Apex `audio/ship/general.mbnk` + `general_stream.mstr`: the bank parses, every source is listed, names and languages look sane, header ranges lie inside the `.mstr`, RADA headers give a plausible rate / channels / duration (frames ÷ rate vs the bank's `sampleCount`), a RADA source says `UnsupportedVersion`.
- A pre-S24 or Titanfall 2 bank (if you can get one): BCF1 decodes; compare with rsx WAVs within ±2 LSB and, for old-bitstream files, with `ffmpeg -f binka`.

Always: clippy `-D warnings` for the new crate, `cargo test --workspace`, `cargo tree -p respak-miles` shows no plugin-to-plugin dependency.

## Gate answers, round 2 (user, 2026-10-14)
1. Multichannel: **one mono track per channel** (`tracks()` reports N mono tracks, `wav` with `track: n` writes channel n). A target that writes all channels in one go is not in this slice unless asked.
2. The test list above is agreed.

## Built (2026-10-14)
- `crates/respak-miles`: all of the sketch is implemented and wired into the CLI (`MilesPlugin` after `NihavPlugin`). 30 tests (unit + `e2e_tests.rs` through the real engine and the `wav` edge); clippy `-D warnings` is clean for the crate.
- Bink Audio: `BinkAudioBlocks` in the `chiririll/nihav` fork, branch `miles-bink-audio` (commits `b79dbcb`, `c91ea62`, pushed with the user's OK); the workspace pins `c91ea62` for all three nihav dependencies. The old decoder is untouched (two helpers made `pub(super)`).
- Opening an `asrc` reads the `.mstr` header and the codec header block, so a missing stream is `MissingDependency` at open time, not at listing time.
- `tracks()` is one mono track per channel; `wav` with `track: n` writes channel n. No all-channels target yet.
- Left out on purpose: `seek_offsets` (decoding is linear from the start), `mprj`, events, GUID aliases.
- `spawn_stream` is copied from `respak-nihav/src/bik.rs` (about 25 lines). A shared home in core is a follow-up, not done here.

## Not verified yet
- No real bank has been read. The field tables of the five bank layouts come from rsx and are only self-consistent in the synthetic fixtures. The first real check is `discover audio/ship/general.mbnk --depth 1` on the latest Apex depot (bank v49): counts, names, languages and header ranges must look sane.
- The BCF1 decoder has only synthetic tests (silence, a DC coefficient, the Bink Audio 2 sign order). It has never decoded real audio. Open: per-channel first-block overlap (FFmpeg clears the flag after the first sub-stream), linear overlap weights.
- RADA: header only, as decided.

## Gate questions (answered, kept for the record)
1. **Multichannel export.** The decision is one mono WAV per channel. The cheapest way that needs no new edge: `tracks()` reports a stereo source as one 2-channel track and the existing `wav` writes it as one interleaved file (what rsx does), and per-channel files come later; or `tracks()` reports one **mono track per channel**, so `--to '{"type":"wav","track":1}'` gives one channel and a new composite target would write them all. Which one do you want in this slice?
2. Agree or change the test list above.

## Leaves out
Events and graphs, `mprj`, RADA decoding, Opus, float WAV, `WAVE_FORMAT_EXTENSIBLE`, markers for banks other than v49, writing Miles files, GUID aliases (unless the gate says otherwise).

## Demo
The user lists a bank, converts a few sources to WAV and listens (a BCF1 mono, a stereo, a localised voice line), comparing against rsx exports.
