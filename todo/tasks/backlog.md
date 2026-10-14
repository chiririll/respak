# 14 · Backlog
**Status:** blocked

Each item becomes its own vertical slice (README: how a slice works), ending in something the user can run. Each Respawn kind's rsx code (`src/game/rtech/assets/*.cpp`) is reviewed first, then ported (PROJECT.md §12).

## New asset kinds

**Respawn kinds** (`respak-respawn`):
- model (`rmdl`) → Cast, including its streamed vertex data (`Mesh` / `Skeleton`)
- animation sequence (`aseq`), anim rig (`arig`) → `Animation`
- datatable (`dtbl`) → CSV/JSON → `Table`
- UI image atlas (`uimg`, rsx `'gmiu'`, `ui_image_atlas.cpp`): atlas texture by guid + per-sprite UV rects; export as image + JSON with an option to slice into sprites
- shaders, settings, subtitles, localization
- Respawn VPK
- … every kind rsx supports

**Common formats** (`respak-formats`): Cast, SMD + QC.

**Audio** (`respak-miles`):
- Miles banks (`mbnk`, `mstr`) → `Audio` (spec: [`../formats/miles.md`](../formats/miles.md), [`../formats/miles-codecs.md`](../formats/miles-codecs.md))
- RAD Audio (`RADA`, Apex S24+) decoder: the first Miles slice reads the header only, decoding gives `UnsupportedVersion`. No open decoder exists; the only source is RAD's SDK inside Unreal Engine (EULA, not usable). Needs a clean-room reverse engineering or a future open decoder (miles-codecs.md, open question 1)

## Follow-ups to built slices

- uiia / Snowflake: `snowflake::decompress` is verified on one real stream (the high resolution of `ui_main_menu`); txtr streamed mips with codec 2 still say `UnsupportedVersion` (one-line wiring, untested on real data); Oodle/RTech high-resolution uiia data is untested (rsx decodes to `streamedSize` bytes, a likely bug); the Snowflake multi-block path (blocks over the block size, 16 MiB here) has no real-data test
- material (from slice 13): v12 (Titanfall 2) `matl` layout; dxStates / blend and rasterizer state, uber data and texture animation in the material JSON; the v23 snapshot (`msnp`); shader variants other than the first; SM5.1 DXBC; material name repair for old paks (rsx brute-forces the shader-type suffix against the guid)
- Bink 2 in-loop deblocking (`bik` video, `chiririll/nihav` fork, `bink2.rs`): the decoder parses the row/column deblock flags (`frame_flags & 0x10000`) but never applies the filter, so clips with a lot of motion drift over time (measured on `media/frontend.bik`, 2969 frames, one keyframe: mean abs. difference vs. the RAD reference about 2 at frame 1, 9 at frame 500, 27 at frame 2900; hairlines on 16-row block edges that grow by ~1.5 levels per frame, and colour bleeding next to hard edges). `media/respawn.bik` (little motion) stays within about 1.5-3.3. Strength depends on MV differences and the number of coded coefficients, 0 to 2 pixels per side, horizontal edges first (<https://codecs.multimedia.cx/2019/04/bink2-some-words-about-loop-filter/>); no formula is public and FFmpeg's Bink 2 has no deblocking either. Plan: reverse-engineer RAD Video Tools in Ghidra. Reference frames: `.respak/research/frontend/input.mp4` (H.264 re-encode, so it has its own noise). Also unverified: chroma upsampling in `yuv420_to_rgba8` (about +1 level in R/B on the keyframe), and a likely typo in `MVInfo::predict_mvs` (top row, `mv1` uses `mv_pos - 1` where `decode_mv_new` uses `mv_pos - 3`)

## Core features

- raw dump of a pak asset with `copy` / `convert --copy`: today they skip assets (an asset is not a file: header in the rpak pages, streamed data in the starpak). A dump would write the header plus the streamed blobs as a bundle, per kind
- the in-memory cache (PROJECT.md §7). It also fixes the dedup gap: the dedup shares only **concurrent** requests, so a texture used by two materials that do not overlap in time is decoded twice
- rpak patches (PROJECT.md §11)
- writing Respawn formats (PROJECT.md §5.8), including the `bundle` rule

## Cleanup

- a dead-code pass over everything built before the slices (the user does this after the full implementation)

The open questions live in PROJECT.md §11.
