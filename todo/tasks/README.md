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
- Fixtures are synthetic and generated in tests. No game files in the repo. No Steam or downloader code outside `respak-steam` ([STEAM.md](../STEAM.md)).
- Live Steam tests and any run that talks to Steam are integration tests: never run them without a need and the user's explicit permission ([STEAM.md](../STEAM.md) §8).
- Don't commit unless the user asks.

## Active
| # | Task | Demo | Depends on | Status |
|---|---|---|---|---|

Respawn format code comes from rsx: reviewed first, then ported (PROJECT.md §12).
