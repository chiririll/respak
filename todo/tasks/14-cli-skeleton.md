# 14 · CLI skeleton
**Status:** todo · **Depends on:** 11–13 · **See:** PROJECT.md §11

## Goal
Write the command-line host for manual use and later in-process tests. Argument definitions are complete; command bodies are `todo!()`.

## Scope (`crates/respak-cli`)
- `src/lib.rs`: `pub async fn run(cli: Cli, engine: &Engine, out: &mut impl Write) -> anyhow::Result<ExitCode>`. It is testable in-process with `services::Memory`.
- `src/main.rs`, under `#[tokio::main]`:
  - parse the args with clap
  - build `services::Fs` operators from `--root <dir>`, `--out <dir>` and the optional `--cache <dir>`
  - register `RpakPlugin` and `ImagePlugin`
  - spawn a task that prints events to stderr
  - wire Ctrl-C to the `CancellationToken`
- Subcommands:
  - `formats` prints the registered handlers as JSON.
  - `discover <input>` prints the Discovery JSON.
  - `run <request.json | ->` runs a full `ConvertRequest` and prints the `ConvertReport` JSON.
  - `convert <input> --select '<json>' --target '<json>' [--path P]` is the single-output shorthand.
- Exit code: non-zero if any output failed.

## Acceptance
- `cargo run -p respak-cli -- --help` works, and so does `--help` on every subcommand.
