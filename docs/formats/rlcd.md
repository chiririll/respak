# rlcd · LCD screen effect
> **Verified against** Apex `paks/Win64/common_early.rpak` (3 assets, version 0, header size 48): layout, size and version.
**rsx sources:** `game/rtech/assets/lcd_screen_effect.{h,cpp}` (`LcdScreenEffect_v0_s`, `LcdScreenEffect_s`, `ExportLcdScreenEffectAsset` :5, `InitLcdScreenEffectAssetType` :83), `game/asset.h` (`AssetType_t::RLCD` :49, path name `lcd_screen_effect` :202), `docs/assets/rpak/LcdScreenEffect.md` (empty stub) · **Games / versions:** Apex Legends (asset version 0 only; Titanfall 2 unknown) · **Priority hint:** low value, trivial size (**S**): one 48-byte header, no dependencies, JSON output.

## Summary
A set of shader parameters for the LCD/CRT-like screen look (pixel scale, brightness, contrast, wave distortion, bloom, flicker), used for in-world screens. A user converts it to a small JSON file of named floats.

## Where it lives
- Container: rpak. FourCC stored `rlcd` (file order); rsx enum `MAKEFOURCC('r','l','c','d')` (`asset.h:49`) and the binding is registered reversed as `'dclr'` (`lcd_screen_effect.cpp:88`). Header alignment 4 (`:89`). Extension in respak: `<path>.rlcd`; rsx export path folder name `lcd_screen_effect`.
- Name: **not stored in the header and not set by rsx.** The binding has no `loadFunc` (`:90`), so rsx never calls `SetAssetName`; the export uses `asset->GetAssetName()` (`:44`), which without a name falls back to whatever the container provides (likely the guid, or a name from the pak's string list; not checked). Respak: name from guid-only fallback unless the pak provides one.
- Streamed data: none.

## Versions and detection
Only asset version **0** (`switch (pakAsset->version()) case 0`, `:13-15`). Anything else: warning "Unsupported version", `assert(0)`, return false (`:36-41`) -> `UnsupportedVersion`. Proposed: kind `rlcd` -> score 10, `Version { major: 0, minor: 0 }`; header size must be 48.

## Layout
`LcdScreenEffect_v0_s`, little-endian, natural alignment (no `static_assert` in rsx, offsets computed by hand), 48 bytes, no pointers:
| off | size | type | field |
|---|---|---|---|
| 0x00 | 4 | f32 | `pixelScaleX1` |
| 0x04 | 4 | f32 | `pixelScaleX2` |
| 0x08 | 4 | f32 | `pixelScaleY` |
| 0x0C | 4 | f32 | `brightness` |
| 0x10 | 4 | f32 | `contrast` |
| 0x14 | 4 | f32 | `waveOffset` |
| 0x18 | 4 | f32 | `waveScale` |
| 0x1C | 4 | f32 | `waveSpeed` |
| 0x20 | 4 | f32 | `wavePeriod` |
| 0x24 | 4 | f32 | `bloomAdd` |
| 0x28 | 1 | bool (u8) | `doBloomLuminance` (0 = false) |
| 0x29 | 3 | - | padding (alignment of the next f32) |
| 0x2C | 4 | f32 | `pixelFlicker` |
The meaning of each float beyond its name is unknown. **Verified:** the asset entry's header size is 48 in all three `rlcd` assets of `common_early.rpak`; the floats are plausible (e.g. `1.0, 2.0, 0.25, 0, 0, 0.005, 0.01, 0.3, 1.0, 3.0, bool 0, 0.1`), padding bytes at 0x29..0x2B are zero.

## Data outside the header
None. No pages beyond the header, no starpak, no compression.

## Dependencies
None.

## Decoding
Read 48 bytes at the asset header (reject if the entry's header size is smaller), read twelve fields at the offsets above, `doBloomLuminance = byte != 0`. Floats are copied as-is (NaN/inf possible; JSON needs a policy).

## Output
- rsx: `<exportdir>/<name parent>/<name stem>.json` (`:43-55`), settings ignored (`UNUSED(setting)`). Hand-built text, tab indent, floats with `{:f}` (six decimals, fixed), bool as `true`/`false`, key order as in the struct, no trailing newline:
```
{
	"pixelScaleX1": 0.000000,
	... (in struct order)
	"doBloomLuminance": true,
	"pixelFlicker": 0.000000
}
```
- Proposed respak: JSON via the existing text/JSON capability (no new capability). Prefer lossless float formatting (shortest round-trip) over `{:f}`, which loses precision; keys as above (camelCase, as rsx).

## rsx quirks
- `LcdScreenEffect_v0_s` and `LcdScreenEffect_s` are identical; the second is a pointless copy.
- Fixed six-decimal float output loses small values.
- Registered as `'dclr'` (FourCC reversed, the rsx convention); `loadFunc` is null so no name is set and nothing is validated.
- Unsupported version ends in `assert(0)` after the log (debug only).

## Reuse in respak
`rpak.asset` (header bytes) and the existing JSON writing used by other metadata kinds (e.g. `dtbl`/`stgs`). Everything else is new but trivial (a 12-field struct).

## Tests
- Synthetic: 48 bytes with distinct floats 1.0..10.0 at 0x00-0x24, `01` at 0x28, 0.5 at 0x2C; expect all twelve fields. Edge cases: header shorter than 48 -> `Corrupt`, version 1 -> `UnsupportedVersion`, bool byte 2, NaN float.
- Real files: `paks/Win64/common_early.rpak` (Apex, depot 1172471) holds 3 `rlcd` assets (e.g. `0x7B3C2E7568D28EB8`, `0x84A400890152FBFC`, `0xD041BC7AC2599ACD`), version 0, header 48 bytes.

## Open questions
- Name: none stored; unchanged (open).
- Titanfall 2 absent in rsx's docs; not checked.
- JSON float formatting: round-trip vs rsx's six decimals.
- `doBloomLuminance` was 0 in all three samples, so a non-zero value (and the padding bytes) are not observed.
