# R6 · Housekeeping
**Status:** done (typestate builder and `lock()` helper left as low-priority backlog) · **Risk:** low · Any time.

- Move `png`, `ddsfile`, `bcdec_rs`, `tempfile` into `[workspace.dependencies]` (also used by `integration/Cargo.toml`).
- Remove the redundant `tokio`/`serde_json` dev-dependencies in `respak-formats/Cargo.toml`; consider adding tokio's `time` feature at workspace level.
- Check whether `respak-nihav` needs `respak-formats` as a normal dependency or only as a dev-dependency.
- Replace `_object_safe` (`capability/mod.rs:38`) and `_assert_bounds` (`error.rs:227`) with `const _: fn() = ..` or `#[cfg(test)]` checks; drop their `allow(dead_code)`.
- Check `resolve.rs` `Mounts::len/is_empty` for non-test use.
- `plugin.rs:246,252` `.expect("cost() must follow from_cap() or to()")`: consider a typestate builder. Low priority.
- `Mutex::lock().unwrap()` in `mediator.rs` and `middleware.rs` (~10 places): a small `lock()` helper. Low priority.
- Document in the `Plugin` docs that `PluginCache` swallows errors into events by design.
