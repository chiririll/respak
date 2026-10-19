# rmap · Map asset (`rmap`) and light layer (`llyr`)
> **Verified against** Apex `paks/Win64/mp_lobby.rpak` (one `rmap`): asset **version 7**, header size **104 (0x68)**, the whole header is zero, and the name rule `map/<pak stem>.rmap` holds. `rmap` also exists in nine `mp_rr_*` paks and `mp_quicklighting_test.rpak` (listed, one each), but their page data could not be read (Steam login expired, chunks not cached). No `llyr` appears in any of the 115 non-cosmetic paks listed.
**rsx sources:** `game/rtech/assets/map.h` (`MapAssetHeader_v5_t`), `game/rtech/assets/map.cpp` (`LoadMapAsset`, `InitMapAssetType`), `game/asset.h:70-71, 151-152, 225-226` (FourCC and display-name tables), `core/main.cpp:97-101, 170-173` (registration) · **Games / versions:** Apex Legends rpak (the header struct is named `v5`; a v4 header is guessed to be smaller, unverified) · **Priority hint:** very low value, **S**. rsx only gives the asset a name; it parses nothing.

> **Scope warning.** rsx has almost nothing for these kinds. `rmap` has an opaque 104-byte header struct and a name-resolver only (no preview, no export, no post-load). `llyr` has a FourCC and a display name ("llayer") in `asset.h`, and the registration in `main.cpp` is a bare comment `// llyr` with no function: **`llyr` is not supported by rsx at all.** Everything beyond the 104-byte blob is unknown. Nothing below should be treated as a layout.

## Summary
`rmap` is the per-map asset in a map's rpak (the one that ties the level's pages together). `llyr` is, by its name, a "light layer" (lighting layer for a map); its content is entirely unknown to rsx. A user could at best get the raw header/page bytes as a blob. There is no meaningful conversion target.

## Where it lives
- **rpak asset**, FourCC `rmap` (`AssetType_t::RMAP`, `asset.h:70`) and `llyr` (`AssetType_t::LLYR`, `asset.h:71`). The rpak type id is the little-endian u32 of the FourCC; rsx writes the `rmap` registration as `'pamr'` (`map.cpp:22`, a multi-char literal, i.e. the bytes `r m a p` in file order).
- Display names: `rmap` = "map", `llyr` = "llayer" (`asset.h:225-226`); the binding name is "Map".
- Name in the rpak volume: `map/<pak stem>.rmap` (`map.cpp:12`), where `<pak stem>` is the rpak file name without extension (e.g. `mp_rr_canyonlands_64k_x_128k`; the `.rpak`/patch suffix handling is `CPakFile::getPakStem`, `cpakfile.h:906`). The name is **not stored**: rsx builds this string, hashes it with `RTech::StringToGuid` and accepts it only if the hash equals the asset GUID (guess-and-verify). On mismatch the asset stays unnamed (shown by guid). For `llyr`: no name rule exists (unknown).
- No streamed data known. Header alignment 8 (`headerAlignment = 8`, `map.cpp:24`).

## Versions and detection
- rsx has **no version switch** for `rmap`. The struct is called `MapAssetHeader_v5_t` and carries a comment "todo: check if v4 header is smaller ?" (`map.h:3`). So at least asset versions 4 and 5 are believed to exist; how rsx would distinguish them is not implemented.
- Detection in respak: by rpak asset FourCC and the asset's own version number (rpak `rpak.asset` header). Proposed score: FourCC `rmap` → 10, header length == 104 → +2 (real data: version 7, so do not require version 5). Mapping `Version { major: asset_version, minor: 0 }`. Header of 104 bytes → known size (versions 5 and 7 seen/expected); any other size → `UnsupportedVersion` (do not guess v4).
- `llyr`: no version information (unknown).

