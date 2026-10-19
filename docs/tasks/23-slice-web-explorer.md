# 23 · Slice: web server, settings and explorer
**Status:** in-progress (built; demo pending) · **Depends on:** 21 · **See:** [webui.md](../design/webui.md) §1-§6, §8

## Goal
`respak-web` starts, opens the browser, lets the user pick a **local folder**, and shows the file tree with search. Nothing converts yet.
```
cargo run -p respak-web             # prints the URL, opens the browser
```
Settings page → Connect → tree (directories, containers expand into assets) → file-name search.

## Changes
1. New crate `respak-web` (binary; **not** a default workspace member, so `cargo build` needs no Node). axum on the existing tokio.
2. Frontend in `web/`: Svelte 5 + Vite + TypeScript, no SvelteKit. `build.rs` / `rust-embed` embeds `web/dist`; a clear error when `web/dist` is missing. Dev: Vite proxy to a running `respak-web`.
3. Session: one `Engine` built through `respak-host` from the saved settings; reconnect cancels running work first.
4. API: `GET/PUT /api/config`, `POST/DELETE /api/session`, `GET /api/tree?path=`, `GET /api/search?q=` (scope: files), `GET /api/entry?path=`. Paths are virtual paths as in the CLI.
5. Tree: lazy, one level per request, paged; containers mount only when expanded. Pak views: path view and kind view (webui.md §6).
6. Search: scope *files* (disk names) and scope *loaded containers* (assets of containers opened this session).
7. Security (webui.md §8): `127.0.0.1` only, random free port (`--port`, `--no-open`), `Host` / `Origin` / `Sec-Fetch-Site` checks, JSON-only state-changing endpoints, no CORS.
8. Settings page writes `respak.toml` through `respak-host`. Steam fields are shown but connecting to Steam waits for slice 26 (manual `[steam]` in the file works through the host crate).

## Leaves out
Previews, convert, jobs, Steam login UI, version catalog, deep search, dark theme polish.

## Open
- Port/launch flags (`--port`, `--no-open`) are a proposal.
- Where the UI keeps its own state (last folders): browser storage or a small file.
- Directory browsing for the local-folder field (server-side listing) or text field only.

## Proposed tests (for the gate)
- Server: `Host` header of another name → 403; foreign `Origin` → 403; `POST` with a non-JSON content type → 415; no CORS headers in responses.
- Tree: lazy listing of a synthetic root with a synthetic rpak; a container is not mounted by listing its parent; paging at the limit.
- Search files finds by substring, case-insensitively; loaded-container scope finds only expanded paks.
- Config: `PUT` then file on disk equals the model; unknown key → 400.
- Frontend: component tests for the tree (virtualization) and the settings form; one end-to-end check with the browser against a synthetic root.

## Built
`crates/respak-web` (not a default member) + `web/` (Svelte 5, Vite, TypeScript, no SvelteKit; embedded with `rust-embed`, `build.rs` stops with a clear message when `web/dist` is missing).
- Server: `GET/PUT /api/config`, `GET/POST/DELETE /api/session`, `GET /api/tree`, `GET /api/search`, `GET /api/entry`, `GET /api/fs`, `GET /api/events` (SSE), static files. Security as in webui.md §8 (`Host`, `Origin`, `Sec-Fetch-Site`, JSON-only state changes, no CORS), `--port`, `--no-open`, `--dir`.
- Tree: lazy per level, paged (500 by default, 2000 at most), container listing cached per session; Paths / Kinds / Names views. A container's directory structure is made from the names it lists. Folders cannot be ticked in a pak whose names were read (their paths would not match the entries' real paths).
- Search: scopes files / opened paks / both; the file index is built on the first search and kept.
- Open questions decided: the local-folder field has a server-side folder browser (`/api/fs`); the UI's own state is only the theme and the sidebar width, in `localStorage`; profiles are not switchable.
- Tests: `crates/respak-web/tests/web.rs` (guards, config, lazy tree, paging, search, a texture preview, jobs, downloads, frontend served).
