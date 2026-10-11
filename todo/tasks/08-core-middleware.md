# 08 · Core: middleware
**Status:** todo · **Depends on:** 07 · **See:** PROJECT.md §5.6

## Scope (`src/middleware/`)
- `trait Middleware { async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value>; }`
- Built-ins, in order: cancellation, cycle guard (call chain + wait-for graph), dedup (concurrent requests → `Shared` future; volumes retained for the `Engine` call), tracing span, events.
- Docs: no per-request concurrency limit and why; dedup only merges concurrent requests.

## Acceptance
- It compiles; the built-in order is documented and fixed.
