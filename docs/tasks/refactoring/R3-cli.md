# R3 · CLI cleanup
**Status:** partly done (R3.1, R3.2 and the tail of R3.4 left) · **Risk:** low–medium · **Depends on:** R2

Goal: shorter, more readable commands. Flags may go after the subcommand, a project config / profile supplies the defaults (`respak apex convert ...`), and bare `respak convert` prints help for `convert`.

Order: R3.0 → R3.3 → R3.4 → R3.5 → R3.6 → R3.7 → R3.8 → R3.9. R3.1 and R3.2 are independent and not required for the config work.

## R3.0 CLI unit tests (S, do first) — **done**
No tests exist for `parse_size` (`lib.rs`) and `event_line`. Add table-driven tests before touching them.

## R3.1 Move engine logic out of the CLI (M, after R2.3)
- `commands/dump.rs` does its own chunked volume copy with blocking `std::fs`, duplicating `Mediator::copy_blob` and `Engine::read_file`. Add `Engine::dump(path)` that writes through the output operator, so `--out` shares conflict handling and events.
- `commands/names.rs` drives the whole scan loop (expand → inspect → discover fallback → read as text). Add an `Engine` name-scan API or a visitor. `names::run` is ~120 lines with deeply nested arms; split into probe / feed / report steps.
- `dump` silently `continue`s when `open` doesn't return exactly one value; report it.

## R3.2 Deduplicate command boilerplate (S–M)
`list`, `discover`, `names`, `dump` and `output::report` repeat the `failed`/`ExitCode` loop. Add a `Failures` collector and an exit-code helper. Consider making `copy` a form of `convert --to file`; drop `output::file_target` once R2.5 lands. Replace the magic `version: 1` in the convert/copy requests with a constructor. Simplify the nested `matches!` in `output::skip_assets`.

## R3.3 One event printer (S) — **done**
`lib.rs:156-195` (`run_with_events`) duplicates the match arms of the `recv()` loop in the `try_recv()` tail loop (and the Lagged message); `main.rs:60-68` adds a second subscriber for Steam events. Extract `print_event` and use it for both.

## R3.4 Tidy `main.rs` and dispatch (S–M) — **mostly done**
Done: `main()` split (`init_logging`, `spawn_ctrl_c`, `open_report`; `open_input` and `build_engine` live in `setup.rs`), one `cfg(steam)` input block, the Steam subscriber uses `print_events_to_stderr`. Not done: the error convention (`core::Error` vs `anyhow`). Skipped: `RespawnOptions` (the plugin now takes four options, all in `Settings`); the `Command::Steam` arm of `run` stays, since `run` handles every command and `main` just short-circuits it. `parse_size` / `event_line` stay `pub` (integration tests use them).
- `Command::Steam` is handled in `main.rs:30-33` and again in `lib.rs:208`: remove the dead arm.
- Split `main()` (~110 lines): `init_logging`, `spawn_ctrl_c`, `open_input`, `build_engine`, `open_report`. Merge the two `cfg(feature = "steam")` blocks that build `input`.
- Move engine assembly to `respak_cli::build_engine(&Settings, ..)` (R3.5); check that `integration/tests/*.rs` don't re-assemble it.
- Build the respawn plugin from a `RespawnOptions` struct instead of mirroring the CLI options by hand (`main.rs:85-97`).
- One error convention: `core::Error` for request problems, `anyhow` only at `main`.
- Make `parse_size` / `event_line` / `run_steam` `pub(crate)` unless tests need them.

## R3.5 `Settings::resolve` (M) — **done**
Deviations: `--out` stays required for every command, because the engine builder demands an output operator (making it optional needs an engine change). `--steam` still `conflicts_with = "root"` in clap, since both are command-line flags; only the `requires` attributes moved into `resolve`. `main` copies the resolved output directory into `cli.out` because `dump` still reads it there (goes away with R3.1).
`Cli` stays the clap struct; add `Settings`, the resolved view (non-optional input source and `out`), built by `Settings::resolve(cli, config)`.
- `Cli.rig`, `Cli.studio`, `Cli.branch` become `Option`s (defaults `auto` / `auto` / `public` applied in `resolve`). `integration/tests/aseq.rs:570-572` reads `cli.rig`/`cli.studio` and needs updating.
- `resolve` is the single place for:
  - the required `--root`/`--out` checks (today in `main.rs:34-36`, `:74-77`, `:81-84` and `dump.rs:30`); `--out` only for commands that need it;
  - the Steam rules (`--steam` conflicts with `--root` and requires `--depot`; `--branch`, `--depot`, `--steam-cache-size` require `--steam`). Drop the clap `conflicts_with`/`requires` attributes, since config-supplied values bypass clap. Move the assertions in `tests/steam.rs:82` to `resolve` tests;
  - defaults.
