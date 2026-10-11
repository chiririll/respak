# 12 · Plugin skeleton: respak-rpak
**Status:** todo · **Depends on:** 11 · **See:** PROJECT.md §3, §5 (Plugin), §6

## Goal
Show how an input plugin plugs into the core. This is a review aid only: no parsing, and bodies are `todo!()`.

## Scope
- `RpakPlugin: Plugin`, with options such as `.kinds([...])` to enable a subset. `register` registers:
  - `RpakOpen: Handler<OpenInput>`, where `can_handle` checks the `.rpak` extension and/or a header probe
  - handlers for `Discover`, `Select`, `Locate` and `ReadItem` on opened rpaks
  - `TextureDecoder: Handler<Decode>` with `kind = "txtr"`, a version range, and `produces = Texture::TYPE_NAME`
- `RpakSelector` (serde, internally tagged or untagged, to be decided in review):
  - `{ "asset": "<path>" }`: the path is hashed to a GUID, so it works without a name table
  - `{ "guid": "0x…" }`
  - `{ "kind": "txtr" }`
- `RpakInput: OpenedInput` (stub). Its doc comment states that a GUID is a hash of the asset path, that a dependency is referenced by GUID only and resolved through `Locate`, and how companion files (starpaks, `patch_master.rpak`, `name(NN).rpak`) are found through the input `Operator`.
- **Depends only on `respak-core`.**

## Acceptance
- It compiles, and `RpakPlugin` registers with an `Engine` builder.
