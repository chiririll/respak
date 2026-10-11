# Tasks

Read [`../PROJECT.md`](../PROJECT.md) first. It holds the full design and the working rules.

Status values: `todo` · `in-progress` · `blocked` (waiting on the user or another task) · `done`. Update the status in the task file and in this table.

## Phase 1: interfaces only (compiles, `todo!()` bodies, no tests)
| # | Task | Depends on | Status |
|---|---|---|---|
| 01 | [Workspace setup](01-workspace-setup.md) | — | done |
| 02 | [Core: errors](02-core-errors.md) | 01 | done |
| 03 | [Core: values, formats, keys](03-core-values.md) | 02 | done |
| 04 | [Core: capabilities](04-core-capabilities.md) | 03 | todo |
| 05 | [Core: events](05-core-events.md) | 03 | todo |
| 06 | [Core: plugins, registry, detection, path search](06-core-registry-detection.md) | 03, 04 | todo |
| 07 | [Core: mediator, Cx, output writing](07-core-mediator.md) | 05, 06 | todo |
| 08 | [Core: middleware](08-core-middleware.md) | 07 | todo |
| 09 | [Core: volumes and virtual paths](09-core-volumes-paths.md) | 04, 07 | todo |
| 10 | [Core: plugin cache](10-core-plugin-cache.md) | 07 | todo |
| 11 | [Core: Engine and JSON API](11-core-engine-api.md) | 05–10 | todo |
| 12 | [Plugin skeleton: respak-respawn](12-respawn-skeleton.md) | 11 | todo |
| 13 | [Plugin skeleton: respak-formats](13-formats-skeleton.md) | 11 | todo |
| 14 | [CLI skeleton](14-cli-skeleton.md) | 11–13 | todo |
| 15 | [Phase 1 verification & handoff](15-phase1-verification.md) | 01–14 | todo |

## Phase 2: user gates
| # | Task | Depends on | Status |
|---|---|---|---|
| 16 | [Interface review with the user](16-interface-review.md) | 15 | blocked |
| 17 | [Define test cases with the user](17-define-test-cases.md) | 16 | blocked |

## Phase 3: implementation (TDD)
| # | Task | Depends on | Status |
|---|---|---|---|
| 18 | [Implement registry, detection, path search, mediator, middleware](18-impl-core.md) | 17 | blocked |
| 19 | [Implement volumes and virtual paths](19-impl-volumes.md) | 18 | blocked |
| 20 | [Implement Engine, plugin cache, CLI](20-impl-engine-cli.md) | 19 | blocked |
| 21 | [RPak format research](21-rpak-research.md) | 17, 27 (first milestone) | blocked |
| 22 | [Implement rpak and starpak containers](22-impl-rpak.md) | 20, 21 | blocked |
| 23 | [Implement texture (txtr)](23-impl-txtr.md) | 22 | blocked |
| 24 | [Implement PNG/DDS](24-impl-png-dds.md) | 20 | blocked |
| 25 | [End-to-end: rpak texture → PNG/DDS via CLI](25-e2e-texture.md) | 23, 24 | blocked |

## Backlog (later, each starts with its own interface/test-definition pass)
| # | Task | Status |
|---|---|---|
| 26 | [More formats](26-backlog-formats.md) | blocked |
| 27 | [Open questions to resolve with the user](27-open-questions.md) | blocked |
