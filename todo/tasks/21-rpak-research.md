# 21 · RPak format research
**Status:** blocked (waits for 17 and the first-milestone decision in 27) · **Depends on:** 17

## Goal
Write down the RPak knowledge respak needs, from the rsx sources (`C:\Users\chiririll\Projects\rsx`), into `docs/formats/rpak.md`. rsx is a reference only; no code is copied. Use subagents for the source sweep. Limit it to the pak versions chosen for the first milestone.

## Questions to answer
- Header layouts per version (v6 / v7 / v8) and how versions are detected (`cpakfile.h`).
- Asset entry layout: GUID, kind FourCC (byte order), version, `headerStructSize`, header and data pointers, dependencies.
- **Pointer relocation:** how the pak records pointer locations in pages. Does it support the `.mem` relocation scheme in PROJECT.md §6? Report back if not.
- **Compression:** is the body one stream? Codecs and flags. Can it be stream-decompressed, keeping only tables and needed pages?
- Starpaks: offset encoding, mandatory vs optional, table of contents, path resolution (`starpak_source.h`).
- GUID hashing of asset paths.
- Texture (`txtr`) header and mip layout per version and variant (`assets/texture.cpp`), per-mip compression bits, PS4 swizzle.
- Flag licensing questions (Oodle) to the user.

## Done when
`docs/formats/rpak.md` exists and the user has seen the summary of open points.
