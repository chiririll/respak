# 08 · Core: request types & OpenedInput
**Status:** todo · **Depends on:** 03, 05, 07 · **See:** PROJECT.md §6

## Goal
Define the standard request types that make up the pipeline: open → select → read → decode → encode.

## Scope (`src/requests.rs`)
Each request implements `Request` with a documented `key()`:

| Request | Response |
|---|---|
| `OpenInput { path, format: Option<String> }` | `Arc<dyn OpenedInput>` |
| `Discover { input: String }` | `Vec<DiscoveredItem>` (the engine fills in `targets`) |
| `Select { input: String, selector: Value }` | `Vec<ItemRef>` |
| `Locate { from: ItemRef, key: String }` | `ItemRef` (or the error `MissingDependency`) |
| `ReadItem { item: ItemRef }` | `RawItem` (with lazy `ItemData`) |
| `Decode { item: ItemRef }` | `Decoded` |
| `Encode { item: ItemRef, target: Value, path: Option<String> }` | `Vec<String>` (written paths) |

- `trait OpenedInput: Send + Sync` with:
  - `format_id()`
  - `path()`
  - `source_files() -> &[String]`, which lists every file pulled in (rpak + starpaks + patches)
  - `items()`
  - `suggested_path(&ItemRef) -> String`
- Docs per request:
  - Who handles it and how it is routed: by format, by (kind, version), or by target `type`.
  - `Locate` semantics: the owning plugin checks its own file, then its cache, then opens other files **lazily, only when the current request needs them**.
  - The large-file rule (PROJECT.md §7).
  - Linking: encoders send `Encode` for the dependencies they need. There is no `with_dependencies` flag.

## Acceptance
- It compiles and is documented.
