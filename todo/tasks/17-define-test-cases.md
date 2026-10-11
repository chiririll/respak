# 17 · Define test cases with the user
**Status:** blocked (waits for 16) · **Depends on:** 16

## Goal
Agree the TDD test suite **together with the user** before any implementation. Don't write the suite alone.

## How
- Propose test case lists per area. Candidate areas, not a decided list:
  - **Detection:** extension first, then fallback; highest score wins; tie → `AmbiguousFormat`; unknown variant → `UnsupportedVersion`; header prefix read once.
  - **Path search:** cheapest path, tie-break + warning, `via`, capability edges, `NoPath`.
  - **Middleware:** cancellation; cycle guard (same chain, cross-chain wait-for); dedup of concurrent requests; events.
  - **Output:** written-paths map (same key skipped, `OutputConflict`), composite folders, default paths.
  - **Volumes & paths:** nested resolution, real files win, glob rules, typed entries.
  - **Engine:** one failing output doesn't stop the others; discovery round-trips into a request; bundle rule; nested requests don't deadlock under `max_concurrency`.
  - **Cache:** absent cache works, errors are warnings, namespacing by id + version.
  - **CLI:** commands in-process against `services::Memory`; exit codes.
  - **I/O rule:** byte-counting `Operator` layer on a synthetic multi-GB starpak.
- Fixtures: synthetic data generated in the tests. **No game files in the repo.**
- Write the agreed list into each implementation task (18–25) under "Tests".

## Done when
The user approves the test lists.
