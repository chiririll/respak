# 11 · Slice: batch requests, globs, composite outputs
**Status:** blocked · **Depends on:** 10 · **See:** PROJECT.md §5.5, §6 (globs), §8

## Goal
```
respak run request.json
```
with several outputs, including a glob into a pak (`paks/Win64/*.rpak/**/*.txtr` → dds) and `{"type":"png","mips":"all"}`, producing one report.

## Scope
**Core:**
- The full `ConvertRequest` / `ConvertReport` from §8: `from` as a path, a glob or a list; `{path, format}` overrides detection (loose assets); one report entry per conversion.
- Globs (`globset`) with the container rules: descend only where a segment matches the container file and more segments follow; `**` alone never enters containers; nothing matched → `NoMatch`.
- Outputs run in parallel up to `max_concurrency`; one failure doesn't stop the others.
- The written-paths map for the whole `Engine` call: the same key on the same path is skipped, a different key → `OutputConflict`.
- Composite outputs get their own folder, starting with `mips: "all"`. `cx.write`.
- `Engine::open(path_or_glob)`.

**CLI:** `run <request.json | ->`. `convert` becomes a thin wrapper over the same request. Non-zero exit if any output failed.

## Not in this slice
The bundle rule (no target takes `bundle` until writing Respawn formats is done).

## Demo
The user runs a request over a real pak directory and reads the report.
