# 27 · Slice: version catalog
**Status:** in-progress (built; shipped catalog has no versions) · **Depends on:** 26 · **See:** [webui.md](../design/webui.md) §5a

## Goal
The Steam settings offer named game versions (seasons, events, hotfixes) instead of manifest IDs. Picking one fills `[steam]` in `respak.toml` with concrete pinned manifests.

## Changes
1. Catalog code in `respak-web` (moves to `respak-host` if the CLI wants it): TOML model per webui.md §5a (`[game]`, `[[game.depots]]`, `[[game.versions]]`), strict parsing with clear errors that name the file and key.
2. Loading: shipped catalogs (embedded; Apex Legends and Titanfall 2) merged with user files (directory **(?)**); a user entry overrides a shipped one by `id`.
3. Resolution: a version lists only changed depots; unlisted depots take the manifest of the closest earlier version (by `date`, ties by file order) that lists them; a depot with no earlier manifest is left out and reported.
4. API: `GET /api/catalog` (games, depots, versions grouped by `group` / `type`), `POST /api/catalog/resolve` (version id, chosen depots → pinned `ID=MANIFEST` list).
5. Frontend: version picker grouped by season / event, depot checkboxes (`default = true` preselected), an indication of left-out depots.
6. Shipped data: first entries collected from SteamDB by hand.

## Leaves out
`--version` in the CLI. Fetching depot names from Steam. Comparing versions (the resolved sets make it possible later). Automatic updates of the shipped catalog.

## Open
- User directory path (next to `respak.toml`, or a per-user config directory).
- How the shipped set is produced and kept current.
- Behaviour when two shipped versions have the same date.

## Proposed tests (for the gate)
- Parse: a valid catalog; unknown key, duplicate version id, manifest given as an integer → errors naming file and key.
- Merge: a user version with a shipped id replaces it; a new id is added; shipped file unchanged.
- Resolve: unlisted depot uses the closest earlier manifest; ties by file order; no earlier manifest → left out and reported; a depot not selected is not resolved.
- Ordering by TOML date; versions of different groups do not affect each other's fallback.
- Resolved output parses as a valid `[steam]` section through `respak-host`.

## Built
`crates/respak-web/src/catalog.rs` with the TOML model of webui.md §5a, strict parsing (errors name the file, and the line when the TOML parser has one), merge of shipped + user files by id, and resolution. API: `GET /api/catalog`, `POST /api/catalog/resolve`. The picker is in the Steam settings.
- Decided: user files are `.respak/catalog/*.toml` next to the settings; a broken user file is reported and the others still load; groups are for display only (the fallback order is by date across all versions, ties by file order).
- Shipped data: `catalog/apex.toml` and `catalog/titanfall2.toml` (one file per game; every `catalog/*.toml` is embedded by `rust-embed`) have Apex Legends (app 1172470) and Titanfall 2 (app 1237970, **no versions** yet). Apex has 24 season launches (S7-S30) and 34 events from the wiki's Event page (S7-S20), 58 versions in all, with manifests from the SteamDB history pages of its 11 depots (1172471-1172479, 1311105, 3893181). A version's manifest is the last one seen before 20:00 UTC on its date (the launch patch, not a later hotfix). S1-S6 and the events before the Steam release (2020-11-03) are left out. S22-S30 are not on the wiki page: their dates are the Tuesday launches that match a depot 1172471 manifest the same day. Events of S21+ have no dates on the wiki and are not listed. Hotfixes are not listed. The other depots are the nine language packs (French to Spanish) and 3893181, EA Anti-Cheat; none is default. The unused Arabic depot 1311106 has no manifests and is not listed.
- Tests: parse errors, merge, resolution (fallback, left out, defaults, ties), a resolved version parses as a `[steam]` section through `respak-host`, the API.
