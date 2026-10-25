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
| 20 | [Subtitles → table (CSV / JSON), TXT](20-slice-subt.md) | `convert …/x.subt --to '{"type":"json"}'` | 07, 08 | in-progress (built; real-file check done; demo pending) |
| 21 | [`respak-host` crate](21-slice-host-crate.md) | `cargo test -p respak-cli` unchanged | none | in-progress (built; demo pending) |
| 22 | [Target option schemas](22-slice-option-schemas.md) | `discover` shows a `schema` per target | none | in-progress (defaults only; JSON Schema not built) |
| 23 | [Web: server, settings, explorer](23-slice-web-explorer.md) | `cargo run -p respak-web`, browse a folder | 21 | in-progress (built; demo pending) |
| 24 | [Web: viewers](24-slice-web-viewers.md) | open a texture, atlas, audio in the browser | 23 | in-progress (built; demo pending) |
| 25 | [Web: convert, basket, jobs](25-slice-web-convert.md) | tick files, convert, download | 22, 24 | in-progress (built; demo pending) |
| 26 | [Web: Steam login](26-slice-web-steam.md) | log in by QR, browse a depot | 23 | in-progress (built; never run against Steam) |
| 27 | [Web: version catalog](27-slice-web-catalog.md) | pick "Season 20 launch" | 26 | in-progress (built; shipped catalog has no versions) |
| 28 | [Web: release packaging](28-slice-web-release.md) | CI-built `respak-web` archives | 23 | in-progress (workflow edited; not run) |
| 29 | [In-memory cache (core)](29-slice-memory-cache.md) | a texture shared by two materials converts once | 13 | in-progress (built; demo pending) |

Slices 21-28 build the web UI ([webui.md](../design/webui.md), guide: [../web.md](../web.md)). They were built in one go without the sketch gates, at the user's request; each task file has a `## Built` section with what was decided on the way.

Respawn format code comes from rsx: reviewed first, then ported (project.md §12).
