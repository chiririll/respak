# 13 · Slice: linking, a material with its textures
**Status:** blocked (output format to decide with the user) · **Depends on:** 12 · **See:** PROJECT.md §5.5, §5.6 (built-ins 2, 3), §6 (GUID lookup), §7, §12

## Goal
Exporting a material also exports its textures, which may live in other paks, into the material's own folder. This is the first converter that asks the mediator for other values.

## rsx review
(filled in during the sketch, approved at the gate)

## Scope
Material parsing is ported from rsx (`assets/material.{h,cpp}`, all versions and variants). Review it first, then port it (PROJECT.md §12).

**respak-respawn:**
- `matl`: read from `rpak.asset`, `Metadata` (name, guid, dependencies).
- `rpak.guid → rpak.asset` (`RpakLocate`) with the §6 search order; `RespawnPlugin::new().search([...])`.
- A material output that converts its textures through the mediator and writes them via `cx.write` into the composite folder. **The output format (JSON? something else?) is the user's call.**

**Core:**
- Dedup middleware: concurrent requests for the same (key, target) share one `Shared` future.
- Cycle guard: the call chain plus a wait-for graph → `DependencyCycle`.
- Plugin cache (`cx.cache()`, namespaced by plugin id + `cache_version`), first used for the GUID → file index. Cache errors become warnings.
- `dependencies` in the report.

**CLI:** `--search <glob>...`, `--cache <dir>`.

## Demo
The user exports a level material whose textures live in `common.rpak`. Two materials sharing a texture each write their own copy, and that texture is decoded once per run.