- `run` keeps taking `Cli` for now (used by `integration/tests/convert.rs`); revisit after R3.1.
- Behavior unchanged at this step.

## R3.6 Global flags and help (S) — **done**
- `global = true` on `root`, `out`, `search`, `no_patches`, `rig`, `studio`, `cache`, `report` and the Steam flags, so `respak convert X --out out` works. No name clashes with subcommand flags (checked).
- `arg_required_else_help = true` on `discover`, `list`, `names`, `copy`, `dump`, `run`, `convert` and on top-level `Cli`; `steam` gets `subcommand_required` + `arg_required_else_help`. `formats` is unchanged. `respak convert` prints help for `convert` (exit code 2 is kept).
- Tests: flags before and after the subcommand; bare `respak`, `respak convert`, `respak steam` print help.

## R3.7 Config file `respak.toml` (M) — **done**
New `config.rs` in `respak-cli`; new `toml` dependency, `serde` from the workspace; `deny_unknown_fields`.
- Discovery: walk up from the current directory, first match wins; missing file is fine. `--config <FILE>` loads that file only; `--no-config` skips everything. Log the used path at `tracing::info`.
- Schema:
  ```toml
  root = "D:/Games/Titanfall"
  out = "out"
  cache = ".cache"
  report = "report.json"
  search = ["**/*.rpak"]
  no_patches = false
  [respawn]
  rig = "auto"
  studio = "auto"
  [steam]
  app = 1172470
  depots = ["1172471", "1172472=MANIFEST"]
  branch = "public"
  cache_size = "500M"   # parse_size
  ```
- Merge: command line > config > defaults. The input source (`root` / `steam`) is one group: if the command line gives either, the config's are ignored; both in the config is an error. `--depot` / `--steam-cache-size` / `--branch` on the command line are valid when the config supplies `steam`. A command-line `--search` or `--depot` replaces the config's list. `--no-patches` can only enable (a `--patches` counterpart is parked).
- Relative paths in the config resolve against the config file's directory; command-line paths stay cwd-relative. The Steam cache stays `.respak` relative to cwd (parked).
- Tests: precedence, parent-directory discovery, relative-path resolution, unknown keys, input-source group, builds with and without the `steam` feature.

## R3.8 Profiles `respak [profile] <command>` (M) — **done**
- Profile `apex` loads only `respak-apex.toml` (same walk-up search); `respak.toml` is not read. A missing file is a hard error: `profile 'apex': respak-apex.toml not found in <cwd> or any parent`. `--config <FILE>` overrides profile lookup; a profile given together with `--config` is ignored with a warning.
- Parsing: `#[command(external_subcommand)] Profile(Vec<OsString>)` on `Command`. If the first parse yields `Profile([name, rest..])`, remove that token from argv by position (tail length) and re-parse, so global flags before the profile keep working. Add `-p/--profile <NAME>` as an equivalent escape hatch.
- Profile names: `[A-Za-z0-9_-]`, not starting with `-`. A command name can never be a profile word (it parses as the command); `-p NAME` is the way to use one. No reserved-list test is needed.
- Document `[PROFILE]` in `--help` (`override_usage` / `after_help`).
- Tests: `respak apex convert ...`; `respak --out x apex convert ...`; `respak steam login` is not a profile; a typo like `respak covnert` gives the missing-profile error; `--config` plus a profile warns; profile found in a parent directory.

## R3.9 Docs (S) — **done**
README examples (`README.md:41`, `:48`) and the global-options table and examples in `docs/cli.md`; add config / profile sections; mention the new help behavior.

## Parked
`--patches` counterpart; `respak profiles` (list); `respak init <name>`; `extends` / shared base for profiles; Steam cache relative to the config directory; `--to dds` shorthand for `convert`; renaming `convert`/`copy` `--path` to `--dest` and `list --map`/`--names` clean-up; positional naming (`paths`) and the shared `InputPaths` group; `rig`/`studio` as `value_parser`s.

