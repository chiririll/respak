# CLI guide

```
respak [GLOBAL OPTIONS] [PROFILE] <command>
```

The global options can also come after the command (`respak convert x --out out`), and can be kept in a [config file](#config-file-and-profiles). A command run with no arguments (`respak convert`) prints its help.

Paths you pass to commands (`from`, `PATH`) always use `/` and are relative to `--root` (or to the Steam depots, see [steam.md](steam.md)).

## Global options

| Option | Meaning |
|---|---|
| `--root DIR` | Input directory (the game folder). Required unless `--steam` is used (or set in the config file). Created if missing, so a typo makes an empty folder |
| `--out DIR` | Output directory, created if missing. Required by every command except `steam` (or set in the config file) |
| `--search GLOB` | Extra paks to look in when an asset refers to another asset (such as a material's textures) that is not in its own pak or the `common.rpak` beside it. Relative to `--root`, repeatable, e.g. `--search "paks/Win64/*.rpak"`. Nothing is scanned unless a lookup needs it |
| `--no-patches` | Do not apply rpak patches. By default `common.rpak` next to a `patch_master.rpak` is opened as its newest patch (`common(NN).rpak`), and a patch file is completed from the files it lists. With this flag `patch_master.rpak` is ignored and a patch file is refused as an unsupported version |
| `--rig AUTO\|GUID\|PATH` | The skeleton animation sequences (`aseq`) are bound to. `auto` (default) takes the `arig` (else the `mdl_`) that lists the sequence, looked for in its pak and the `common.rpak` beside it. Otherwise the guid (`0x...`) or the path of an `arig` / `mdl_` |
| `--studio AUTO\|LAYOUT` | How the studio layout of an `arig` is found: `auto` (a `mdl_` in the same pak, else the block itself) or `v16`, `v17`, `v19_2` |
| `--cache DIR` | Remembers where `--search` found each asset, so later runs are faster. Optional |
| `--memory-cache SIZE` | Keep finished conversions in memory, up to `SIZE` (e.g. `256MiB`), so an asset used by several others (a texture shared by materials) is converted once. Off by default, `0` is off. Also `memory_cache` in the config file |
| `--report FILE` | Write the JSON report to a file instead of stdout |
| `--steam`, `--depot`, `--branch`, `--steam-cache-size` | Read from Steam, see [steam.md](steam.md) |
| `--config FILE`, `--no-config`, `-p/--profile NAME` | Which config file supplies the defaults, see [below](#config-file-and-profiles) |

## Config file and profiles

Typing `--root`, `--out` and the rest every time gets old. Put them in a `respak.toml`:

```toml
root = "D:/Games/Apex"
out = "out"
cache = ".cache"
report = "report.json"
search = ["paks/Win64/*.rpak"]
no_patches = false
memory_cache = "256MiB"

[respawn]
rig = "auto"
studio = "auto"

# Instead of `root`:
# [steam]
# app = 1172470
# depots = [1172471, "1172472=7843921"]   # ID or "ID=MANIFEST"
# branch = "public"
# cache_size = "8GiB"
```

Then `respak convert "paks/Win64/common.rpak/*.txtr" --to '{"type":"png"}'` is the whole command. Every key is the flag of the same name; unknown keys are an error.

- **Where it is looked for:** `respak.toml` in the working directory, else in the nearest parent directory that has one. No file is fine, the flags alone then have to say everything.
- **Relative paths** in the file (`root`, `out`, `cache`, `report`) are relative to the file's directory, not to where you run `respak`. Paths on the command line stay relative to the working directory.
- **The command line wins.** A flag overrides the file; for `--search` and `--depot` the flag's list replaces the file's. `--no-patches` can only be switched on from the command line.
- **The input is one choice.** If you pass `--root` or `--steam`, the file's `root` and `[steam]` are not looked at. `--depot`, `--branch` and `--steam-cache-size` on their own refine the file's `[steam]`.
- `--config FILE` reads that file instead; `--no-config` reads none.

### Profiles

For several games, keep one file per game, `respak-<name>.toml`, and name it before the command:

```sh
respak apex convert "paks/Win64/common.rpak/*.txtr" --to '{"type":"png"}'    # respak-apex.toml
respak -p apex list paks/Win64/common.rpak                                    # the same, with the flag
```

A profile reads only its own file (not `respak.toml`), found the same way, and the file must exist: a typo like `respak covnert ...` ends with `respak-covnert.toml not found`. Names are letters, digits, `-` and `_`. `-p/--profile` is the way to use a profile named like a command. `--config` overrides the profile lookup (with a warning).

Run `RUST_LOG=info respak ...` to see which file was read.

## Commands

### `formats`
Prints the format graph as JSON: every format, its file extensions and what converts to what.

### `discover PATH... [--depth N] [--names]`
Says what each path is and what it can be converted to. `--depth 0` (default) describes the item itself, `--depth 1` lists what is inside a pak. `--names` reads every entry to fill in its `name` where it is not known without decoding (an rpak asset's name is in its header); slow, like `list --names`.

```sh
respak --root game --out out discover paks/Win64/common.rpak --depth 1
```

Each entry has a `path` you can paste into a request, the detected `format` and `version`, and `targets`, where `type` plus `defaults` can be used as `--to`.

### `list PATH... [--map | --names]`
Prints the entries of a container (rpak, mbnk, ...), one per line, sorted, relative to the container (`assets/some_asset.type`). Nothing is described or opened, so it is fast even for banks with tens of thousands of entries. `--map` reads every entry and, where it has a name of its own, prints it after a tab (`0x…16.txtr<TAB>texture/ui/icon.txtr`). `--names` prints only those names, one per line, and skips entries without one. Both are slow, and only asset kinds respak can read have names. To use an entry in `convert`, put the container path in front: `paks/Win64/common.rpak/assets/some_asset.type`.

```sh
respak --root game list paks/Win64/common.rpak
```

### `names [--hashes FILE] [--prefix TEXT]... [--suffix TEXT]... [--guids] SOURCE...`
Looks for the names of asset guids. Prints one name per line; with `--guids` each line is `0x<GUID><TAB><name>`. The wanted guids come from `FILE` (a local file; any text, each `0x<16 hex digits>` counts, so a `list` output works), or, without `--hashes`, are the assets of the sources that have no name of their own. Each `SOURCE` is a path or glob on the input (the `--root` folder or the Steam depots).

Everything is read in one pass. Each pak among the sources is read like `list --names` does, its assets decoded once and several at a time, and each gives its stored name (a name in one pak often names an asset of another), the names of its dependencies, and what its format offers as possible names (a `dtbl`'s asset-path cells). A file of no known format (an executable, a script) is read as bytes and every run of printable ASCII is taken. Paks are not scanned as bytes, their data is compressed; files of a known format with no names in them (textures, video) are skipped. With `--hashes` the remaining sources are skipped once every wanted guid is named; without it everything is read, since the unnamed assets are only known at the end.

Each string, and each word in it (split at quotes, spaces, brackets), is hashed and kept when it matches a wanted guid. `--prefix` / `--suffix` also try the string with that text added (e.g. `--prefix datatable/ --suffix .rpak`). Matches print as `0x<GUID><TAB><name>`; progress and the count go to stderr. A match is verified by the hash, so a wrong string can only miss. A file that cannot be read is a warning.

```sh
respak --root game --out out names --suffix .rpak "paks/Win64/*.rpak" "*.exe"
```

### `convert PATH --to TARGET [--path OUT] [--copy]`
Converts a file or every file a glob matches. `TARGET` is JSON, e.g. `'{"type":"dds"}'`. `--path` sets the output path under `--out`; `--copy` also copies the source file unchanged.

```sh
respak --root game --out out convert "paks/Win64/common.rpak/*.txtr" --to '{"type":"png","mips":"all"}'
```

On PowerShell, quote the JSON with single quotes and escape inner quotes as `\"` if your shell strips them.

### `copy PATH [--path OUT]`
Copies a file or glob to `--out` unchanged (with `--steam`: downloads it). Assets inside a pak have no file of their own and are skipped.

### `run REQUEST`
Runs a request file (`-` reads stdin) with any number of outputs. See below.

### `steam login|logout`
See [steam.md](steam.md).

## Request files

```json
{
  "version": 1,
  "outputs": [
    { "from": "paks/Win64/common.rpak/*.txtr", "to": { "type": "png", "mips": "first" } },
    { "from": "paks/Win64/common.rpak/*.matl", "to": { "type": "matl.json", "textures": { "type": "dds" } } },
    { "from": "media/intro.bik", "to": { "type": "avi", "codec": "mjpeg", "quality": 85 }, "path": "video/intro.avi" }
  ]
}
```

- `from`: a path or glob, a list of them, or `{"path": "...", "format": "txtr@10"}` to force the format of an asset that is not in a container.
- `to`: `{"type": "<format>", ...options}`. Options sit in the same object. Optional `"via": ["fmt", ...]` forces the conversion to pass through those formats. Unknown options are rejected.
- `path` (optional): output path under `--out`. Without it the name is derived from the source and the target's extension. Use it only with a single source: a glob or list written to one `path` conflicts.
- Outputs run independently; one failure does not stop the rest.

### Globs

`*`, `?`, `[...]` and `{a,b}` are supported. A glob only enters a pak when a segment names the pak file and more segments follow: `paks/Win64/*.rpak/*.txtr` opens the paks, `paks/**/*.txtr` does not.

### Paks

A pak is addressed like a folder: `paks/Win64/common.rpak/0x8A3F12C4D5E6F701.txtr`. Names are not stored in paks, so listings show `0x<GUID>.<kind>`. A readable name such as `.../texture/foo.txtr` is hashed to its GUID and resolves too. Starpaks are looked up beside the pak.

## Target formats

| `type` | From | Options (default) | Result |
|---|---|---|---|
| `png` | texture, UI image, dds, png | `mips`: `"first"`, `"all"` or a level number | one file; `"all"` gives a folder |
| `dds` | any image | none | one file |
| `png.frames` | video | `from` (1), `step` (1), `count` (all) | folder of `frame_00001.png`... |
| `wav` | audio from video | `track` (0) | 16-bit PCM |
| `avi` | video | `codec`: `mjpeg`, `cinepak`, `zmbv` (lossless); `quality` 1-100 (90); `audio_track`; `mute` (false) | one file |
| `mov` | video | as `avi`, codecs `mjpeg`, `cinepak` | one file |
| `y4m` | video | codec `rawvideo`; no sound | one file |
| `matl.json` | material | `textures`: a target (`{"type":"png"}`) | folder with `material.json` and `textures/` |
| `uiia.json` | UI image | `image`: a target (`{"type":"png"}`) | folder with the image and JSON layout |
| `uimg.json` | UI image atlas | `image`: a target (`{"type":"png"}`); `sprites`: cut every sprite out as its own image (false) | folder with `atlas.<ext>`, `uimg.json` (sprites in stored order) and, with `sprites`, `sprites/<path>.<ext>` |
| `cast` | model | `lods`: `0`, `"all"` or a list (0); `skin` (0); `textures` (true); `image`: texture format (`"png"`) | folder with one `<bodypart>_<n>_LOD<m>.cast` per LOD and model, and `textures/` |
| `cast` | animation | `root_fixup`: `"rsx"` (turn bone 0 by -90 degrees of yaw, position `(x, y)` to `(y, -x)`, as rsx does) or `"none"`; `blend`: `"all"` or one blend index (all) | folder with one `<name>_<blend>.cast` per blend: the skeleton and, per bone, position, rotation (and scale) curves with a key on every frame |
| `rmdl.json` | model | none | one JSON file: header, bones, body parts, materials, skins, LODs, hitboxes, attachments |
| `file` | any file | none | unchanged copy |

Textures of a material are found by GUID in the same pak, then the `common.rpak` beside it, then `--search`, then the cache. Slot names come from the material's shader; if that fails they become `slot_<n>` with a warning.

## Output

The JSON report goes to stdout (or `--report`); progress, warnings and errors go to stderr.

```json
{ "outputs": [
  { "from": "paks/Win64/common.rpak/0x...txtr", "result": { "Ok": { "files": ["0x....png"], "dependencies": [] } } },
  { "from": "paks/Win64/common.rpak/0x...matl", "result": { "Err": { "error": { "kind": "MissingDependency", "message": "..." } } } }
] }
```

Entries come in request order, one per matched source.

| Exit code | Meaning |
|---|---|
| 0 | everything succeeded |
| 1 | any output failed, or a fatal error (missing `--out`, a profile without its file, bad JSON, unreadable request, not logged in to Steam) |
| 130 | second Ctrl-C |

The first Ctrl-C stops cleanly: unfinished outputs are reported as `Cancelled`. Press it again to abort at once.

## Logging

Logs go to stderr; the default level is warnings. Set `RUST_LOG=info` for more, e.g. what Steam is doing.

## Limitations

- Read-only: no writing back to Respawn formats.
- Patched paks, zstd paks, Bink 1 video, `matl` outside versions 15-23 (e.g. Titanfall 2) and swizzled/console textures are unsupported.
- Models convert only for asset versions 16-20 (current Apex), without blend shapes or physics; animations only for `aseq` 12.1-14 with RLE data (no events, IK, blend-grid parameters or datapoint compression); older models and animations and in-pak audio cannot be converted; audio is only available from Bink videos.
- A Cast material links a texture only when the material's shader slots can be named; otherwise the textures are still written as `slot_<n>` files and a warning says why.
