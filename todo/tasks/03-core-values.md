# 03 · Core: values, formats, keys
**Status:** todo · **Depends on:** 02 · **See:** PROJECT.md §5.1

## Goal
The value model: everything is a value tagged with a format.

## Scope (`src/value/`)
- `pub type Value = Arc<dyn Data>`; `trait Data` with `format`, `key`, `approx_size`, the `as_*` capability accessors (default `None`) and `capability(TypeId)`.
- `FormatId { name, version: Option<Version> }`, `Version { major, minor }`. Parse/print `txtr@10` and `txtr@23.2`; serde as that string.
- `ValueKey`: virtual path + optional range for files; source key + target format + normalized options for converted values. Document the normalization (defaults filled in, fields sorted).
- Core formats with their value types: `file { op, path, range }`, `blob`, `bundle`, `compressed { codec, data, decoded_len }`.
- Downcast helper from `Value` to a concrete type.

## Acceptance
- It compiles; `Data` is object-safe; `cargo doc` explains keys and versions (`minor` = layout variant).
