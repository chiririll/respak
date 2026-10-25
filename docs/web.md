# respak web UI

A browser UI for respak: choose where the game files come from, browse them as a tree, look at textures, atlases and sounds, and convert whatever you tick. It is a second host of the same engine the CLI uses, and reads and writes the same `respak.toml`.

```sh
respak-web                      # prints the address and opens the browser
respak-web --port 7878          # a fixed port instead of a free one
respak-web --no-open            # only print the address
respak-web --dir path/to/work   # look for respak.toml here and in the directories above
```

The server listens on `127.0.0.1` only. Design and decisions: [design/webui.md](design/webui.md).

## 1. Settings

The first page asks where the input is:

- **Local folder:** the game directory (`root`). *Browse* lists the folders of this computer.
- **Steam:** the app ID, the depots (`ID` or `ID=MANIFEST`, one per line), the branch. See [Steam](#steam).

Also the output folder (`out`, where converted files go), and under *More options* the search globs, the cache folder, the rpak patches switch and the Respawn plugin's `rig` / `studio`. *Save* writes `respak.toml` (found like the CLI finds it: the working directory, else the nearest parent; created in the working directory if there is none). Comments and key order of an existing file are kept. *Save & connect* builds the engine and opens the explorer.

## 2. Explorer

- **Tree.** Directories, then containers (`rpak`, `starpak`, `mbnk`) that open into their assets. One level is loaded per click and nothing is opened before you expand it, so a 60 GB starpak is as cheap as a text file. Long lists are paged (*Show more*).
- **Inside a pak** the buttons on its row switch the layout: *Paths* (as the pak lists them), *Kinds* (grouped by asset kind) and *Names* (reads every entry to learn its stored name: slow, you asked for it).
- **Search** (top of the sidebar) matches every word, in any case. *File names* searches the names on disk or in the Steam manifest; *Opened paks* searches the assets of the paks you have expanded; *Everything* does both. A hit opens in the tree. A GUID works as a search word, so a dependency shown in the details panel can be found with one click.

## 3. Viewers

The viewer is chosen by what the detected format can become, not by the file name.

| What it is | Viewer |
|---|---|
| Bitmap (`txtr`, `uiia`, `png`, `dds`, ...) | picture on a checkerboard: mip selector, zoom (wheel), pan (drag), 1:1 and fit (double click) |
| `uimg` atlas | the atlas with a rectangle per sprite; click a sprite or pick it in the list |
| Audio (Miles banks) | player with a waveform, track selector, WAV download |
| Something that converts to JSON or CSV (`matl`, `dtbl`, ...) | the text, with a format selector and *Copy* |
| A container | what it holds is in the tree |
| Anything else | detection result, metadata (name, kind, GUID, dependencies) and the first bytes as hex |

The side panel always shows the detected format and version, the score and any runner-up, the asset metadata and what the entry can become. Previews are converted on demand into memory: nothing is written to the output folder.

## 4. Convert

Tick the box on any row (files, assets, folders, paks, kinds in the Kinds layout) to put it in the **basket**, at the bottom of the window. A ticked folder or pak stands for everything under it. *Convert* opens the dialog:

1. the rows (for a folder or a pak you may give a *kind* such as `txtr` to take only those),
2. the target: what the ticked files can become, then every other format,
3. the options of the target, as a form made from its default options,
4. for a single file, an optional output path.

Results go to the output folder and appear under **Jobs**: per row and per output, with thumbnails of images and a download button per file. One failing output shows its error and does not stop the rest. Several jobs can run at once, and each can be cancelled.

## Steam

*Sign in* works in the page:

- **QR code:** scan it with the Steam mobile app (Steam Guard → Sign in with QR); it refreshes itself when Steam rotates it.
- **Account name and password:** then the Steam Guard code (emailed or from the authenticator), or approve in the mobile app. The password goes to the local server for that one sign-in, is never stored or logged, and is never returned.

The token is kept in `.respak/account.json`, exactly as `respak steam login` keeps it, and the API never returns it. *Sign out* deletes it. Steam allows one session per account: a second sign-in while one is in progress is refused.

### Game versions

*Pick a game version* offers named versions (seasons, events, hotfixes) instead of manifest IDs. Picking one pins every selected depot to a concrete `ID=MANIFEST` and fills the depots field, so the CLI and the engine only ever see manifests.

Catalogs are TOML. respak ships one file per game (every `crates/respak-web/catalog/*.toml` is embedded, nothing to register: the game, its depots and versions; the format is in the comments of `apex.toml`). Your own files in **`.respak/catalog/*.toml`** next to your `respak.toml` add to it, or replace an entry that has the same `id`:

```toml
[apex]
name = "Apex Legends"
app_id = 1172470

[[apex.depots]]
id = 1172471
name = "Apex Legends Content"
default = true                 # ticked by default; languages stay opt-in

[[apex.versions]]
id = "s20-launch"              # a slug; your file replaces a shipped version by this id
name = "Season 20 launch"
date = 2024-02-13              # a TOML date; orders the versions
type = "season"                # season | event | hotfix
group = "season-20"            # optional: shown together
manifests = [ { depot = 1172471, manifest = "7843921000000000000" } ]   # a string: ids are 64-bit
```

A version lists only the depots that **changed**. For another depot, the closest earlier version (by `date`, ties by file order) that lists it gives the manifest; a depot with no earlier manifest at all is left out, and the page says so. A file with a mistake is reported (with its name and the line) and the others still load. The shipped catalog carries no manifests yet: they come from SteamDB and have to be collected by hand.

## Safety

The page can read your game folder and the server holds your Steam token, so other pages open in the same browser must not be able to use it: the server refuses a `Host` that is not its own address (DNS rebinding), any `Origin` or `Sec-Fetch-Site: cross-site` request from elsewhere, and any state-changing request that is not `application/json` (which a foreign page cannot send without a refused preflight). There are no CORS headers and no endpoint serves files outside the input, the output and the cache. Any program running on your computer can still call the API, as it could read the same files directly.

## Developing

The frontend (`web/`: Svelte 5, Vite, TypeScript) is built once and embedded in the binary; the repo holds no built files.

```sh
cd web && npm ci && npm run build     # needed once before building respak-web
cargo build -p respak-web             # not a default workspace member: plain `cargo build` needs no Node
```

A debug build of `respak-web` reads `web/dist` from disk, so `npm run build` is enough to see a change. For hot reload:

```sh
cargo run -p respak-web -- --port 7878 --no-open
cd web && npm run dev                 # http://localhost:5173, /api goes to port 7878 (RESPAK_API to change)
```

`cargo run -p respak-web --example demo -- target/demo` writes a small synthetic game folder (a texture pak with an atlas) to try it on; then `respak-web --dir target/demo`.
