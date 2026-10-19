# 21 · Slice: `respak-host` crate
**Status:** in-progress (built; demo pending) · **Depends on:** nothing · **See:** [webui.md](../design/webui.md) §3, project.md §10, [cli.md](../cli.md#config-file-and-profiles)

## Goal
Move the code both hosts need out of `respak-cli`, so `respak-web` (slice 23) reuses it instead of copying it. **No behaviour change:** the CLI's commands, flags, messages and tests stay as they are.
```
cargo test -p respak-cli          # unchanged tests pass
cargo tree -p respak-host         # no clap, no axum
```

## What moves (from `crates/respak-cli/src/`)
- `config.rs`, `settings.rs`: the `respak.toml` model, lookup (working directory, else nearest parent), profiles, merging of flags over file. The merge takes a plain struct of overrides, not clap types.
- `setup.rs`: plugin registration and `Engine` building from resolved settings, including the input choice (root or Steam) and the `steam` feature.
- Anything else in `lib.rs` / `main.rs` that builds operators or reads the config.

Stays in `respak-cli`: clap definitions, commands, printing, Ctrl-C handling.

## Changes
1. New crate `respak-host` (workspace member and workspace dependency). Features: `steam` (default, forwards to `respak-steam`), same as the CLI today.
2. Move the modules above; the CLI imports them from `respak-host`. Public items documented.
3. Add write support to the config model (needed by slice 23's settings page): serialize back to TOML, keeping unknown-key rejection. Keep comments if the TOML crate allows it (`toml_edit`) **(?)**.
4. `cargo build -p respak-cli --no-default-features` still builds without Steam code.

## Leaves out
Any web code. Any change to flags or file format. The version catalog.

## Open
- Does saving keep the user's comments and key order (`toml_edit`), or is a rewrite acceptable?

## Proposed tests (for the gate)
- All existing `respak-cli` tests pass unchanged (they are the regression suite).
- Config round trip: load → save → load gives the same settings.
- Saving a config with a comment keeps the comment (if `toml_edit` is chosen).
- Engine builds from settings with `services::Memory` operators (no disk).
- `cargo tree -p respak-host` shows no clap.

## Built
Built as specified, without the sketch gate (the user asked for the whole web UI to be done without questions).
- `crates/respak-host`: `config` (model, lookup, `ConfigSelect`, `load`, `load_file`, `save`), `settings` (`Overrides` instead of clap types, `Settings::resolve`), `setup` (`open_input` now takes the `Events` to publish on instead of printing to stderr itself; `build_engine_with` for a different output), `size`. Features: `steam` (default).
- Saving keeps comments, layout and key order: `toml_edit` merges the new values into the existing document, and keys that are gone are removed.
- `respak-cli` keeps clap, the commands, printing and Ctrl-C; `Cli::overrides()` and `Cli::load_config()` bridge to the host. The old settings tests live on as `resolve_tests.rs` and pass unchanged in substance. `cargo build -p respak-cli --no-default-features` builds without Steam.
- Added tests: config round trip, comments kept, removed keys dropped, missing profile.
