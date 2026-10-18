# 25 · Slice: web convert, basket and jobs
**Status:** todo (spec done; sketch and gate pending) · **Depends on:** 22, 24 · **See:** [webui.md](../design/webui.md) §2 step 4, §4, §7 (options forms), §10

## Goal
Tick entries in the tree into a basket, pick a target, set options in a generated form, convert. Several jobs run at once; each shows progress and can be cancelled. Results are written to the output directory and offered for download.

## Changes
1. Basket: entries and folders/containers (a folder is one row standing for a glob per chosen kind, webui.md §6). Survives navigation.
2. `POST /api/convert` takes a request in the `ConvertRequest` shape (project.md §8) built from the basket; returns a job id. Jobs are independent, each with its own `CancellationToken`; the engine's `max_concurrency` bounds the work.
3. `GET /api/events` (SSE): job progress from `Event::Progress` / `RequestFinished` / `RequestFailed`, plus `OutputWritten`. `DELETE /api/jobs/{id}` cancels.
4. `GET /api/output/…`: download a written file; confined to the output directory.
5. Frontend: convert dialog with a form generated from the target schema (slice 22), jobs panel (progress, per-output errors, cancel), download links.
6. One failed output never stops the others; the report is shown per output.

## Leaves out
Writing Respawn formats. A job history across restarts. Compare / diff of outputs.

## Open
- Does a finished job stay in the panel until dismissed, and for how long.
- Download of many outputs: individual links, or a zip **(?)**.

## Proposed tests (for the gate)
- A basket of a glob and a single file produces the expected `ConvertRequest`.
- Two jobs run concurrently; cancelling one leaves the other finishing; a cancelled job reports `Cancelled`.
- SSE: events arrive in order and a lagging client does not stop the job.
- Output download: a path outside the output directory (`..`, absolute) → 404.
- A failing output appears as an error entry while the rest succeed.
- Form rendering from a schema: defaults shown, invalid value blocks submit (frontend test).
