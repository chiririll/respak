# 17 · Define test cases with the user
**Status:** blocked (waits for 16) · **Depends on:** 16

## Goal
Agree the TDD test suite **together with the user** before any implementation. Don't write the suite alone.

## How
- Propose test case lists per area. Here are candidate areas to discuss, not a decided list:
  - **Mediator:** routing order, `NoHandler` vs `UnsupportedVersion`, plugin-defined requests.
  - **Middleware:**
    - cancellation
    - cycle guard (same chain, and cross-chain deadlock via the wait-for graph)
    - dedup (concurrent identical requests run once, and the entry is removed after completion)
    - events emitted
  - **Engine:**
    - one failing output doesn't stop the others
    - discovery shapes round-trip into a request
    - default paths
    - overwrite behavior
    - the concurrency limit doesn't deadlock nested requests
  - **Cache:** absent cache works, cache errors are non-fatal, namespacing.
  - **Typed helpers:** invalid selector/options give the right errors, and defaults are filled in.
  - **CLI:** commands driven in-process against `services::Memory`; exit codes.
  - **I/O rule:** reads are ranged; a test `Operator` layer can count the bytes read, so a large-file read can be detected.
- Fixtures: synthetic data generated in the tests. **No game files in the repo.**
- Write the agreed list into each implementation task (18–24) under "Tests".

## Done when
The user approves the test lists.
