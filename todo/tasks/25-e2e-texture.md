# 25 · End-to-end: rpak texture → PNG/DDS via CLI
**Status:** blocked · **Depends on:** 23, 24

## Goal
Run the first full vertical slice through the CLI:
- `respak discover paks/Win64/common.rpak`
- `respak convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'`
- `respak run request.json` with several outputs, including a glob

## Checks
- An end-to-end test runs on a synthetic pak in `services::Memory`.
- A manual check on a real game pak is **done by the user**, because game files aren't in the repo.
- The byte-count check shows that the starpak was not fully read.