## Layout
### `MapAssetHeader_v5_t` (`map.h:4-10`, 104 bytes = 0x68, `static_assert`ed)
| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 88 | u8[88] | unk_0 | unknown; may itself contain pointers/counts (rsx does not know) |
| 0x58 | 8 | page ptr | unk_58 | unknown pointer (rsx types it `void*`); a page pointer relocated by the rpak volume |
| 0x60 | 8 | page ptr | unk_60 | unknown pointer, same |

The page pointers at 0x58/0x60 are the only fields known to be pointers; where they point and what is behind them is unknown. Nothing is dereferenced anywhere in rsx.

### `llyr`
No struct. Unknown.

## Verified on real data
`mp_lobby.rpak` `rmap` `0xB64650BB27D57FE9`: version **7** (rsx's struct is named v5, so v5 and v7 both exist or at least v7 is current), header size 0x68 (matches the struct), **all 104 bytes zero**, including the two pointer slots (they are plain zeros, not relocated; a pointer slot that is non-null would have been rewritten to a `.mem` offset). `string_to_guid("map/mp_lobby.rmap")` equals the asset guid, so the name rule is correct. No CPU data (`data` page) and no stream. For `mp_lobby` the `rmap` is therefore an empty marker asset. Detection should accept version 7 with a 104-byte header, not only version 5.

## Data outside the header
None known for either kind. Pages referenced by `unk_58`/`unk_60` are ordinary rpak pages (no starpak known). Compression: none known (rpak-level compression only, see the rpak container).

## Dependencies
None resolved by rsx. A map asset presumably refers to the level's other assets (unknown). The only link is the name rule `map/<pak stem>.rmap` → `rpak.guid`.

## Decoding
```
rmap:
  name = "map/" + pak_stem + ".rmap"
  if guid(name) == asset.guid: asset.name = name       // else leave unnamed
  header bytes are not parsed
llyr:
  nothing
```
That is the entire rsx behaviour. A respak implementation can offer only: the named asset plus the 104-byte header and the raw page contents it points to as opaque data.

## Output
rsx: **none.** `ExportFunc` is null (`.e = { nullptr, 0, nullptr, 0ull }`), no preview, no post-load.
Proposed respak: either nothing, or a raw dump target (`Blob` of the header, optional `pages/` volume) so a user can inspect the bytes. A real conversion is not possible without reverse engineering beyond rsx. JSON shape: none.

## rsx quirks
- `unk_0` blob and the two pointers are never read; the struct exists only so `static_assert(sizeof == 104)` documents the size.
- The name rule only works when the pak stem equals the map name; patched/renamed paks would fail the hash check.
- `llyr` is declared in `AssetType_t` and has a comment placeholder in `main.cpp`, no implementation; the `rmap` row in the colour table is commented out (`asset.h:151`).
- `.type = 'pamr'` uses a multi-char literal (compiler/endian specific); respak should use its FourCC helper.
Keep: the name rule. Drop: everything else (nothing to port).

## Reuse in respak
`rpak.asset`, `rpak.guid` (to verify the name), page pointer resolution of the rpak volume (for the two pointers). New: nothing unless a target is chosen.

## Tests
- Synthetic: a 104-byte header of zeros plus two zero page pointers; name test with pak stem `mp_rr_test` → `map/mp_rr_test.rmap` accepted only when its guid equals the asset guid.
- Edge: header shorter than 104 (→ `Corrupt`/`UnsupportedVersion`), wrong stem (unnamed).
- Real: `paks/Win64/mp_lobby.rpak` (`rmap`, version 7, all-zero header). No `llyr` sample.

## Open questions
1. Is it worth shipping a raw-dump target at all, or skip `rmap`/`llyr`? (Evidence for skipping: the only `rmap` that could be read has an all-zero header.)
2. The 88-byte prefix and the two pointers are all zero in the one sample read (`mp_lobby`); real map paks (`mp_rr_*`) hold `rmap` too, but they could not be read (see below), so whether the header is ever non-zero is unknown. v4/v5 header sizes, and any `llyr` layout, are unverified (no `llyr` was found in any pak listed).
3. How `rmap` relates to the BSP/wrap lumps of the same pak ([bsp.md](bsp.md)).
