# Tasks

Read [`../PROJECT.md`](../PROJECT.md) first. It holds the full design and the working rules. The design is the target, not a to-do list: each part of it is built when a slice needs it.

Status values: `todo` · `in-progress` · `blocked` (waiting on the user or another task) · `done`. Update the status in the task file and in this table.

## How a slice works
From task 05 on, every task is a **vertical slice**: a thin path through every layer it touches (plugin → core → engine → CLI) that ends in a command the user can run. Build only what the slice's demo needs. Each task lists what it leaves out.

1. **Sketch:** interfaces for this slice only (compiles, `todo!()` bodies), plus a proposed list of test cases. A slice that ports rsx code also writes its rsx review (PROJECT.md §12).
2. **Gate (user):** the user reviews the sketch and agrees on the tests. Write the agreed list into the task under `## Tests`. Never invent the test suite alone.
3. **Build:** test-first until the tests and the demo pass.
4. **Demo (user):** the user runs the demo. Record what was learned, and any design change, in PROJECT.md before the next slice starts.

**If the design in PROJECT.md doesn't hold up in a slice, stop and tell the user.** Don't work around it. Catching that early is the reason for slicing.

Every slice ends with:
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass; public items have docs.
- `cargo tree -p respak-respawn` / `-p respak-formats` show no plugin-to-plugin dependency, and core has no format knowledge.
- Fixtures are synthetic and generated in tests. No game files in the repo. No Steam or downloader code.
- Don't commit unless the user asks.

## Foundation (done)
| # | Task | Depends on | Status |
|---|---|---|---|
| 01 | [Workspace setup](01-workspace-setup.md) | — | done |
| 02 | [Core: errors](02-core-errors.md) | 01 | done |
| 03 | [Core: values, formats, keys](03-core-values.md) | 02 | done |
| 04 | [Core: capabilities](04-core-capabilities.md) | 03 | done |

## Slices
| # | Task | Demo | Depends on | Status |
|---|---|---|---|---|
| 05 | [Walking skeleton: PNG ↔ DDS](05-slice-png-dds.md) | PNG → DDS → PNG round trip | 04 | done |
| 06 | [`formats` and `discover`](06-slice-discover-formats.md) | `discover icon.png` | 05 | done |
| 07 | [Open an rpak, list its assets](07-slice-rpak-listing.md) | `discover common.rpak --depth 1` | 06 | done |
| 08 | [rpak texture → DDS (permanent mips)](08-slice-txtr-to-dds.md) | `convert …/foo.txtr --to dds` | 07 | done |
| 09 | [BC decoding and target options](09-slice-txtr-to-png.md) | `convert …/foo.txtr --to png` | 08 | blocked |
| 10 | [Streamed mips from starpaks](10-slice-streamed-mips.md) | full-size texture, byte count | 09 | blocked |
| 11 | [Batch requests, globs, composite outputs](11-slice-batch-requests.md) | `run request.json` | 10 | blocked |
| 12 | [Events, progress, cancellation](12-slice-events-cancel.md) | progress + Ctrl-C | 11 | blocked |
| 13 | [Linking: a material with its textures](13-slice-linking-material.md) | material + textures | 12 | blocked |
| 14 | [Backlog](14-backlog.md) | — | — | blocked |

Respawn format code comes from rsx: reviewed first, then ported (PROJECT.md §12).
