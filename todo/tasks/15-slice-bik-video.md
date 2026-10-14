# 15 · Slice: Bink 2 video → PNG frames + WAV
**Status:** in-progress (sketch done; tests to agree at the gate) · **Depends on:** 11b (the demo files come from Steam) · **See:** PROJECT.md §5.4, §6, §12

## Goal
Apex's Bink 2 videos convert to common formats, on Windows and Linux, without RAD's tools or DLLs:
```
respak --steam 1172470 --depot 1172471 --out out convert media/frontend.bik --to '{"type":"png.frames"}'
respak --steam 1172470 --depot 1172471 --out out convert media/respawn.bik  --to '{"type":"wav"}'
```
(`--to` is required by the CLI; `discover` lists the targets.) The user joins them into a video with ffmpeg, e.g. `ffmpeg -framerate 30 -i frame_%05d.png -i respawn.wav out.mp4`. There is no mp4 target (decision 4).

## Spike (done, 2026-10-14, nothing in the repo)
- **Decoder: NihAV** (`nihav_rad`: Bink demuxer, `bink2-video`, `bink-audio-dct`/`-rdft`). Pure Rust, no C. Not on crates.io; AGPL-3.0, compatible with respak (AGPL-3.0-only). **Fork: `github.com/chiririll/nihav`**, full upstream history (bundle from nihav.org) plus three commits: `895ba0d` fixing the DCT audio decoder, `3b973db` fixing Bink 2 inter DC and chroma MC rounding, and `aabd6fc` keeping full precision between the two luma MC passes (without these the picture drifts in brightness and colour over a long clip). Used as a git dependency pinned by rev, like `oozextract` (11c). Crates: `nihav_core`, `nihav_codec_support`, `nihav_rad` (features `demuxer_bink`, `decoder_bink2`, `decoder_binkaud`). Not reported upstream (the user's decision).
- **Video:** `media/frontend.bik`: KB2i, 1920x1080, 30 fps, 210 frames, no audio. The first 20 frames (1 MiB of 10.9 MB) decode to clean pictures, 7.8 ms/frame single-threaded. Output is 8-bit planar YUV 4:2:0, full range (`YUVJ`), no alpha. Not compared with a reference decode.
- **Audio:** `media/respawn.bik`: KB2i, 1920x1080, 30 fps, 9.8 s, one stereo 48 kHz DCT track. NihAV as shipped is wrong (bass/DC coefficients wiped, scale off by √2); with the fork's patch the first 16 packets match ffmpeg's `binkaudio_dct`: 97.8 % of samples identical, max difference 5. Not tested: RDFT audio, the rest of any file, A/V sync over a whole file, KB2 revisions other than `i`.
- ffmpeg has no Bink 2 decoder (only Bink 1). Here it is only a **test oracle** for the audio.

## Decisions (gate, answered by the user)
1. **Home:** `bik` first lived in `respak-respawn`; it moved to `respak-nihav` (Bink is RAD's, and all NihAV code is there). The common targets `wav` and `png.frames` go to `respak-formats` and connect through the capabilities.
2. **Async everywhere:** the capabilities return async streams (not blocking callbacks).
3. **Frame format is configurable:** the consumer asks for `Rgba8` or `Yuv420`; the video converts.
4. **No mp4 / no ffmpeg subprocess** in this slice.
5. **Whole-file read** of the `.bik`.
6. **Tests use real game files** (full `frontend.bik` and `respawn.bik`); **unit tests don't use large files.** Game files are not committed: the real-data tests read them from a path in an env var (`.respak/research/`, git-ignored, like 11c).

## Sketch (built, compiles, bodies are `todo!()`)
**Core** (`respak-core/src/capability/`):
- `video.rs`: `FrameFormat { Rgba8, Yuv420 }`, `VideoInfo { width, height, fps, frame_count, full_range }`, `VideoFrame { index, format, planes, strides }`, `trait Video { fn info(); async fn frames(&self, format, cx) -> Result<BoxStream<'static, Result<VideoFrame>>> }`.
- `audio.rs` (was an empty stub): `AudioTrackInfo { channels, sample_rate }`, `AudioChunk { samples: Vec<f32> /* interleaved */ }`, `trait Audio { fn tracks(); async fn samples(&self, track, cx) -> Result<BoxStream<'static, Result<AudioChunk>>> }`.
- `Cap for dyn Video` / `dyn Audio`, `Data::as_video`, the object-safety check.
- A stream ends after the last frame; a damaged or truncated source yields what it can, then one `Err`. Implementations decode with `cx.blocking` (CPU-bound, bounded by `blocking_limit`) and check cancellation between items.

**`respak-respawn`**:
- `bik.rs`: `BikHeader` (44-byte fixed header, `parse`), `BikRead` (score: `KB2` + a letter = 10, plausible fields +2, extension `bik`/`bk2` +1; `Version { major: 2, minor: letter - 'a' }`; Bink 1 `BIK` is scored too and its read gives `UnsupportedVersion`), `BikFile { bytes, header }` as `Data` + `Video` + `Audio`. Registered as `bik` providing `Video` and `Audio`. `BikRead::score` currently returns `None` (discovery calls every file reader, so it must not panic).
- `bik_decode.rs`: the Bink side of NihAV: `VideoDecoder`, `AudioDecoder` (synchronous, stateful), `yuv420_to_rgba8` (full-range BT.601). NihAV errors → `Corrupt` / `UnsupportedVersion`; decoder panics are caught → `Corrupt`. **The NihAV dependency is added when the implementation starts**, not in the sketch.

**`respak-formats`**:
- `wav.rs`: `WavEncode` (`from_cap::<dyn Audio>`), options `{ "track": 0 }`. 16-bit PCM, `(x * 32768).round()` clamped, hand-written RIFF header.
- `frames.rs`: `FramesEncode` (`from_cap::<dyn Video>`) → format `png.frames`. Asks for `Rgba8`, returns `BundleData::named` (`frame_00001.png …`), a composite output with its own folder. `Progress` per frame.
- `FormatsPlugin` registers `png.frames` and `wav`. The one existing test that lists the formats (`formats_prints_the_graph`) now expects `["png", "dds", "png.frames", "wav"]`.
- New dependency in both crates: `futures` (already a workspace dependency).

`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass with the sketch.

## Tests (proposed, for you to agree or change)
**Unit** (small, hand-built, no game files):
- **Header / detect:** a 44-byte `KB2i` header parses (frames, size, fps, tracks) and scores 10+ with `Version 2.8`; `.bik`/`.bk2` +1; random bytes, a zero frame count/size/fps and a short header are not scored; a `BIK` (v1) header reads as `UnsupportedVersion`, not `UnknownFormat`; `discover` of a header-only file lists `png.frames` and `wav` as targets.
- **`yuv420_to_rgba8`:** known values (black, white, mid gray, saturated colours) match BT.601 full range; odd width/height (chroma rounds up); stride larger than width.
- **wav (stub `Audio`):** header fields (rate, channels, byte rate, data size) are right; samples round, not truncate, and clamp at ±1.0; chunked input equals one-chunk input; track out of range and no tracks give a clear error; cancellation per chunk writes nothing.
- **png.frames (stub `Video`):** N frames → N named PNGs in one folder, each with the expected pixels; `Progress` reaches N/N; cancelling in the middle gives `Cancelled` and writes nothing; the stub is asked for `Rgba8`.
- **Corrupt input:** a valid header followed by garbage never panics: the stream gives `Corrupt`.

**Real files** (full `frontend.bik` and `respawn.bik` via env vars; skipped with a message when unset; not committed):
- `frontend.bik`: `info()` says 1920x1080, 30 fps, 210 frames; all 210 frames decode; first, middle and last are not blank; `Rgba8` equals `yuv420_to_rgba8` of `Yuv420`; the `png.frames` target writes 210 PNGs.
- `respawn.bik`: one track, 48 kHz stereo; the sample count matches the duration (9.8 s); the wav is not silent; **matches `ffmpeg -vn` output** (max difference ≤ 5 over the common part; skipped if ffmpeg is not on `PATH`); audio duration is within one frame of the video's.
- Cancelling mid-decode on a real file returns `Cancelled`.

Always: clippy `-D warnings`, `cargo test --workspace`, `cargo tree -p respak-formats` / `-p respak-respawn` show no plugin-to-plugin dependency (`respak-respawn` → `respak-formats` already exists today).

## Leaves out
mp4 and any ffmpeg subprocess (a y4m target or an ffmpeg muxer are backlog candidates); Bink 1; RDFT audio (untested); several audio tracks in one request (`track` picks one); frame ranges and seeking; alpha; KB2 revisions other than `i` are attempted but untested; writing Bink; ranged/streaming reads of huge files; reporting the NihAV bugs upstream.

## Demo
The user runs the two commands above over Steam on `frontend.bik` and `respawn.bik` (live Steam access only with the user's permission), then joins frames and wav with ffmpeg and watches the result (picture and sound in sync).

## Risks
- Only two files seen (both KB2i), one 1 MiB prefix each. Later frames, scene cuts and other files may expose NihAV decoder bugs (its author calls the Bink 2 decoder imperfect, notably the deblocking filter). If a real file shows artifacts: stop and tell the user.
- A/V sync over a whole file is unverified (the demuxer gives each frame's audio packets the frame's timestamp).
- The real-data tests need the full files; only 1 MiB prefixes are cached today, so the full files come from Steam (with the user's permission).
- Whole-file read of large videos; the git dependency needs network on the first build.
- A frame stream built on `cx.blocking` calls: the decoder is stateful, so each call hands it to a blocking task and back (a mutex or an owned state moved in and out).
