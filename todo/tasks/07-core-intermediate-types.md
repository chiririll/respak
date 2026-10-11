# 07 · Core: intermediate types
**Status:** todo · **Depends on:** 02 · **See:** PROJECT.md §6

## Goal
Define the open set of decoded "intermediate" data. Decoders produce it and encoders consume it. This set is what keeps input plugins and output plugins independent of each other.

## Scope (`src/types/`)
- `trait Intermediate: Any + Send + Sync + Debug { fn type_name(&self) -> &'static str; }`, plus a downcast helper.
- `type Decoded = Arc<dyn Intermediate>`.
- **Skeletal** standard types. Use only obvious fields, mark them `#[non_exhaustive]`, and note that the fields will be refined with the first real decoders and encoders:
  - `Texture { width, height, depth, array_size, mips: Vec<Mip>, format: PixelFormat, … }`
  - `Model`, `Material`, `Animation`, `Rig`, `Audio`, `Table`, `Text`, `Binary`
- Each type has a `TYPE_NAME` constant that is used in `HandlerInfo::produces` and `accepts`.
- Document that plugins may define their own `Intermediate` types.

## Acceptance
- It compiles and is documented. There is no format-specific logic in core.
