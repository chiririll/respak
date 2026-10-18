# respak — web UI

A browser UI for respak: choose where the game files come from (Steam or a local folder), browse every file as a tree with search, open files in a viewer, and convert them. It runs as `respak serve` and is a thin layer over the same `Engine` the CLI uses.

**Status:** draft, after the first round of questions. Items marked **(?)** are open and are collected in §12. This document is the target design; it follows the slice workflow of [project.md](project.md) §1 and is built only as slices need it.

---

## 1. Rules
- **The UI is a host, like the CLI.** It builds the `Engine`, registers plugins and talks to it through the public API (project.md §8). No format knowledge, no conversion logic and no Steam protocol code in the web crate.
- **Local only.** The server listens on `127.0.0.1` and is protected against other web pages (§8). It is a personal tool, not a hosted service.
- **Same configuration as the CLI.** Settings read and write the same `respak.toml` ([cli.md](../cli.md#config-file-and-profiles)), found the way the CLI finds it (working directory, else the nearest parent); there is no second config format. Profile switching in the UI is not in scope **(?)**.
- **Same I/O rules** (project.md §6): sizes from `stat`, ranged reads only, nothing downloaded that a request doesn't need. Listing a tree or previewing a texture must not pull a 60 GB starpak.
- **Read-only.** The UI never writes Respawn formats and never modifies the input.
- **One binary.** The frontend is built once and embedded; releases stay a single executable.
- **Minimal first slice.** Everything past the flow in §2 is listed in §11 and is not built until asked for.

## 2. User flow
1. **Settings page.** Pick the source:
   - *Local folder:* path to the game directory (`root`).
   - *Steam:* login (QR shown in the page, or username + password with the Steam Guard step: email code, authenticator code, or confirmation in the mobile app), then the game version picked from the **version catalog** (§5a) or entered by hand (app ID, depots as `ID` / `ID=MANIFEST`, branch).
   - Other options of `respak.toml`: output directory, search globs, no-patches, cache location.
   - Save writes the config; Connect builds the `Engine` and opens the explorer.
2. **Explorer.** Tree view of the whole input: directories, then containers (`*.rpak`, `*.starpak`, …) that expand into their assets, and search across names.
3. **Open.** Selecting a file shows a viewer chosen by its detected format, plus its metadata (format, version, score, GUID, dependencies).
4. **Convert.** Entries are ticked with checkboxes anywhere in the tree (decided: multi-select) into a **basket** that survives navigation. Pick a target among those `discover` reports for the basket's entries (or per entry when they differ), set its options, convert. Results are written to the output directory and offered for download; a failed output shows its error and the others still finish.

## 3. Crates and dependencies
```
crates/
  respak-web/     # separate binary: axum server, API, embedded frontend; host code
web/              # Svelte frontend sources (not a Rust crate)
```
`respak-web` is its own executable (decided); the CLI binary stays lean. Code both hosts need (the `respak.toml` model and loading, plugin registration, `Engine` building from a config, Steam operator setup) moves out of `respak-cli` into a new host crate `respak-host` (decided). This is a refactor of the CLI, done as its own slice before the web work; the CLI's behaviour and tests must not change.
```
respak-host/      # config model + loading, plugin registration, Engine builder; no clap, no axum
```
| Purpose | Choice | Note |
|---|---|---|
| HTTP server | `axum` on the existing `tokio` | |
| Frontend | Svelte 5 + Vite + TypeScript, no SvelteKit (?) | static build, no SSR |
| Embedding | `rust-embed` or `include_dir` (?) | built output is embedded |
| Live updates | Server-Sent Events | progress and events are server → client only (?) |

**Frontend build (decided):** CI builds the frontend and the release binaries embed it; the repo holds no built files. Local development runs the Vite dev server against a running `respak-web`. A local `cargo build -p respak-web` needs a one-time `npm run build` in `web/` (the build fails with a clear message if `web/dist` is missing). `cargo build` (default members) must not need Node, so `respak-web` is not a default member **(?)**.

## 4. Architecture
```
Browser (Svelte)  ──HTTP/SSE──▶  respak-web  ──▶  Engine (core + plugins)  ──▶  input Operator (fs | steam)
                                                                          └─▶  output Operator (fs)
```
- One **session** = one `Engine` built from the current settings. Changing the source rebuilds it; running jobs are cancelled first.
- The server holds the session in memory; the UI is stateless apart from what it shows (tree expansion, selection).
- Long work (listing a big pak over Steam, a batch convert) is a **job**: started by a request, reported over SSE, cancellable through its own `CancellationToken`. **Several jobs may run at once** (decided); the UI has a jobs panel with progress and a cancel button per job, and the engine's `max_concurrency` bounds the real work.
- Steam allows one session per account: the session owns the only Steam connection, and all jobs share it.

## 5. API (sketch)
JSON over HTTP, all under `/api`, all requiring the session token (§8).

| Endpoint | Purpose |
|---|---|
| `GET/PUT /api/config` | read / save the settings (the `respak.toml` model) |
| `POST /api/session` , `DELETE /api/session` | connect (build the `Engine`) / disconnect |
| `GET /api/steam/status`, `POST /api/steam/login/qr`, `POST /api/steam/logout` | login state; QR challenge stream over SSE |
| `GET /api/tree?path=…` | one level of the virtual tree: name, kind (dir / container / file), size, format if cheap |
| `GET /api/search?q=…` | search names across the input (§6) |
| `GET /api/entry?path=…` | `discover` for one entry: format, version, score, alternatives, targets |
| `GET /api/preview?path=…` | viewer data (§7) |
| `POST /api/convert` | start a job from a request (project.md §8 `ConvertRequest`) |
| `GET /api/events` | SSE: job progress, `Event`s, Steam download stats |
| `GET /api/output/…` | download a written file |

Paths are virtual paths exactly as in the CLI (`/`, relative to the input, may go through containers).

## 5a. Version catalog (Steam)
Instead of typing depot and manifest IDs, the UI offers named game versions from catalog files (decided direction; details open **(?)**):
- A catalog is a data file listing manifests (taken from SteamDB) with human names, grouped by season or in-game event, e.g. app → group "Season 20" → "Launch patch" → `{ depot: manifest }`. The user can edit catalog files by hand, and releases ship a default set.
- Picking an entry fills `[steam] app / depots / branch` in the config.
- **Format: TOML** (decided), hand-editable, with comments. Shape:
  ```toml
  [apex]
  name = "Apex Legends"
  app_id = 1172470

  [[apex.depots]]
  id = 1172471
  name = "Apex Legends Content"
  default = true               # selected by default; languages stay opt-in

  [[apex.versions]]
  id = "s20-launch"            # slug; user files override by this id
  name = "Season 20 launch"
  date = 2024-02-13            # native TOML date; orders the versions
  type = "season"              # season | event | hotfix
  group = "season-20"          # optional, groups versions in the UI
  manifests = [ { depot = 1172471, manifest = "7843921000000000000" } ]   # a string: IDs are 64-bit
  ```
- **A version lists only the depots that changed** (decided). For a depot it doesn't list, the version uses that depot's manifest from the closest *earlier* version (by `date`, ties by file order) that lists it. This is resolved when a version is picked and written into the config as pinned `ID=MANIFEST` pairs, so the CLI and the engine see only concrete manifests. A depot with no earlier manifest at all is left out and the UI says so.
- Because every version resolves to a full manifest set, two versions can be compared depot by depot (which manifests differ) later.
- Depot names are kept in the file. Reading them from Steam to fill blanks is a later convenience. No per-version `branch` (decided); pinned manifests make it unnecessary.
- **Location: shipped + user, merged** (decided). The release ships catalogs; the user's files (in a user directory, path open **(?)**) add entries or override shipped ones by id, so updating the shipped set never clobbers user edits.
- **UI only for now** (decided): the catalog code lives in `respak-web`; it moves to `respak-host` if the CLI ever wants `--version NAME`.
- **Shipped content:** Apex Legends and Titanfall 2 (decided). Who keeps it current, and how entries are collected from SteamDB, is open **(?)**.

## 6. Tree and search
- The tree is **lazy**: one directory or container level per request. Containers are opened (mounted) only when expanded.
- **Search has scopes** (decided):
  1. **Files:** names on disk or in the Steam manifest. Free, instant, opens nothing. First version.
  2. **Loaded containers:** assets of the containers already opened (expanded) in this session. First version **(?)**.
  3. **Deep search** within selected files (opens them, may download over Steam): later, not designed yet.
  Many rpak assets have only a GUID until the name cache knows them; search matches the name when known and the GUID otherwise.
- A large directory is paged / virtualized in the UI; the server never returns an unbounded list.
- **Inside a pak there are two switchable views** (decided): *path view* (as the rpak volume lists: `texture/foo.txtr`, or `0x8A3F….txtr` when the name is unknown) and *kind view* (pak › `txtr` › items). Both are views over the same listing; the entry's virtual path, used for convert requests, is the same in either.
- Ticking a directory or container in the tree stands for everything under it (a glob such as `dir/**/*.txtr` per chosen kind), not a list of thousands of entries; the basket shows it as one row.

## 7. Viewers
Chosen by the detected format and capabilities, not by file extension:
| Capability / format | Viewer |
|---|---|
| `Bitmap` (`txtr`, `uiia`, `uimg`, `png`, `dds`) | image with mip / layer selector (via a PNG conversion). **First version** |
| `uimg` | atlas with sprite outlines. **First version** |
| `Audio` | audio player (via `wav`). **First version** |
| `Metadata` | key/value panel |
| `matl`, JSON-like targets | text / JSON view |
| `bik` | video: later; no browser-playable target exists today (AVI / MOV / Y4M), so convert-only until one does |
| `Mesh`, `Skeleton`, `Animation` | not in the first version: metadata and convert only |
| anything else | metadata + hex of the first bytes **(?)** |

Previews are converted on demand and are not written to the output directory.

**Convert options forms are generated from schemas** (decided). The server returns a JSON Schema per target; the frontend renders the form from it. This needs option schemas from core and the plugins first (the `schemars?` question in project.md §11), so it is a **prerequisite slice** before the convert dialog. Until then the options of a target can only be its defaults from `discover`.

## 8. Security
A local server that can read the game folder and holds a Steam refresh token must not be usable by any web page open in the same browser.
- Bind to `127.0.0.1` only. Random free port by default, `--port` to fix it, `--no-open` to not launch the browser (proposed **(?)**).
- **No session token** (decided); protection is the browser checks below. Accepted limit: any *local process* can call the API, as it could read the same files directly. A token can be added later without changing the API shape.
- Reject any request whose `Host` isn't the server's own address (stops DNS rebinding) and any request carrying an `Origin` or `Sec-Fetch-Site: cross-site` that isn't the server itself.
- Every state-changing endpoint takes `application/json` only (forces a preflight from other origins, which is refused). `GET` endpoints never change state.
- No CORS headers. No endpoint serves files outside the input, the output and the cache.
- The Steam token stays in `.respak/` exactly as in [steam.md](steam.md); the API never returns it.

## 9. Caching
- **The UI adds no cache of its own** (decided). Steam keeps using its existing cache unchanged, [steam.md](steam.md) §6 (`.respak/steam/…`), and the optional `cache` operator of the engine (project.md §7) is whatever `cache` in the config says.
- Previews are converted again on each open; if that proves slow, a preview cache is a later slice.

## 10. Errors and progress
- Engine errors are returned as kind + message (project.md §9) and shown next to the entry that caused them; one failed output never blocks the rest of a batch.
- Progress comes from `Event::Progress` and `RequestFinished` / `RequestFailed`, forwarded over SSE.

## 11. Out of scope for the first version
Writing Respawn formats · 3D model, skeleton and animation viewers · multiple simultaneous sessions or users · remote access / authentication beyond the local token · editing assets · a desktop wrapper (Tauri could embed the same server and frontend later) · mobile layouts.

## 12. Open questions
Collected from **(?)** above; each is decided with the user before its slice is sketched.
Decided: search scopes (§6), no UI cache (§9), output goes to the out directory with a download link (§2), separate `respak-web` binary (§3), `respak-host` crate (§3), first-version previews = images, uimg atlases, audio (§7), config is `respak.toml` (§1), CI-built frontend (§3).
1. Version catalog: exact TOML schema, user directory path, how shipped entries are produced and updated (§5a).
2. Frontend stack details (§3).
3. Where the UI keeps its own state (last source, recent folders), and how Steam login state is shown.
4. Release packaging (CI jobs, archive names). Platforms are decided: Windows x64 and Linux x64; macOS is not targeted.
5. Option schemas: `schemars` or hand-written, and who owns them (core vs. plugins).
6. Build order of slices (proposal): `respak-host` extraction → option schemas → server + settings + tree → viewers → convert + jobs → Steam login in the UI → version catalog.
