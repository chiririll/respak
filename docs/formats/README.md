# Format specs

One file per format that rsx supports. The *respak support* column says whether respak handles it: `yes`, `partially` (see the spec and `../tasks/backlog.md` for what is left) or `no`. **Each file stands on its own:** an agent reads that one file (plus [`../design/project.md`](../design/project.md) for the rules) and can sketch the slice without opening rsx again. They are research notes, not tasks. A spec becomes a slice (`../tasks/`) when the user picks it, and the slice still goes through the rsx review and the gate (project.md §12, tasks/README).

Already built, with a spec describing what respak implements: [txtr](txtr.md), [matl](matl.md) (v15+, includes the `shds`/`shdr`/DXBC helper), [uiia](uiia.md), [dtbl](dtbl.md), [stgs](stgs.md), [stlt](stlt.md). Already built, no spec here: `rpak`, `starpak`, `uimg`, `ptch`, `bik` (NihAV, not rsx). `shdr` exists only as a helper inside `matl` (`shader.rs`, `dxbc.rs`).

rsx = `C:\Users\chiririll\Projects\rsx`. Paths in the specs are relative to `rsx/src`.

## Index
**Written so far:** every row below (plus the built-format specs above). The specs written in the second research run (source-mdl, font, txan, txls, msnp, efct-rpsk, rlcd, ui, rtk, shdr, shds, rson, locl, rmap, odl, bpk) come from reading the rsx source; offsets are hand-computed. Checked against real Apex paks (each has a "Verified" banner): rlcd, txls, msnp, txan, rson, locl, rmap, odl (`odla`/`odlp`), shds, shdr, efct. **Not yet checked:** ui, rtk, font, odlc, rpsk (the Steam login expired mid-run; `rpsk` is in no current pak), source-mdl, bpk. `mt4a` and `txtx` are not supported by rsx. `wrap`, `wepn` and `impa` are rpak assets, not VPK entries.
| Spec | rsx kind (FourCC) | What | Group | respak support |
|---|---|---|---|---|
| [rmdl](rmdl.md) | `mdl_` | rpak model (studiohdr r5, VG vertex data) | model | partially |
| [source-mdl](source-mdl.md) | `mdl`, `seq` (loose) | Source/R1/R2 loose `.mdl` + `.vtx/.vvd/.phy`, loose `.seq` | model | no |
| [arig](arig.md) | `arig` | animation rig | animation | partially |
| [aseq](aseq.md) | `aseq`, `asqd` | animation sequence and its data | animation | partially |
| [anir](anir.md) | `anir` | animation recording | animation | no |
| [font](font.md) | `font` | UI font atlas | texture/UI | no |
| [txan](txan.md) | `txan` | texture animation | texture/UI | no |
| [txls](txls.md) | `txls` | texture list | texture/UI | no |
| [msnp](msnp.md) | `msnp` | material snapshot | texture/UI | no |
| [efct-rpsk](efct-rpsk.md) | `efct`, `rpsk` | effect, particle script | texture/UI | no |
| [ui](ui.md) | `ui` | UI layout (RUI) | texture/UI | no |
| [rtk](rtk.md) | `rtk` | UI RTK | texture/UI | no |
| [rlcd](rlcd.md) | `rlcd` | LCD screen effect | texture/UI | no |
| [shdr](shdr.md) | `shdr` | shader (standalone) | shader | partially |
| [shds](shds.md) | `shds` | shader set | shader | no |
| [rson](rson.md) | `rson` | RSON | descriptor | no |
| [subt](subt.md) | `subt` | subtitles | descriptor | yes |
| [locl](locl.md) | `locl` | localization | descriptor | no |
| [vpk](vpk.md) | `vpkf` | Respawn VPK | vpk | no |
| [wrap](wrap.md) | `wrap` | wrap (wrapped file) | vpk | no |
| [wepn](wepn.md) | `wepn` | weapon definition | vpk | no |
| [impa](impa.md) | `impa` | impact | vpk | no |
| [miles](miles.md) | `asrc`, `aevt`, `mbnk`/`mstr` | Miles audio banks, sources, events | audio | partially |
| [miles-codecs](miles-codecs.md) | — | Miles payload codecs (BCF, RADA) → PCM | audio | partially |
| [bsp](bsp.md) | — | Respawn BSP maps and entities | map | no |
| [rmap](rmap.md) | `rmap`, `llyr` | map / light layer assets | map | no |
| [odl](odl.md) | `odla`, `odlc`, `odlp` | ODL assets | map | no |
| [ptch](ptch.md) | `Ptch`, `vers` | patch master, patch version | pak | yes |
| [bpk](bpk.md) | `bpwf` | Bluepoint pak | other | no |

A file may be missing or merged with another if the research found the kind too small or not supported by rsx; the agent says so at the top of the nearest file.

## Template
Every spec uses these sections, in this order. Write "none" or "unknown" rather than leaving a section out.

```markdown
# <kind> · <human name>
**rsx sources:** files and functions · **Games / versions:** … · **Priority hint:** how useful, how big (S/M/L)

## Summary
What it is in the game, what a user would convert it to.

## Where it lives
Container (rpak / starpak / vpk / loose file), FourCC as stored and as rsx writes it, extension, name in the rpak volume (`<path>.<ext>`), how the name is found (stored, guid only, brute force).

## Versions and detection
Every version rsx handles, how it tells them apart (asset version field, header size, game), the proposed score function and `Version { major, minor }` mapping. Unknown variant → `UnsupportedVersion`.

## Layout
Every struct, per version: field, offset, size, type, meaning. Byte-exact, little-endian unless stated. Pointers are marked as page pointers (relocated by the rpak volume). Include constants and enums with their values.

## Data outside the header
CPU/page data, streamed data (mandatory/optional starpak, `.stream` / `.opt` entries), embedded blobs; compression (RTech, Oodle, zstd, Snowflake, other).

## Dependencies
Other assets referenced (by guid, by name), whether the output needs them, how rsx resolves them. Maps onto respak linking (project.md §5.5) and `rpak.guid`.

## Decoding
Step by step, enough to write the code. Algorithms in pseudocode where they aren't obvious.

## Output
What rsx exports (formats, options, file layout on disk). Proposed respak targets and capabilities (existing in `respak-core/src/capability/` or new), and the JSON shape if JSON.

## rsx quirks
Bugs, dead branches, hacks, global settings, things to keep or drop when porting (project.md §12).

## Reuse in respak
Existing respak code to build on (`rpak.asset`, `rpak.guid`, `txtr` `Bitmap`, `matl`, …) and what is new.

## Tests
Synthetic fixtures to build (bytes to construct), edge cases, and real files to try (by pak / path, no Steam runs without permission).

## Open questions
For the user to decide at the gate.
```
