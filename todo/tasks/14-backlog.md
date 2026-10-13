# 14 · Backlog
**Status:** blocked

Each item becomes its own vertical slice (README: how a slice works), ending in something the user can run. Each Respawn kind's rsx code (`src/game/rtech/assets/*.cpp`) is reviewed first, then ported (PROJECT.md §12).

**Respawn kinds** (`respak-respawn`):
- model (`rmdl`) → Cast, including its streamed vertex data (`Mesh` / `Skeleton`)
- animation sequence (`aseq`), anim rig (`arig`) → `Animation`
- datatable (`dtbl`) → CSV/JSON → `Table`
- UI image / atlas (`uimg`)
- shaders, settings, subtitles, localization
- Respawn VPK
- … every kind rsx supports

**Common formats** (`respak-formats`): Cast, SMD + QC, audio.

**Other:**
- `respak-miles`: Miles banks (`mbnk`, `mstr`) → `Audio`
- rpak: read only the blocks that are needed. Today (after 11b) a compressed `.rpak` is decoded front to back up to the offset of the last read, so a page near the end costs almost the whole file. Fix: when only the *segments* are compressed and not the whole body (as the header flags / block layout allow), decompress just the segments a request needs, with ranged reads of their compressed bytes. First find out from rsx and real paks which layouts exist and whether RTech blocks refer back across block boundaries; touch nothing else in the rpak volume.
- rpak patches (PROJECT.md §11)
- writing Respawn formats (PROJECT.md §5.8), including the `bundle` rule
- the in-memory cache (PROJECT.md §7)
- a dead-code pass over everything built before the slices (the user does this after the full implementation)

The open questions live in PROJECT.md §11.
