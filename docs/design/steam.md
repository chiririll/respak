# respak — Steam input

The respak CLI can read game files straight from Steam depots. It downloads only the byte ranges a request needs, and caches what it downloads. This document is the design of the `respak-steam` crate; the user guide is [`../steam.md`](../steam.md).

---

## 1. Rules
- **Steam code lives only in `respak-steam`**, an optional dependency of `respak-cli` behind the cargo feature `steam` (**on by default**). This is the one exception to project.md §1 "No Steam or downloader code". Core and the plugin crates never depend on it and know nothing about Steam.
- **For core, Steam is just an input `Operator`.** Existing commands (`discover`, `convert`, `run`) work on it unchanged. The I/O rules (project.md §6) apply: sizes come from `stat`, every read is ranged, and nothing is read ahead beyond the chunks that cover the range.
- **No trait over the Steam client.** `respak-steam` uses `steamroom` directly; replacing it means changing this crate only.
- **The account must own the game.** There is no anonymous login and no third-party key or request-code service (tek-s3).
- **State lives in `.respak/` in the working directory**, like DepotDownloader. It is listed in `.gitignore`, because it holds a plaintext token.
- **Steam is input only.** Output always goes to the local `--out`.
- **Out of scope:** branch passwords, depot listing commands, OS keyring storage and token encryption.

## 2. Crate and dependencies
```
crates/respak-steam/   # login, token file, PICS, depot keys, manifests, CDN chunks, cache, depot Operator
```
| Purpose | Crate | Note |
|---|---|---|
| Steam CM, auth, PICS, CDN, manifest parsing, chunk decoding | `steamroom` | MIT/Apache. Pinned to a **git rev**: the auth fix "mobile confirmation alongside Steam Guard codes" (Aug 2026) is not released yet (0.3.0). Young, single author. |
| QR code in the terminal | `qrcode` (`unicode::Dense1x2` renderer) | |
| Password prompt | `rpassword` | Only during `steam login --user`. |
| Storage | `opendal` (workspace pin) | The depot is a read-only OpenDAL service (project.md §6 "Read-only service"). |

