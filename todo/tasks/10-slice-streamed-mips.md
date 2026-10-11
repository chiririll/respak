# 10 · Slice: streamed mips from starpaks
**Status:** blocked · **Depends on:** 09 · **See:** PROJECT.md §6, §12

## Goal
Full-resolution textures, with proof that only the needed bytes are read:
```
respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'   # now full size
```

## rsx review
(filled in during the sketch, approved at the gate)

## Scope
Starpak handling is ported from rsx (`starpak_source.h`, `cpakfile.{h,cpp}`: offset encoding, mandatory vs optional, the table of contents at the file end, path resolution). Review it first, then port it (PROJECT.md §12).

**respak-respawn:**
- Starpak volume (`x.starpak/<offset>`, sized by its table of contents), opened lazily.
- `texture/foo.txtr.stream` / `.opt` in the rpak volume, mapped onto ranged reads on the starpak.
- Companion files are opened only when the current request needs them. A missing mandatory starpak → `MissingDependency{item, file}`. A missing optional starpak → fall back to lower mips with a warning.

**Tests:** a byte-counting `Operator` layer on a synthetic multi-GB starpak shows that only the table of contents plus the requested ranges are read.

## Demo
The user converts a texture with streamed mips and compares the size and quality with rsx.
