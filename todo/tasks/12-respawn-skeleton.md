# 12 · Plugin skeleton: respak-respawn
**Status:** todo · **Depends on:** 11 · **See:** PROJECT.md §4, §5.2, §5.4, §6

## Goal
Show how the Respawn plugin plugs into core. Review aid only: no parsing, bodies are `todo!()`.

## Scope
- `RespawnPlugin::new()` with `.search([...globs])`; `register` as in the §5.2 example.
- Formats:
  - `rpak`, `starpak`: read from `file`, provide `Container`
  - `rpak.asset`: typed entry (kind, version, header size, guid, header / mem / stream / opt files)
  - `rpak.guid`: `→ rpak.asset` via `RpakLocate`, with the §6 search order
  - `txtr`: read from `rpak.asset`, provides `Bitmap` + `Metadata`
- Codec edges `compressed → blob` for RTech, Snowflake, Oodle (stubs).
- `RpakVolume: VolumeSource` stub; its doc comment describes the §6 layout (named + GUID paths, `.stream`, `.opt`, `.mem`, pointer relocation).
- **Depends only on `respak-core`.**

## Acceptance
- It compiles, and `RespawnPlugin` registers with an `Engine` builder.
