# 07 · Core: mediator, Cx, output writing
**Status:** todo · **Depends on:** 05, 06 · **See:** PROJECT.md §5.5

## Scope (`src/mediator/`)
- `Mediator::convert(source, Target, &Cx)`, `identify(source, &Cx)`, `publish`, `subscribe`. `Target { format, options: serde_json::Value, via }`. One request type `Convert`; `identify` goes through the same middleware.
- `Cx`: mediator handle, call chain of keys, `CancellationToken`, input/output operators, `cache()`, `write(path, value)`, `blocking(..)`, `io(..)`.
- Output writing: core writes `Blob` values; written-paths map for the whole `Engine` call (same key → skip, different key → `OutputConflict`); composite outputs go into their own folder (documented convention for converters).
- `TypedTarget<T>`: options → plugin struct, missing fields from `Default`, errors → `InvalidOptions`.

## Acceptance
- It compiles; docs explain linking (converters ask the mediator, no `with_dependencies`) and the composite folder rule.
