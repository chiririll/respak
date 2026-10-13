# 12 · Slice: events, progress, cancellation
**Status:** in-progress (built; waiting for the user's demo) · **Depends on:** 11 · **See:** PROJECT.md §5.6 (built-ins 1, 4, 5), §5.7

## Goal
During a large `respak run`, stderr shows progress and warnings. Ctrl-C stops cleanly, and the report says what finished.

## Scope
**Core:**
- `Event` (§5.7), the broadcast channel with `event_capacity`, `LogSubscriber`, `Engine::events()`, `Mediator::publish` / `subscribe`. The tie-break and missing-mip warnings from earlier slices become `Warning` events.
- The middleware trait and chain, with the first built-ins: cancellation, a tracing span, events (started / finished / failed). Hosts can add middleware.
- `CancellationToken` in `Cx` and on `Engine` calls; long handlers check it.
- `cx.blocking(..)` / `cx.io(..)` with `blocking_limit` / `io_limit`.
- `InputOpened`, `OutputWritten`, `Progress`.

**CLI:** events to stderr; Ctrl-C → cancel.

## Demo
The user runs a large glob, watches the progress and presses Ctrl-C halfway.

## Built (differences from the sketch)
- Middleware wraps `Mediator::convert` only; `identify` is not wrapped yet. The cycle guard and dedup built-ins are not part of this slice.
- `Engine::{discover, convert, open}` take `&CancellationToken`; `CancellationToken` and `broadcast` are re-exported from core.
- Warnings that used to be `tracing::warn!` are `Event::Warning` (tie-break, skipped optional mips, png layers, "cannot describe", "cannot look inside"). Without a subscriber they are silent; the CLI prints them.
- CLI: `run` takes the token; `run_with_events` prints events (`warning:`, `[done/total]`, `opened`, `wrote`) to stderr; first Ctrl-C cancels, second exits with 130.
- Test 10 triggers the token between two reads of an Oodle body (not inside one segment's decode).
- Rtech and Oodle bodies carry the call's token from `ReadHandler::read`; PNG encoding runs on `cx.blocking`.

## Sketch (compiles, `todo!()` bodies)
In `respak-core`: `event.rs` (`Event`, `Events`, `LogSubscriber`), `middleware.rs` (`Convert`, `Next`, `Middleware`, three built-ins), and in `mediator.rs`: `Mediator::{publish, subscribe, with_events, with_middleware}`, `Cx::{cancel, with_cancel, with_limits, check_cancelled, warn, blocking, io}`. `Cx::new` and `Mediator::new` keep their signatures. Core now depends on `tokio` and `tokio-util` at runtime.

Not yet sketched (they change existing signatures; done in the build step): `Engine::{discover, convert, open}` take `&CancellationToken` (PROJECT.md §8); `EngineBuilder::{middleware, blocking_limit, io_limit, event_capacity}`; `Engine::events()`; CLI `signal` feature, stderr event printer, Ctrl-C.

## Tests (agreed)
Core:
1. `Events`: a subscriber receives published events in order; publishing with no subscriber does not fail; a lagging subscriber gets `Lagged`, not a panic (`event_capacity` 2).
2. Middleware chain: a host middleware runs, sees the request and can short-circuit; built-ins run before host middleware.
3. Cancellation middleware: a token triggered before `convert` gives `Cancelled` without calling any handler.
4. Events middleware: a successful convert publishes `RequestStarted` then `RequestFinished`; a failing one publishes `RequestFailed`.
5. `Cx::blocking` never runs more than `blocking_limit` closures at once (counter in the closures); `Cx::io` the same for `io_limit`.
6. The tie-break warning is published as `Event::Warning` (from `convert` and from `plan`).
7. `Engine::convert` with a token triggered during the run: finished outputs stay `Ok`, the rest are `Err(Cancelled)`, the call itself returns `Ok`.
8. `Engine` emits `OutputWritten` per file and `Progress{done,total}` per output; a container mount emits `InputOpened`.

Respawn / formats:
9. Missing optional mips publish `Warning` (through `cx.warn`).
10. The rpak body decode loop returns `Cancelled` when the token is triggered mid-decode (synthetic multi-chunk pak).
11. PNG `mips: "all"` stops between mips on cancel and writes no partial file.

CLI:
12. `run` with a pre-triggered token reports every output as `Cancelled` and exits non-zero.
13. Events are printed to the error stream (progress and warnings) for a small `run`.
(Ctrl-C itself is checked in the demo, not in a test.)
