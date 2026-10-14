# Steam guide

respak can read a game straight from Steam's servers, so you don't need it installed. Only the chunks of files that are actually read are downloaded.

You need a Steam account that **owns the game**. Anonymous access is not possible.

## 1. Log in once

```sh
respak steam login --qr              # scan the QR code in the Steam mobile app
respak steam login --user NAME       # password (hidden), then Steam Guard code or app confirmation
respak steam logout                  # delete the stored login
```

`--qr` and `--user` are mutually exclusive. The login is stored in `.respak/account.json` and renewed automatically; if it can't be renewed you get "not logged in to Steam" and need to log in again.

Steam allows one session per account, so don't run two respak processes with Steam at once.

## 2. Read from Steam

Use `--steam APP` instead of `--root`, and list the depots you want:

```sh
respak --steam 1172470 --depot 1172471 --out out convert "paks/Win64/*.rpak/*.txtr" --to '{"type":"png"}'
```

| Option | Meaning |
|---|---|
| `--steam APP` | App id. Replaces `--root` |
| `--depot ID` | Depot to read. Repeatable. Depots are not discovered automatically; find ids on [SteamDB](https://steamdb.info) |
| `--depot ID=MANIFEST` | Pin a specific manifest. Steam may refuse it if the account doesn't own it or it is too old |
| `--branch NAME` | Branch whose current manifest is used (default `public`, also the fallback) |
| `--steam-cache-size SIZE` | Chunk cache limit, e.g. `500MiB`, `8GiB`, `2GB` (default 8 GiB) |

All depots are merged into one read-only tree using the depot's own file paths (no depot id prefix). Lookups are case-insensitive. If two depots have the same path, the later `--depot` wins silently.

`copy` downloads files to `--out`; `convert`, `discover`, `run` and `--search` work as with `--root`. At the end respak prints how many chunks were downloaded and how many came from the cache.

## Where things are stored

Everything lives in `.respak` in the **current working directory** (not configurable):

| Path | Contents |
|---|---|
| `.respak/account.json` | account name, Steam id and refresh token |
| `.respak/steam/keys/` | depot keys |
| `.respak/steam/manifests/` | manifests |
| `.respak/steam/chunks/` | downloaded chunks, the size-capped cache (least recently used removed first) |

Once keys, manifests and chunks are cached, conversion needs no Steam logon at all.

## Security

`account.json` holds a refresh token in **plaintext** that grants access to your Steam account's session. Your password is never stored. Don't commit, share or upload `.respak/` (this repo's `.gitignore` already excludes it), and run `respak steam logout` on shared machines.

## Troubleshooting

- `RUST_LOG=info` shows what Steam is doing (app info, manifests, depots, token renewal).
- "does the account own the game?": the logged-in account lacks a license for that app or depot.
- Pinned manifest rejected: drop `=MANIFEST` to use the branch's current one.
- Steam access is only tested manually, not in CI; the CDN region is fixed.
