# 11b · Slice: Steam depots as input
**Status:** todo · **Depends on:** 11a · **See:** [STEAM.md](../STEAM.md) §3, §5–7

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

## Tests (proposed, to agree at the gate)
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
