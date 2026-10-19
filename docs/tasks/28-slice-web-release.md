# 28 · Slice: web release packaging
**Status:** in-progress (workflow edited; not run) · **Depends on:** 23 (earlier is fine) · **See:** [webui.md](../design/webui.md) §3, §12

## Goal
CI builds the frontend and the `respak-web` binary for Windows x64 and Linux x64, with the frontend embedded, and attaches them to a release alongside the CLI.

## Changes
1. CI job: Node + `npm ci` + `npm run build` in `web/`, then `cargo build --release -p respak-web`.
2. Fail with a clear message when `web/dist` is missing in a local build.
3. Archive names and contents (binary, shipped catalogs if not embedded, licence files, README for the UI) **(?)**.
4. Document `respak-web` in the README and a `docs/web.md` user guide (settings, explorer, convert, Steam login, catalog).

## Leaves out
macOS builds. Installers. Auto-update.

## Open
- Archive naming and whether the CLI and web ship together.
- Whether the AGPL notice has to be shown in the UI (AGPL-3.0 network-use clause applies to a local server; check).

## Proposed tests (for the gate)
- CI: the release job produces a binary that serves `/` and `/api/config` on a smoke run.
- `cargo build` (default members) still needs no Node.

## Built
`.github/workflows/release.yml` now sets up Node, builds `web/`, builds `respak-cli` and `respak-web` for Windows x64 and Linux x64, smoke-tests the Linux server (`/`, `/api/info`, `/api/config`) and ships both binaries in the one archive per platform (`respak-<tag>-<platform>`). `docs/web.md` is the user guide and the README links to it. `cargo build` (default members) still needs no Node.
- Open: whether the AGPL notice has to appear in the UI.
