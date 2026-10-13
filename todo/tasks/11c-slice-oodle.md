# 11c · Slice: Oodle decompression
**Status:** in-progress · **Depends on:** 11b (real Oodle data comes from Steam) · **See:** PROJECT.md §3, §6, §12

## Goal
Oodle-compressed paks and streamed mips work, so the current Apex version converts:
```
respak --steam 1172470 --depot 1172471 --out out convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'
```

## Decoder
- **`oozextract`** (pure Rust, decoder only) in `respak-respawn`. It is a port of ooz (powzix), a reverse-engineered Oodle decoder: Kraken, Mermaid, Selkie, Leviathan, plus LZNA and BitKnit.
- **License:** the crate says MIT. ooz is GPL-3.0-or-later, so treat it as GPL-3.0+: compatible with AGPL-3.0-only. The proprietary oo2core (rsx `thirdparty/oodle/`) is never linked; neither are the `oodle`, `oodle-sys` or `oodle-safe` crates.
- **API:** `Extractor::new()` (about 768 KiB of scratch, so reuse it) and `read_from_slice(input, out)`. The output buffer must have the exact decoded length, which every call site knows.
- **Version:** 0.5.5 (2026-08-12) lacks a fix for tail over-reads that is on `dev` ("don't fail on the unaligned tail over-reads ooz performs"). At the sketch: 0.5.6 if released, otherwise a git rev.
- **Not yet proven:** that it decodes Apex data. No public tool does this yet (rsx and LegionPlus link oo2core). **Check this first**, on one real pak and one real starpak mip, before anything else in this slice.

## rsx review
(Subagent sweep.)
- **What it does:**
  - Every Oodle call goes through `RTech::DecompressStreamedBuffer` (`src/game/rtech/utils/utils.cpp:2259-2296`, `:2349-2385`): `OodleLZDecoder_Create(rawLen)` + a `DecodeSome` loop, with the whole output as the window, so it is effectively a whole-buffer decode. Flags: fuzz-safe off, CRC off.
  - **Pak body** (flag `1 << 9`, pak v7/v8): `cpakfile.cpp:800-842`. For Oodle, `cmpSize` covers the body only, **without the header**, unlike RTech. The output is `dcmpSize - pakHdrSize`.
  - **Streamed txtr mips** (v9/v10, codec 3): `texture.cpp:235-263`, `:1203-1206`. The input is the padded block `(compressedBytes[i] + 1) << 12`; the output is `slicePitch`; the decoder stops at the end of its stream, so the padding is ignored.
  - **Later, with their asset kinds:** model LOD vertex groups (mdl v16–v20, `model.cpp:523-530`), wrap (`wrap.cpp:108-112`), uiia (`ui_image.cpp:160-163`, `:291-294`, `:365-368`). `ramen.cpp` is rsx's own cache: not game data, not ported.
- **Problems:**
  - The compressed length passed is `bufSize - bufPos`, a decompressed size, which over-reads the input.
  - If the first `DecodeSome` fails, the input is returned unchanged as if it were uncompressed.
  - The decoded length is never checked.
  - uiia passes `streamedSize` as the output size (looks like an rsx bug, unverified).
- **Fit:**
  - `oodle::decompress(input, decoded_len) -> Result<Vec<u8>>` in `respak-respawn`, next to `rtech::decompress`.
  - A short or failed decode → `Corrupt`, never the input unchanged.
  - Panics inside the decoder (one `panic!` in `decoder/pointer.rs`) are caught → `Corrupt`.
  - Pak body: a `Body::Oodle` variant in `rpak.rs` next to `Body::Rtech`, decoding `file[header_len .. header_len + cmp_size]` whole (the same cost as RTech, PROJECT.md §11). The rejection at `pak.rs:242-244` goes away. `rpak.rs:418` computes `needed` from `compressed_size`; fix that for Oodle's body-only size.
  - txtr: codec 3 at `txtr.rs:312` decodes the padded block to `mip.len`.

