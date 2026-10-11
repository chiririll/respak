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
- rpak patches (PROJECT.md §11)
- writing Respawn formats (PROJECT.md §5.8), including the `bundle` rule
- the in-memory cache (PROJECT.md §7)
- a dead-code pass over everything built before the slices (the user does this after the full implementation)

The open questions live in PROJECT.md §11.
