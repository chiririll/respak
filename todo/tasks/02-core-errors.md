# 02 · Core: errors
**Status:** done · **Depends on:** 01 · **See:** PROJECT.md §9

## Goal
One shared, cloneable error type for the whole library.

## Scope (`src/error.rs`)
- `Error(Arc<ErrorKind>)`: `Clone`, `Display`, `std::error::Error`; `pub type Result<T> = std::result::Result<T, Error>`.
- `#[non_exhaustive] enum ErrorKind` with exactly the variants in PROJECT.md §9, each with a doc comment saying when it is used.
- Serialization as `{ "kind": "...", "message": "...", ...fields }` for reports.
- `From<opendal::Error>`.

## Acceptance
- It compiles; `Error` is `Clone + Send + Sync + 'static`.
