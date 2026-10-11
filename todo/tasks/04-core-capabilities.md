# 04 · Core: capabilities
**Status:** todo · **Depends on:** 03 · **See:** PROJECT.md §5.4

## Goal
The core traits through which formats of different crates talk to each other.

## Scope (`src/capability/`)
- `Blob`: in-memory bytes.
- `Container`: `volume() -> Operator`, `entry(path) -> Result<Value>` with a default returning `file { volume, path }`. Document typed entries (the rpak example).
- `Metadata`: name, guid, dependencies.
- `Bitmap`: dimensions, depth, array/faces, mip count, pixel format (enum incl. BC formats), `read_mip(level, layer)`.
- `Mesh`, `Skeleton`, `Animation`, `Table`, `Audio`: skeletal traits, marked as to be refined.
- Owned helper `BitmapBuf` for converters that create data.

## Acceptance
- It compiles; traits are object-safe; docs say capabilities are lazy views over the native type.
