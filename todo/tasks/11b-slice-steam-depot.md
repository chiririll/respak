# 11b · Slice: Steam depots as input
**Status:** in-progress (built; waiting for the user's demo) · **Depends on:** 11a · **See:** [STEAM.md](../STEAM.md) §3, §5–7

## Goal
Run the existing commands on game files straight from Steam, downloading only the chunks the request needs:
```
respak --steam 1172470 --depot 1172471=<manifest> --out out discover paks/Win64/common.rpak --depth 1
respak --steam 1172470 --depot 1172471=<manifest> --out out convert paks/Win64/common.rpak/texture/foo.txtr --to '{"type":"png"}'
```

## Scope
**respak-steam:**
- PICS (current manifest per `--branch`, `depotfromapp`), depot keys, manifest request codes, CDN servers and auth tokens. The connection is opened lazily, only when something isn't cached.
- Manifests stored as downloaded; filenames decrypted with the depot key.
- One merged tree over the `--depot` list: `/` separators, original case, case-insensitive lookups and globs, the later depot wins.
- A read-only OpenDAL service: `stat`, `list`, `read(range)` → the overlapping chunks (binary search), at most 8 fetches in parallel, shared in-flight fetches per `(depot, sha)`.
- Cache in `.respak/steam/` (STEAM.md §6): keys and manifests kept forever; chunks verified (size + Adler32), LRU by mtime with an 8 GiB cap (`--steam-cache-size`); the size is scanned once, then tracked in memory.
- A summary per run: chunks and bytes downloaded, cache hits.

**respak-cli:** `--steam APP`, `--branch B`, `--depot ID[=MANIFEST]...`, `--steam-cache-size`; `--steam` and `--root` are mutually exclusive.

## Leaves out
Oodle (11c), so the demo uses an Apex version from before Oodle. rpak patches. Progress events (12): downloads show as `tracing` logs only.

## Tests (agreed at the gate: "ok", with the additions below)
All synthetic, no Steam:
- Range → chunks: inside one chunk, across a boundary, the last chunk, past EOF (clamped), an empty file.
- Tree merge: later depot wins, case-insensitive lookup, `\` → `/`, globs.
- Cache: store, then hit; a corrupt chunk is detected, deleted and fetched again; eviction keeps the size under the cap; the startup scan matches the in-memory size.
- Two concurrent reads of the same chunk fetch it once.
- With every depot pinned and everything cached, a read makes no network call.

Live (env var only): open a pinned depot, `stat` and ranged-read one file.

## Demo
On an Apex version from before Oodle (pinned with `--depot ID=MANIFEST`): `discover paks/Win64/common.rpak --depth 1`, then convert a texture with streamed mips. The summary shows only the needed MB downloaded, and a second run downloads nothing.

## To verify
Manifest magic constants against a real manifest; where steamroom unwraps the manifest zip; whether the Adler32 check covers decompressed data; that request codes for old manifests are granted (STEAM.md §9).

## Decisions at the gate
- No license fetch: steamroom has no API for it, and a key request for a game the account does not own fails with a clear error anyway.
- SHA-1 of each chunk is checked against the manifest on top of size + Adler32.
- CDN auth tokens are fetched per host, cached until just before they expire.
- Added tests: random ranges against the file, `--depot` spec parsing, CLI flags and sizes, a failed fetch names depot/manifest/file.

## Done
- `respak-steam`: `chunks` (binary search + assemble), `tree` (merge, case-insensitive), `cache` (verified, LRU by mtime, 90% eviction target, size in memory), `store` (shared in-flight fetches, 8 parallel, stats), `depot` (`VolumeSource` -> `Operator`), `cdn` (`ChunkSource`: pooled CDN servers, `process_chunk` + SHA-1), `link` (lazy `Resolver`: PICS, `depotfromapp`, keys, request codes, manifests; `open`).
- CLI: `--steam APP --depot ID[=MANIFEST]... [--branch B] [--steam-cache-size SIZE]`, exclusive with `--root`; end-of-run summary on stderr.
- Live (current Apex, depot 1172471): key, manifest (4.5 MB), CDN chunk, ranged read of `common.rpak` all work; the first rpak read fails with "unsupported version 8 (Oodle)", as planned (11c).
- Learned: Steam allows one session per account, so two respak processes (or parallel live tests) using the same token kick each other off (`Disconnected`). Run live tests with `--test-threads=1`. The `discover ""` of the root is "not found" (core does not treat the empty path as a directory; not changed here).
- Not done / left: reconnect after a dropped connection (the lazy client is opened once per run); the manifest magic constants were not a problem on a real manifest.
- Still for the demo: an older manifest ID from before Oodle (SteamDB).
