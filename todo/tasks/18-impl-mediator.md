# 18 · Implement mediator & middleware (TDD)
**Status:** blocked (waits for 17) · **Depends on:** 17 · **See:** tasks 05, 06

## Goal
Implement the mediator and the built-in middleware, test-first, against the test cases agreed in task 17.

## Tests
_To be filled in by task 17._

## Notes
- Dedup uses `futures::future::Shared`. Remove the entry on completion.
- The wait-for graph for cross-chain cycle detection must be cheap: record "A waits for B" only when a request joins an in-flight future.
- Don't apply a semaphore per request. It deadlocks with nesting (PROJECT.md §5).
