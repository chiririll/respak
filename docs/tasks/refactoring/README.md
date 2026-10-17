# Refactoring

Whole-workspace refactoring, planned from a review of every crate (2026-10-17). Tasks are small, each lands on its own commit, and `cargo clippy --workspace --all-targets -- -D warnings` plus `cargo test --workspace` pass after every one. Tests run locally; there is no CI.

Status values: `todo` · `in-progress` · `blocked` · `done`. Update the status in the phase file and in this table.

## Decisions (user, 2026-10-17)
- No CI task. Tests are run locally.
- `respak-respawn` test builders (`testing.rs`) move into a separate dev-crate.
- Breaking the `respak-core` public API is fine.

## Rules
- Behaviour-preserving only. No new features inside a refactoring task; park ideas in [`../backlog.md`](../backlog.md).
- Public paths of moved items stay the same via `pub use`, except in R5 where breaks are intended.
- Splitting a file: move tests and fixtures out of `src` first, then split production code, one file per commit.
- Review figures (line numbers, counts) come from a skim. Re-check them when starting a task.
- Codec work (`snowflake`, `rtech`) only with the existing vector tests green.
- Don't commit unless the user asks.

## Phases
| # | Phase | Risk | Depends on | Status |
|---|---|---|---|---|
| R1 | [Shared primitives](R1-shared-primitives.md): errors, byte reader, panic fixes | low | | done |
| R2 | [Core restructuring](R2-core.md): mediator, engine, request types | low–medium | R1 | todo |
| R3 | [CLI cleanup](R3-cli.md) | low–medium | R2 | todo |
| R4 | [Large file splits](R4-splits.md): cast, mbnk, testing, rmdl, rpak | low–medium | R1 | todo |
| R5 | [Riskier work](R5-risky.md): stream helper, codec splits, API break, test layout | medium | R2, R4 | todo |
| R6 | [Housekeeping](R6-housekeeping.md) | low | | todo |

R1 goes first because it touches every crate. R4 can run in parallel with R2/R3. R6 can be done at any point.
