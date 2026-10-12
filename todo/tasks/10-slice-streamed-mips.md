# 10 · Slice: streamed mips from starpaks
**Status:** done · **Depends on:** 09 · **See:** PROJECT.md §6, §12

## Goal
Full-resolution textures, with proof that only the needed bytes are read:
```
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'   # now full size
```

## rsx review
(Subagent sweep, see also 08.) **Starpak file:** only a trailer is read: the last 8 bytes are a u64 `entryCount`; before it `entryCount` entries `{u64 offset, u64 size}` (16 bytes); entries with size 0 are dropped; the TOC must fit in the file. No header or magic. Entry offsets are 4096-aligned. **Reference:** an asset's `starpakOffset` is `-1` (none) or `index = v & 0xFFF` into the pak's mandatory path list and `offset = v & ~0xFFF`; v8 assets also have `optStarpakOffset` against the optional list. The range is looked up by exact masked offset in the TOC (`{0,0}` on a miss = unavailable); the TOC `size` is not used to bound reads; mip sizes come from the texture header. **Paths:** the pak's path (`paks\Win64\pc_all.starpak`) is reduced to its file name (split on both `\` and `/`) and looked up next to the rpak; missing = warning, placeholder entry so indices stay aligned. **Layout of streamed data:** within an entry v9/v10 store mips largest first (`E + 0` is the largest streamed mip), v8 smallest first; each mip is `align16(size)` per array slice, or `(compressedBytes[i]+1) << 12` if compressed. **Problems:** do not use the TOC size as one mip's length; check that the mips fit the TOC entry and the file; handle `-1`, an index past the list, Windows paths; a decoder must get the padded input and the exact decoded length.

## Scope
Starpak handling is ported from rsx (`starpak_source.h`, `cpakfile.{h,cpp}`: offset encoding, mandatory vs optional, the table of contents at the file end, path resolution). Review it first, then port it (PROJECT.md §12).

**respak-respawn:**
- Starpak volume (`x.starpak/<offset>`, sized by its table of contents), opened lazily.
- `texture/foo.txtr.stream` / `.opt` in the rpak volume, mapped onto ranged reads on the starpak.
- Companion files are opened only when the current request needs them. A missing mandatory starpak → `MissingDependency{item, file}`. A missing optional starpak → fall back to lower mips with a warning.

**Tests:** a byte-counting `Operator` layer on a synthetic multi-GB starpak shows that only the table of contents plus the requested ranges are read.

## Demo
The user converts a texture with streamed mips and compares the size and quality with rsx.

## Done
- `respak-respawn/src/starpak.rs`: `Starpak` (trailer table of contents, ranged reads), the `starpak` format (container; entries `0x<offset>`), `split_reference`, `file_name` (Windows paths).
- rpak volume: `<asset>.stream` / `.opt` resolve to the starpak entry at the asset's exact offset (not listed). Starpaks are looked up next to the pak by file name and opened lazily, once. `RpakAsset` carries `stream` / `opt`.
- `txtr`: v9/v10 codec bits (`compTypePacked` at 0x1C v9 / 0x1E v10, `compressedBytes[7]` after it), mip layout per starpak entry (v8 smallest first, v9/v10 largest first), mips checked against the entry size. Missing mandatory starpak or entry → `MissingDependency`; missing optional → warning and the chain starts at the first non-optional mip.
- Compressed streamed mips: codec 1 (RTech) goes through `rtech::decompress(block, 0, Some(len))`. **Not verified against real data** (no synthetic RTech encoder); codecs 2 (Snowflake) and 3 (Oodle) give `UnsupportedVersion` when the mip is read. Arrays with compressed streamed mips are rejected (rsx only asserts `arraySize == 1`).
- Test: a synthetic 2 GiB starpak behind a counting `VolumeSource`: opening reads 24 bytes of it, reading a mip reads exactly that mip.
- The Warning events for the missing-optional case are `tracing::warn!` until slice 12.
