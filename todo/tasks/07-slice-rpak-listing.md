# 07 · Slice: open an rpak and list its assets
**Status:** blocked (license decision, PROJECT.md §11) · **Depends on:** 06 · **See:** PROJECT.md §5.4, §6, §12

## Goal
```
respak discover paks/Win64/common.rpak --depth 1
```
lists the pak's assets: path (name or GUID), kind and version. It covers every pak version rsx supports. This is the first container, and it tests the riskiest part of the design: a container as an OpenDAL service, plus virtual paths that go into it.

## Porting from rsx
The pak code comes from rsx (`C:\Users\chiririll\Projects\rsx`, `src/game/rtech/cpakfile.{h,cpp}` and the codecs). Review it first, then port it, following PROJECT.md §12. The review covers at least:
- pak headers and asset entries per version; version detection → the `rpak` score function;
- body compression and the codecs → how far a volume can decompress as a stream (§6) instead of loading the whole pak;
- pointer relocation → whether it supports the `.mem` scheme (§6), which slice 08 depends on;
- GUID hashing of asset paths.

## rsx review
(filled in during the sketch, approved at the gate)

## Scope
**Core:**
- `VolumeSource` (read-only: `stat`, ranged `read`, `list`) and the generic adapter that turns it into an `opendal::Operator` (API notes in PROJECT.md §11).
- Virtual path resolution: the longest real-file prefix is identified; if it is a `Container`, the rest is resolved with `entry(path)`; repeat. Real files win over container entries. Missing path → `NotFound`.
- `discover` with `depth` > 0.
- A mounted volume is kept for the whole `Engine` call (a plain map for now; the dedup middleware comes in slice 13).

**respak-respawn:**
- `rpak` read edge from `file`: score = magic + version → `Version` including the variant. Known format with an unknown version → `UnsupportedVersion`.
- `RpakVolume`: lists the assets, by name when known, otherwise by GUID; both paths resolve. It decompresses the body only as far as needed (§6). The codecs are ported from rsx. Oodle: check how rsx gets it and ask the user before shipping anything Oodle-related.
- GUID hashing of asset paths, ported.
- Typed `rpak.asset` entries (kind, version, header size, guid, …), with only the fields this slice reads. The `header` / `mem` files come in slice 08.
- Fixtures: a synthetic pak generated in the tests. **No game files in the repo.**

## Not in this slice
Asset bytes, `.mem`, pointer relocation, starpaks, globs, GUID dependencies.

## Stop and ask if
- The OpenDAL adapter turns out to be large or awkward. That is the signal this slice exists to catch.
- rsx's pointer relocation can't support the `.mem` scheme in §6 (check it during the sweep, since slice 08 depends on it).

## Demo
The user runs `discover` on real paks of each version and compares the lists with rsx.
