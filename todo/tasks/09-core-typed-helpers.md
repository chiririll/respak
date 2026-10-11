# 09 · Core: typed selector/target helpers
**Status:** todo · **Depends on:** 08 · **See:** PROJECT.md §6 (Typed helpers)

## Goal
Plugin authors should work with their own Rust structs, while the core only sees JSON `Value`s.

## Scope (`src/typed.rs`)
- `TypedSelector<T: Serialize + DeserializeOwned>`: parses `Value` into `T`, giving `InvalidSelector` on failure, and serializes back into the item `select` JSON for discovery.
- `TypedTarget<T: Serialize + DeserializeOwned + Default>`: reads the `type` tag, parses the options into `T` (missing fields fall back to `Default`), gives `InvalidOptions` on failure, and produces `defaults()` for `HandlerInfo`.
- Doc examples:
  - an `RpakSelector` enum: `{asset}` | `{guid}` | `{kind}`
  - `PngOptions { mips: Highest | All }`
- The analogy worth stating in the docs: this is like the .NET pattern where a generic `IHandler<TSelector>` is adapted to a non-generic `IHandler`.

## Acceptance
- It compiles, and the doc examples type-check (as `no_run` or `ignore` where they need runtime).
