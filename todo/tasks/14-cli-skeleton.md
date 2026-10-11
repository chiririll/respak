# 14 · CLI skeleton
**Status:** todo · **Depends on:** 11–13 · **See:** PROJECT.md §10

## Scope
- `lib.rs`: `async fn run(cli, engine, out) -> Result<ExitCode>`, testable in-process with `services::Memory`.
- `main.rs`: `services::Fs` operators from `--root`, `--out`, optional `--cache`; `--search <glob>...`; registers plugins; events to stderr; Ctrl-C → cancel.
- Commands: `formats`, `discover <path>... [--depth N]`, `run <request.json | ->`, `convert <path|glob> --to '<json>' [--path P]`. Non-zero exit if any output failed.

## Acceptance
- `cargo run -p respak-cli -- --help` and the subcommand help pages work.
