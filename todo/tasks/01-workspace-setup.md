# 01 · Workspace setup
**Status:** todo · **Depends on:** — · **See:** PROJECT.md §3, §4

## Goal
Create the Cargo workspace and the four crates, with dependencies declared once at workspace level.

## Scope
- Root `Cargo.toml`: `[workspace]` with `members = ["crates/*"]` and `resolver = "2"`. Put shared versions in `[workspace.dependencies]`:
  - `opendal` (**exact version pin**, features `services-fs`; the Memory service is always available)
  - `tokio` (`rt-multi-thread`, `macros`, `sync`, `fs`)
  - `tokio-util`
  - `futures`
  - `async-trait`
  - `serde` (`derive`)
  - `serde_json`
  - `thiserror`
  - `tracing`
  - `bytes`
  - `clap` (`derive`)
  - `anyhow`
- Crates:
  - `crates/respak-core` (lib)
  - `crates/respak-rpak` (lib, depends only on core)
  - `crates/respak-image` (lib, depends only on core)
  - `crates/respak-cli` (bin + lib, depends on core, rpak, image)
- `rust-toolchain.toml` set to stable. `.gitignore` covering `target/`.
- Workspace lints: `[workspace.lints]` with `missing_docs = "warn"` for the library crates.
- A root `README.md` with a short description that points to `todo/PROJECT.md`.

## Before starting
- Check the latest stable OpenDAL release on crates.io/docs.rs and confirm these APIs exist under these names: `Operator`, `read_with().range()`, `reader`, `stat`, `list`, `write`, `services::Fs`, `services::Memory`. Write the pinned version and anything that differs into PROJECT.md §4.

## Acceptance
- `cargo build --workspace` succeeds with the empty crates.
- No crate depends on another plugin crate.
