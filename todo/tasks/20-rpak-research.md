# 20 · RPak format research
**Status:** blocked (waits for 17) · **Depends on:** 17

## Goal
Write down the RPak format knowledge that respak needs, taken from the rsx sources (`C:\Users\chiririll\Projects\rsx`). Write it into `docs/formats/rpak.md` in this repo. rsx is read only as a reference; no code is copied.

## Questions to answer
- Header layouts for each version (v6 / v7 / v8+) and how versions are detected (see `src/game/rtech/cpakfile.h`: `PakHdr_v6_t`, `PakHdr_v7_t`, `PakHdr_v8_t`).
- Asset entry layout per version: GUID, type FourCC (note the byte order), version, header and page pointers, dependencies.
- **Compression:** is the body one compressed stream? Which codecs (RTech, Oodle, zstd) are used? Can it be stream-decompressed so that only the tables and needed pages are kept? This confirms or corrects the caveat in PROJECT.md §7.
- Starpaks: how an asset points into a starpak (offset and size), mandatory vs optional starpaks, how their paths are resolved (see `starpak_source.h`).
- Patches: `patch_master.rpak` and the `name(NN).rpak` naming.
- GUID hashing of asset paths (see `RTech::StringToGuid` usage and `src/core/cache/cachedb.h`).
- Texture (`txtr`) header and mip layout per version (`src/game/rtech/assets/texture.cpp`), including the PS4 swizzle.
- Legal note: decompression codecs such as Oodle may need special handling. Flag any licensing question to the user.

## Done when
`docs/formats/rpak.md` exists and the user has seen the summary of open points.
