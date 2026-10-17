# R3 · CLI cleanup
**Status:** todo · **Risk:** low–medium · **Depends on:** R2

## R3.0 CLI unit tests (S, do first)
No tests exist for `parse_size` (`lib.rs`) and `event_line`. Add table-driven tests before touching them.

## R3.1 Move engine logic out of the CLI (M, after R2.3)
- `commands/dump.rs:41-91` does its own chunked volume copy with blocking `std::fs`, duplicating `Mediator::copy_blob` and `Engine::read_file`. Add `Engine::dump(path)` that writes through the output operator, so `--out` shares conflict handling and events.
- `commands/names.rs` drives the whole scan loop (expand → inspect → discover fallback → read as text). Add an `Engine` name-scan API or a visitor.

## R3.2 Deduplicate command boilerplate (S–M)
`list`, `discover`, `names`, `dump` and `output::report` repeat the `failed`/`ExitCode` loop. Add a `Failures` collector and an exit-code helper. Consider making `copy` a form of `convert --to file`; drop `output::file_target` once R2.5 lands.

## R3.3 One event printer (S)
`lib.rs:141-170` duplicates the match arms of the `recv()` loop in the `try_recv()` tail loop; `main.rs:49-58` adds a second subscriber for Steam events. Extract `print_event` and use it for both.

## R3.4 Tidy `main.rs` and dispatch (S–M)
- `Command::Steam` is handled in `main.rs:25-30` and again in `lib.rs:122-124`: remove the dead arm.
- Move engine assembly to `respak_cli::build_engine(&Cli, ..)`; check that `tests/*.rs` don't re-assemble it.
- The `--out required` check is duplicated (`main.rs:31`, `dump.rs:29`); after R3.1 `dump` no longer needs the special `run` signature.
- Build the respawn plugin from a `RespawnOptions` struct instead of mirroring 14+ CLI options by hand (`main.rs:80-88`).
- One error convention: `core::Error` for request problems, `anyhow` only at `main`.
