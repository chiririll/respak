# R2 · Core restructuring
**Status:** in-progress · **Risk:** low–medium · **Depends on:** R1

## R2.1 Split `mediator.rs` (M) — done
`mediator.rs` (649 lines) holds the detect/identify/convert pipeline, `Cx` (limits, cancel, cache, link state), the output-writing path (`claim`/`write`/`write_blob`/`copy_blob`) and plan/target passthroughs. `glob.rs` (`Mediator::expand`) and `resolve.rs` (`open`/`mount`/`resolve`) add more `impl Mediator`.
- Make a `mediator/` directory: pipeline, `cx.rs`, `cx/output.rs`, and fold `glob.rs`/`resolve.rs` in as `open`/`expand`. Keep the `lib.rs` re-exports unchanged.

## R2.2 Fix layering core → engine (S, with R2.1) — done
- `resolve.rs:106` calls `crate::engine::list_volume` (`engine/inspect.rs:290`). Move it to `volume.rs`.
- Six `.expect("mount checks this")` calls (`inspect.rs:60,94,256`, `glob.rs:154`, `resolve.rs:105,151,171`): add a typed accessor (e.g. `Mediator::mount_container(file, cx) -> Result<Mounted>`).
- `LinkedValue` lives in `engine` but is used by `mediator.rs:457`: move it to core.

## R2.3 Break up `engine/inspect.rs` (M) — done
Check first that `tests/batch.rs` and `tests/containers.rs` cover ordering and cancellation.
- Extract `parallel_map_ordered(files, f)` from `inspect()` (worker pool inline).
- `describe()`: add `DiscoveryEntry::from_detected`, `apply_meta`, separate `describe_children`.
- `list()` and `inspect()` share the open → mount → `list_volume` prologue; use the R2.2 accessor.

## R2.4 Split request and report types (S)
`engine/request.rs` mixes request DTOs with report DTOs (`ConvertReport`, `OutputReport`, `LinkedValue`, `OutputResult`). Split into `request.rs` and `report.rs`.

## R2.5 Request constructors (S)
Add `ConvertRequest::new(outputs)` (removes the hand-set `version: 1` in `convert.rs` and `copy.rs`) and `TargetSpec::file()`. Add a core option on `OutputRequest` (e.g. `skip_unsupported`) so the CLI stops matching `ErrorKind::NoPath { to == "file" }` strings in `output::skip_assets`.
