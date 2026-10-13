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
- ~~rpak: read only the blocks that are needed~~ investigated, **not possible**; nothing to build. A compressed `.rpak` is decoded front to back up to the last read (11b), so a page near the end costs almost the whole file, and that stays. Findings: (1) rsx knows only v6 (raw), v7, v8; the header is raw and the whole body is one compressed stream (RTech, Oodle or zstd), never segment by segment. (2) The RTech output chunks (2^o bytes, per-chunk size fields) are not independent: measured on Apex `common.rpak` (app 1172470, depot 1172471 manifest 6197768190074397522; 620 MB, 4 MiB chunks) 2.8M of 26.8M matches (~10%) point into the previous chunk, none further back (max distance 4194288 < chunk size), so there are no resets and chunk N needs the decoded output of N-1, which needs N-2, and so on. Only starpaks and small embedded blobs (texture mips, LODs) are randomly accessible. Only one pak was measured.
- rpak patches (PROJECT.md §11)
- writing Respawn formats (PROJECT.md §5.8), including the `bundle` rule
- the in-memory cache (PROJECT.md §7)
- a dead-code pass over everything built before the slices (the user does this after the full implementation)

The open questions live in PROJECT.md §11.
