# 09 · Core: volumes and virtual paths
**Status:** todo · **Depends on:** 04, 07 · **See:** PROJECT.md §6

## Goal
Containers as OpenDAL operators, and virtual paths that go into them.

## Scope (`src/volume/`)
- `trait VolumeSource` (read-only: `stat`, ranged `read`, `list`) and the generic adapter turning it into an `opendal::Operator`.
- Virtual path resolution: longest real-file prefix → identify → if `Container`, resolve the rest with `entry(path)`; repeat. Real files win.
- Globs (`globset`): descend into a container only where a segment matches the container file and more segments follow; `**` never enters containers; nothing matched → `NoMatch`.
- Docs: the I/O rules (sizes from `stat`, ranged reads, bounded margin, whole-file reads documented).

## Acceptance
- It compiles. If the OpenDAL check in task 01 showed a custom service isn't practical, stop and ask the user.
