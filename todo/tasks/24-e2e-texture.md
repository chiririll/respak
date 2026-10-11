# 24 · End-to-end: rpak texture → PNG/DDS via CLI
**Status:** blocked · **Depends on:** 22, 23

## Goal
Run the first full vertical slice through the CLI:
- `respak discover <pak>`
- `respak convert <pak> --select '{"asset":"texture/..."}' --target '{"type":"png"}'`
- `respak run request.json` with several outputs

## Checks
- An end-to-end test runs on a synthetic pak in `services::Memory`.
- A manual check on a real game pak is **done by the user**, because game files aren't in the repo.
- The byte-count check shows that the starpak was not fully read.
