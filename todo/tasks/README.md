# Tasks

Read [`../PROJECT.md`](../PROJECT.md) first. It holds the full design and the working rules.

Status values: `todo` · `in-progress` · `blocked` (waiting on the user or another task) · `done`. Update the status in the task file and in this table.

## Phase 1: interfaces only (compiles, `todo!()` bodies, no tests)
| # | Task | Depends on | Status |
|---|---|---|---|
| 01 | [Workspace setup](01-workspace-setup.md) | — | todo |
| 02 | [Core: errors](02-core-errors.md) | 01 | todo |
| 03 | [Core: item model & ids](03-core-item-model.md) | 02 | todo |
| 04 | [Core: events](04-core-events.md) | 03 | todo |
| 05 | [Core: mediator, handlers, plugins, Cx](05-core-mediator.md) | 02–04 | todo |
| 06 | [Core: middleware](06-core-middleware.md) | 05 | todo |
| 07 | [Core: intermediate types](07-core-intermediate-types.md) | 02 | todo |
| 08 | [Core: request types & OpenedInput](08-core-requests.md) | 03, 05, 07 | todo |
| 09 | [Core: typed selector/target helpers](09-core-typed-helpers.md) | 08 | todo |
| 10 | [Core: plugin cache](10-core-plugin-cache.md) | 05 | todo |
| 11 | [Core: Engine & JSON API](11-core-engine-api.md) | 05–10 | todo |
| 12 | [Plugin skeleton: respak-rpak](12-rpak-skeleton.md) | 11 | todo |
| 13 | [Plugin skeleton: respak-image](13-image-skeleton.md) | 11 | todo |
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
| 18 | [Implement mediator & middleware](18-impl-mediator.md) | 17 | blocked |
| 19 | [Implement Engine, cache view & CLI](19-impl-engine-cli.md) | 18 | blocked |
| 20 | [RPak format research](20-rpak-research.md) | 17 | blocked |
| 21 | [Implement RPak container](21-impl-rpak-container.md) | 19, 20 | blocked |
| 22 | [Implement texture decoder (txtr)](22-impl-txtr-decoder.md) | 21 | blocked |
| 23 | [Implement PNG/DDS encoders](23-impl-image-encoders.md) | 19 | blocked |
| 24 | [End-to-end: rpak texture → PNG/DDS via CLI](24-e2e-texture.md) | 22, 23 | blocked |

## Backlog (later, each starts with its own interface/test-definition pass)
| # | Task | Status |
|---|---|---|
| 25 | [More RPak kinds & output formats](25-backlog-formats.md) | blocked |
| 26 | [Open questions to resolve with the user](26-open-questions.md) | blocked |
