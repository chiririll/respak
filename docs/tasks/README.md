# Tasks

Read [`../design/project.md`](../design/project.md) first. It holds the full design and the working rules. The design is the target, not a to-do list: each part of it is built when a slice needs it.

Status values: `todo` · `in-progress` · `blocked` (waiting on the user or another task) · `done`. Update the status in the task file and in this table.

## How a slice works
From task 05 on, every task is a **vertical slice**: a thin path through every layer it touches (plugin → core → engine → CLI) that ends in a command the user can run. Build only what the slice's demo needs. Each task lists what it leaves out.

1. **Sketch:** interfaces for this slice only (compiles, `todo!()` bodies), plus a proposed list of test cases. A slice that ports rsx code also writes its rsx review (project.md §12).
2. **Gate (user):** the user reviews the sketch and agrees on the tests. Write the agreed list into the task under `## Tests`. Never invent the test suite alone.
3. **Build:** test-first until the tests and the demo pass.
4. **Demo (user):** the user runs the demo. Record what was learned, and any design change, in project.md before the next slice starts.

**If the design in project.md doesn't hold up in a slice, stop and tell the user.** Don't work around it. Catching that early is the reason for slicing.

Every slice ends with:
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass; public items have docs.
- `cargo tree -p respak-respawn` / `-p respak-formats` show no plugin-to-plugin dependency, and core has no format knowledge.
- Fixtures are synthetic and generated in tests. No game files in the repo. No Steam or downloader code outside `respak-steam` ([steam.md](../design/steam.md)).
- Live Steam tests and any run that talks to Steam are integration tests: never run them without a need and the user's explicit permission ([steam.md](../design/steam.md) §8).
- Don't commit unless the user asks.

## Active
| # | Task | Demo | Depends on | Status |
|---|---|---|---|---|
| 16 | [Miles banks → WAV](16-slice-miles-audio.md) | `convert …/general.mbnk/<name>.asrc --to wav` | 15 | in-progress (built on synthetic data; real-file check pending) |
| 17 | [rpak patches](17-slice-rpak-patches.md) | `respak discover …/common(NN).rpak --depth 1` | 07, 08 | in-progress (built and tested; demo pending) |
| 18 | [rpak model → Cast](18-slice-rmdl-cast.md) | `convert …/x.mdl_ --to cast` | 13 | in-progress (built; Blender import and demo pending) |
| 19 | [rpak animation → Cast](19-slice-aseq-cast.md) | `convert …/x.aseq --to cast` | 18 | in-progress (built on synthetic data; pilot check and demo pending) |

Respawn format code comes from rsx: reviewed first, then ported (project.md §12).
