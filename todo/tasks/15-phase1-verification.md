# 15 · Phase 1 verification & handoff
**Status:** todo · **Depends on:** 01–14

## Checklist
- `cargo build --workspace` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo doc --workspace --no-deps` shows no missing docs on public items.
- `cargo run -p respak-cli -- --help` and the subcommand `--help` pages work.
- `cargo tree -p respak-respawn` and `cargo tree -p respak-formats` show no plugin-to-plugin dependency.
- Core contains no format knowledge.
- No tests yet. That is intended. No Steam or downloader code anywhere.
- Update `todo/tasks/README.md` statuses.
- **Don't commit unless the user asks.**

## Handoff
Tell the user the interfaces are ready for review via `cargo doc --workspace --no-deps --open`, and list any decisions made during implementation that were not covered by PROJECT.md.
