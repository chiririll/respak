# 10 · Slice: streamed mips from starpaks
**Status:** todo · **Depends on:** 09 · **See:** PROJECT.md §6, §12

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
