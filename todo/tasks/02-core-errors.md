# 02 · Core: errors
**Status:** todo · **Depends on:** 01 · **See:** PROJECT.md §10, §5 (results shared via dedup)

## Goal
Define `respak_core::Error` and `Result<T>`.

## Scope
- `src/error.rs`. `ErrorKind` is a `thiserror` enum with these variants:
  - `NoHandler { request: String }`
  - `NotFound { path: String }`
  - `Storage(opendal::Error)`
  - `UnsupportedVersion { kind: String, version: u32 }`
  - `InvalidSelector { reason }`
  - `NoMatch`
  - `UnknownTarget { type_id }`
  - `TargetNotApplicable { type_id, intermediate }`
  - `InvalidOptions { reason }`
  - `MissingDependency { item, file: Option<String> }`
  - `DependencyCycle { chain: Vec<String> }`
  - `Corrupt { context }`
  - `Cancelled`
  - `Custom { source: String, message: String }` (for plugins)
- `Error` is a cheap-clone wrapper, e.g. `Error(Arc<ErrorKind>)`. It is needed because dedup hands one result to several waiters, and `opendal::Error` is not `Clone`.
- `Error` implements `Serialize` (as kind plus message), because it appears in the JSON `ConvertReport`.
- Doc comments on every variant say when it is produced.

## Acceptance
- The crate compiles. `Error: Clone + Send + Sync + std::error::Error`.
