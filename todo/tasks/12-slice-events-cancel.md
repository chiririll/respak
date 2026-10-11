# 12 · Slice: events, progress, cancellation
**Status:** blocked · **Depends on:** 11 · **See:** PROJECT.md §5.6 (built-ins 1, 4, 5), §5.7

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
