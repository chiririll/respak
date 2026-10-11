# 26 · Backlog: more formats
**Status:** blocked

Each item gets its own small cycle: interface check, then test cases with the user, then TDD implementation.

**Respawn kinds** (`respak-respawn`), reference rsx `src/game/rtech/assets/*.cpp`:
- material (`matl`)
- model (`rmdl`), including its streamed vertex data
- animation sequence (`aseq`), anim rig (`arig`)
- datatable (`dtbl`)
- UI image / atlas (`uimg`)
- shaders, settings, subtitles, localization
- Respawn VPK
- … every kind rsx supports

**Common formats** (`respak-formats`): Cast, SMD + QC, table export (CSV/JSON), audio.

**Other:**
- `respak-miles`: Miles banks (`mbnk`, `mstr`)
- rpak patches (PROJECT.md §11)
- writing Respawn formats (PROJECT.md §5.8)

**Linking examples to verify:**
- the Cast encoder converts its textures to PNG into its own folder
- material export converts its textures
