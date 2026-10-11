# 25 · Backlog: more formats
**Status:** blocked

Each item gets its own small cycle: interface check, then test cases with the user, then TDD implementation.

**RPak asset kinds** (`respak-rpak`). Use rsx `src/game/rtech/assets/*.cpp` as the reference:
- material (`matl`)
- model (`mdl_`), including its streamed vertex data
- animation sequence (`aseq`)
- anim rig (`arig`)
- datatable (`dtbl`)
- UI image / atlas (`uiia`, `uimg`)
- shaders
- settings, subtitles, localization
- … every kind rsx supports

**Output formats** (new crates, depending only on core):
- `respak-cast`: models and animations in Cast
- `respak-smd`: SMD + QC
- table export (CSV/JSON)
- audio formats

**Other input families:**
- `respak-miles`: Miles banks (`mbnk`, `mstr`)
- `respak-source`: Source-era MDL v52/53, VPK
- (Titanfall 1/2)

**Linking examples to verify:**
- the Cast model encoder requests textures through `Encode`
- material export requests its textures