## Findings (check on a real pak, 2026-10-13)
`common.rpak` (v8, flags `0x220`, 690 MB, 1.56 GB decoded) is Leviathan, no block checksums. `oozextract` alone fails on it; two things are needed:
- **Restart points.** Every 16 MiB (64 blocks) the block header has the restart bit (`0x80` in byte 0) and the window starts anew. `oozextract` ignores that bit for Kraken/Mermaid/Leviathan. So `oodle::decompress` walks the block headers (2-byte block header + 3-byte quantum header, which gives the compressed size), cuts the stream at the restart points and decodes each segment on its own into `out[seg_start..seg_end]`. Segments are independent: decode on demand.
- **Upstream bug.** `leviathan.rs:545` rejects a literal run that ends exactly at the match zone end (`Expected 8 < 8`). Fixed in our fork `github.com/chiririll/oozextract`, branch `respak` (on top of upstream `dev`, which has the tail over-read fix), pinned by rev in the workspace `Cargo.toml`. To report upstream.
- Whole body decoded in about 5 s. Downloading: the headers are spread over the file, so finding segment k reads all of the file before it. A prefix is enough (like RTech), not a slice from the middle; cache the scan.

## Decisions at the gate
- Oodle is called directly, like RTech (no codec edge from PROJECT.md §6).
- No comparison with rsx output in the live test or the demo: the live check uses sanity checks (decoded length, tables parse, a valid PNG).
- Research files (real paks) live in `.respak/research/` (git-ignored); never used by tests.

## Scope
`oodle::decompress`; the Oodle pak body; Oodle streamed mips. zstd stays unsupported; Snowflake is still open.

## Leaves out
Model, wrap and uiia Oodle data (with those kinds, task 14).

## Open question (decide at the sketch)
PROJECT.md §6 says codecs are shared mediator edges (`compressed → blob`), but none is registered: RTech is called directly (`rpak.rs:71`, `txtr.rs:308`). Should Oodle follow RTech (a direct call), or should this slice turn both into the edge §6 describes? Edges are dispatched by format, so that would also mean one handler switching on `codec`, or one format per codec.

## Tests (agreed at the gate)
Built (all pass):
- `oodle` unit tests on hand-built stored streams: round trip across blocks, restart bits cut segments, padding after the stream is ignored, a partial input asks for more, truncated or wrong-length input is `Corrupt`, garbage never panics, checksums and other decoders are `Unsupported`.
- rpak (v7, v8; restart every block, every 2 blocks, never): an Oodle pak (body-only size) lists its assets and serves them; a short file or a bad stream is `Corrupt`.
- `Body::Oodle` (unit): a read across segments is right; a read in the last segment decodes only that segment; a stream that ends early is `Corrupt`, the part before the cut still reads.
- txtr codec 3: a stored stream in a 4096-byte padded block decodes to exactly `mip.len`; a short or bad stream fails with "oodle"; Snowflake (codec 2) is still `UnsupportedVersion`.
- zstd paks still give `UnsupportedVersion`.

Not done:
- **Real compressed streams as fixtures.** There is no compressor here (rarten/ooz `-z`), and the stored streams do not exercise the Leviathan decoder or the restart fix. What covers them now is the research run on a real pak (below), which is not in the test suite. Needs your call: generate small Kraken/Mermaid/Leviathan fixtures offline and commit them, or leave it.
- **Live** (env var, through 11b): one current-Apex texture with Oodle *streamed* mips. Needs your permission to run against Steam.

Research run (not a test, real file in `.respak/research/`, git-ignored): `common.rpak` (690 MB, 180 958 assets) lists in 0.9 s; a permanent-mip texture converts to a correct PNG.

## Demo
The user converts textures from the **current** Apex version over Steam and compares them with rsx.

## Risks
- Apex data not decoding at all. Then: stop and tell the user (fallback: the host-supplied DLL from PROJECT.md §6).
- Streams with the per-block checksum flag. oozextract skips those 3 bytes (that code is commented out), so it would misread the stream; whether Apex sets the flag is unknown.
- Fuzz safety is not guaranteed.
