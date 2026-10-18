# 22 · Slice: target option schemas
**Status:** todo (spec done; sketch and gate pending) · **Depends on:** nothing (can run in parallel with 21) · **See:** [webui.md](../design/webui.md) §7, project.md §8 `Discovery`, §11 open question (`schemars`?)

## Goal
Every target a plugin offers describes its options as a JSON Schema, so a UI can render a form without knowing the format.
```
respak formats --schemas          # or: discover shows `schema` for each target
```
`Discovery.entries[].targets[]` gains `schema` next to `defaults`.

## Changes
1. Decide how schemas are made: `schemars` derive on each plugin's option struct, or hand-written. Prefer `schemars` (**(?)** user decides at the gate).
2. Core: `TypedTarget<T>` options types expose a schema; the registry stores it with the target; `Discovery` serializes it. Core stays free of format knowledge.
3. Plugins: add the schema to every existing target (`png`, `dds`, `wav`, `cast`, `avi`/`mov`/`y4m`, `png.frames`, the JSON targets, `file`).
4. The schema must agree with what parsing accepts: unknown options stay an `InvalidOptions` error.

## Leaves out
Any UI. Validation messages beyond what parsing already gives.

## Open
- `schemars` or hand-written; which version, and the licence check.
- Where defaults live: in the schema (`default`) or only in `defaults`.

## Proposed tests (for the gate)
- Per target: the schema of the default options validates the defaults.
- Per target: an option the struct rejects is also rejected by the schema (and the reverse for a sample of valid values).
- `Discovery` JSON for a texture contains `schema` for each target.
- No plugin-to-plugin dependency appears in `cargo tree`.
