# respak

![100% Vibe_Coded](https://img.shields.io/badge/100%25-Vibe_Coded-D97757?style=for-the-badge&logo=claude&logoColor=white)

A Rust library and CLI that reads files from Respawn games and converts them into usable formats, for example an `.rpak` texture to PNG or DDS, a material to JSON plus its textures, or a Bink 2 video to AVI.

Conversion is read-only: writing Respawn formats is not supported.

## What it reads

| Format | Notes |
|---|---|
| `rpak` | header versions 6, 7, 8; uncompressed, RTech and Oodle compression. Patched and zstd paks are not supported |
| `starpak` | streamed texture mips, found automatically beside the pak |
| `txtr` | textures, asset versions 8-10 |
| `matl` | materials, converted to JSON plus textures |
| `uiia` | single tiled UI images (not atlases) |
| `bik` / `bk2` | Bink 2 video and audio only |
| `png`, `dds` | plain image files |

Other asset kinds (models, animations, ...) show up in pak listings but cannot be converted yet.

## What it writes

`png`, `dds`, `png.frames` (video to a folder of PNGs), `wav`, `avi`, `mov`, `y4m`, `matl.json`, `uiia.json`, and `file` (unchanged copy).

## Quick start

Build (Rust, see [rust-toolchain.toml](rust-toolchain.toml)), or take a binary from the releases page:

```sh
cargo build --release -p respak-cli      # target/release/respak
```

Convert every texture of a pak to PNG:

```sh
respak --root "C:/Games/Apex" --out out convert "paks/Win64/common.rpak/*.txtr" --to '{"type":"png"}'
```

Read straight from Steam, without installing the game:

```sh
respak steam login --qr
respak --steam 1172470 --depot 1172471 --out out convert "paks/Win64/common.rpak/*.txtr" --to '{"type":"png"}'
```

## Documentation

- [CLI guide](docs/cli.md): commands, options, request files, formats.
- [Steam guide](docs/steam.md): logging in, choosing depots, cache, security.

## Workspace

| Crate | Purpose |
|---|---|
| `respak-core` | engine: format graph, containers, caching, events |
| `respak-respawn` | Respawn formats (rpak, starpak, txtr, matl, uiia) |
| `respak-formats` | PNG, DDS, WAV and frame output |
| `respak-nihav` | Bink 2 decoding and AVI/MOV/Y4M encoding |
| `respak-steam` | Steam login and depot access |
| `respak-cli` | the `respak` binary |

## License

AGPL-3.0-only, see [LICENSE](LICENSE). The format code is ported from the [rsx](https://github.com/r-ex/rsx) tool.