## 3. CLI
```
respak steam login --qr                 # renders a QR code; scan it in the Steam mobile app
respak steam login --user NAME          # password prompt, then Steam Guard: email code, authenticator code,
                                        #   or confirmation in the mobile app
respak steam logout                     # deletes .respak/account.json

respak --steam APP [--branch B] --depot ID[=MANIFEST]... [--steam-cache-size SIZE] --out DIR <command>
respak --steam 1172470 --depot 1172471=7843921… --out out convert 'paks/Win64/*.rpak/**/*.txtr' --to '{"type":"png"}'
```
- `--steam` and `--root` are mutually exclusive. `--steam` needs at least one `--depot`.
- `--depot ID` takes that depot's current manifest for `--branch` (default `public`), read from PICS. `--depot ID=MANIFEST` pins a manifest, which is how older game versions are read. The user gets manifest IDs from SteamDB.
- The chosen depots are merged into one file tree. On a path collision, **the depot later in the `--depot` list wins** (list order, not date).
- Paths: manifest `\` becomes `/`, the original case is kept, and lookups and globs match **case-insensitively** (`paks/win64/common.rpak` finds `Paks/Win64/common.rpak`).
- `respak --steam APP --depot ID --out DIR copy 'paks/Win64/*.rpak'` copies whole files unchanged (ranged reads, 4 MiB at a time); it is `convert --to '{"type":"file"}'` underneath, so a `run` request can mix such outputs with conversions, and `convert ... --copy` writes the original next to each conversion.
- If the stored token is missing, expired or revoked, the command fails with "run `respak steam login`". It never prompts in the middle of a conversion.

## 4. Login and the token file
- Login uses Steam's auth service: `BeginAuthSessionViaQR` or `BeginAuthSessionViaCredentials`, then `PollAuthSessionStatus` until it returns a refresh token.
- **QR:** a poll can return `new_challenge_url`; the QR code is redrawn when it does. steamroom's `poll_auth_session` ignores that field, so respak runs the poll itself through `call_service_method`, or the fix goes upstream.
- **Stored:** `.respak/account.json` = `{ account_name, steam_id, refresh_token }` (account name is empty for a QR login). The password is never stored.
- **Logon with the token:** `CMsgClientLogon` with the refresh token in `access_token` (as steam-vent does). steamroom's `login` takes a generic `ClientMsg`, so respak builds this message.
- **Expiry:** the token is a JWT; respak reads `exp`. Close to or past expiry, or when Steam rejects it, respak tries to renew it once (`GenerateAccessTokenForApp` with renewal; rsx does the same with `tek_sc_cm_auth_renew_token`) and saves the new token. If renewal fails: the "run `respak steam login`" error.
- The device name shown in the account's authorized devices is `respak on <hostname>`.

## 5. Opening a depot (`--steam`)
The Steam connection is opened **lazily**, only when something isn't cached. With every depot pinned (`=MANIFEST`) and everything needed in `.respak/`, a run works offline.
1. Log on with the stored token. Fetch licenses.
2. PICS: get the app's access token, then its product info. From it take `depots/<id>/manifests/<branch>` (falling back to `public`), and `depotfromapp` for shared depots: the key app is the parent app (rsx `steam_depot_util.cpp`). Skipped when every depot is pinned.
3. **Depot key:** from `.respak/steam/keys/<depot>.key`, else `GetDepotDecryptionKey` for the key app, then stored. Keys are fixed per depot, so they are kept forever.
4. **Manifest:** from `.respak/steam/manifests/<depot>_<manifest>.manifest`, else:
   - get a request code (`GetManifestRequestCode`, the branch variant for a non-public branch). Codes are valid for about 5 minutes and are kept in memory only. A code of 0 means Steam refused the request; the error says so and names the depot and manifest;
   - get the CDN server list and, per host, a CDN auth token if required (in memory only);
   - download `depot/{depot}/manifest/{manifest}/5/{code}` and store the bytes **exactly as downloaded** (a zip holding a protobuf). respak has no manifest format of its own. A manifest ID never changes, so it is kept forever.
5. Parse the manifest and decrypt the filenames with the depot key (steamroom). Build the tree (§3: `/`, case-insensitive index, later depot wins).
6. The tree becomes a read-only `Operator`: `stat` = file size from the manifest, `list` = the tree, `read(range)` = §6.

## 6. Ranged reads and the chunk cache
- A file is a list of chunks sorted by offset (about 1 MiB uncompressed each). For a range: binary search for the first overlapping chunk, take every chunk up to the end of the range, fetch them, then slice the result. A chunk is the smallest unit; there is no further read-ahead.
- **Fetch one chunk:** cache hit, else `depot/{depot}/chunk/{sha1 hex}` from a CDN server (try the next server on failure), then decrypt (AES-256: ECB for the IV, then CBC) and decompress (VZip/LZMA, VZstd, plain zip) with steamroom's `process_chunk`. Check the decompressed size and Adler32 against the manifest, then store.
- **Concurrency:** parallel fetches up to 8 at a time (fixed, no flag yet). Concurrent requests for the same `(depot, sha)` share one download.
- **Cache layout:**
  ```
  .respak/
    account.json
    steam/keys/<depot>.key                       32 raw bytes; never evicted
    steam/manifests/<depot>_<manifest>.manifest  as downloaded; never evicted
    steam/chunks/<depot>/<sha1 hex>              decrypted, decompressed; LRU
  ```
- Chunks are identified by their content, so versions of a depot share most of them: switching to an older version downloads only what changed. Chunks are not shared across depots.
- **Read back:** a cached chunk is checked (size + Adler32); a bad one is deleted and downloaded again.
- **Eviction:** LRU by file modification time (a cache hit updates it). The cap is **8 GiB**, set with `--steam-cache-size`. The chunk directory is scanned once when the volume opens; after that the size is tracked in memory, never by walking the tree on every write.
- **Known cost:** a compressed `.rpak` body is one RTech stream of output chunks (about 4 MiB each), decoded front to back. respak decodes only as far as a read needs (`rtech::decompress_prefix`): listing a pak reads the start of the file (a few MB), reading a page near the end reads most of it. Chunks already downloaded stay in the chunk cache. Starpaks stay ranged. (Before this, the whole `.rpak` was downloaded: 885 MB for `common.rpak`.)
- **Reporting:** downloads show as `tracing` logs, plus a summary at the end of each run: chunks downloaded, bytes downloaded, cache hits.
- Steam and CDN errors reach core as `opendal::Error` (→ `Storage`), with a message that names the depot, manifest and file.

## 7. Lessons from the rsx fork
The rsx fork (`../rsx`, `src/core/steam/`, `src/game/rtech/steam_starpak_source.*`) already reads ranges from Steam through tek-steamclient, including older manifests: a logged-in owner gets their request codes directly from Steam. Keep its protocol knowledge; don't copy these problems:

| rsx | respak |
|---|---|
| One shared callback waiter for all requests; late replies write into dead stack frames | One future per request, matched by job ID |
| A disconnect is never noticed; every call then times out after 30 s | Disconnects are detected; respak reconnects with the stored token |
| Token `expires` is never checked; renewal is tried on one code path only | `exp` checked; renew once, everywhere |
| Range → chunks: a linear scan, then chunks fetched one by one | Binary search, bounded parallel fetches |
| Cached chunks are not verified; the whole cache tree is walked on every write | Size + Adler32 check; size tracked in memory |
| A cached manifest still makes network calls for the key and server list | Lazy connection; works offline when everything is cached |
| One depot chosen by a heuristic; starpaks in other depots are missed; basename lookups "first wins" | The user lists the depots; one merged tree; full paths only |
| Whole rpaks and their patch chains downloaded into the cache as files | Ranged reads through the Operator, patch chains included |
| Token stored next to the exe | `.respak/account.json` in the working directory |

## 8. Tests
Unit tests use synthetic data, not Steam: range -> chunk mapping, cache store, verify, evict and rescan, the tree merge (order, case, `/`), token file and expiry handling. Live Steam tests are integration tests (`crates/respak-steam/tests/live.rs`) on the user's account: they are `#[ignore]`d and also need `RESPAK_STEAM_LIVE=1`, they never run in CI, and **they (and any other command that talks to Steam) are never run without a need and the user's explicit permission**. They run with `--test-threads=1`: Steam allows one session per account. `cargo build -p respak-cli --no-default-features` must build without Steam code.

## 9. Known pitfalls
- steamroom's manifest magic constants differ from SteamKit2's (`0x1B81B817` / end `0xD64BF064` vs signature `0x1B81B817` / end `0x32C415AB`); check them against a real manifest.
- Request codes for old manifests are still granted (they work in rsx).
