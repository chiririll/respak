# uiia · UI image (tiled)
**rsx sources:** `game/rtech/assets/ui_image.{h,cpp}` (`UIImageAssetHeader_v2_t`, `UIImageAssetData_v2_t`, `LoadUIImageAsset`, `DoTilingWork`, `CreateTextureForImage`) · **Games / versions:** Apex Legends, asset version 2 only · **Priority hint:** built (S/M). This spec describes what `crates/respak-respawn/src/assets/uiia/` does.

## Summary
One tiled UI image (not an atlas; that is `uimg`). Two resolutions: low in the pak's pages, high in a starpak. respak decodes one of them to an RGBA8 bitmap and can write it, with a JSON description, as a folder.

## Where it lives
rpak asset, FourCC `uiia` (rsx binding `'aiiu'`), extension `uiia`, pak name `<guid>.uiia` unless the stored name is found (`ui_image/<name>`; respak prefixes `ui_image/` when the stored name lacks it). The name is read from the CPU data (`0x30`); if absent the asset stays unnamed. High resolution: starpak entry (`rpak.asset.stream`).

## Versions and detection
`UiiaRead::score`: kind `uiia` = 10, `Version::new(asset.version)`. `UiiaHeader::parse` accepts version 2 only; anything else is `UnsupportedVersion { uiia, <v> }`. Format id `uiia@2`.

## Layout
Little-endian. Asset header 0x40 bytes (relocated), plus 0x40 bytes of CPU data in `.mem` (`data_mem_offset`). Fields respak reads:

| Where | Off | Type | Meaning |
|---|---|---|---|
| header | 0x10 | u16 | streamed size in 4096-byte units (`<< 12` = bytes; 0 = no stream) |
| header | 0x12 | u8 | flags (below) |
| header | 0x20 | f32[8] | four x/y pairs: origin, extent (fraction of canvas), texel offset (`-0.5/(size-1)`), scale (`canvas/(size-1)`) |
| CPU | 0x20 | u16 x2 | high resolution width, height |
| CPU | 0x24 | u16 x2 | low resolution width, height |
| CPU | 0x28 | page ptr | tile data (low resolution) |
| CPU | 0x30 | page ptr | name (NUL string) |

Flags (u8 at header 0x12): bit0 low has BC1 table, bit1 low has BC7 table, bit2 high BC1 table, bit3 high BC7 table, bits4-5 codec of the streamed data (0 none, 1 RTech, 2 Snowflake, 3 Oodle), bit6 / bit7 = no extra pixel in width / height. rsx also names header `0x18/0x1A` (width, height) and unknown bytes `0x00-0x0F`, `0x14-0x17`, `0x1C`: not read by respak (meaning unknown). CPU `0x38` is zero padding.

Tiles: 32x32 px of which 31x31 are visible (the border pixel is shared). `off` = 2, or 0 when the width / height bit is set; tiles per axis = `(n + off + 29) / 31`. A tile is 512 bytes (BC1) or 1024 (BC7). The table (only when both table bits of that resolution are set) is one u32 per tile: offset in the low 24 bits, opcode in the high 8: `0x40` BC1, `0x41` BC7, `0xC0` copy of tile `offset` (resolved against the entries before it), anything else transparent. Without a table tile `i` is at `i * size`, kind BC7 if that resolution's BC7 bit is set, else BC1. A zero dimension or more than `1<<18` tiles is `Corrupt`.

## Data outside the header
Low: `.mem` at the CPU data pointer, only as many bytes as the table / tiles reach (past the end of `.mem` is `Corrupt`). High: starpak entry, first `streamed_size` bytes, decompressed by the codec flag: RTech (`rtech::decompress`), Snowflake (`snowflake::decompress`), Oodle (`oodle::decompress(block, streamed_size)`). The table (if stored) is at the start of the data.

## Dependencies
None (`dependencies()` is empty). The starpak is the asset's stream path; if it is missing (`NotFound` / `MissingDependency`) or `streamed_size` is 0, a warning is issued and the low resolution is used; if the entry is smaller than `streamed_size` it is `Corrupt`.

## Decoding
1. Parse header + CPU; pick quality (High if the stream is there and big enough, else Low).
2. Load the tile data; build the tile list.
3. Per kind (BC1 / BC7): concatenate tiles in order (tiles of the other kind replaced by the transparent tile: BC1 block `00 00 FF FF FF FF FF FF`, BC7 block `10` + 15 zero bytes), `untile` (rsx `DoTilingWork`, a 3-stage block swizzle over the `w/4 x h/4` block grid; the width in blocks must be a multiple of 8), decode to RGBA8 (`respak_formats::decode_rgba8`).
4. If both kinds exist, BC7 tiles overwrite the BC1 image; then drop the shared border pixel of each tile and crop to `width x height`.

## Output
Capabilities: `Bitmap` (single mip, one layer, RGBA8; `read_mip` other than 0/0 is `NotFound`), `Metadata` (name, kind `uiia`, guid). Edge `uiia -> uiia.json` (docs/cli.md): a bundle with `uiia.json` and `image.<ext>` (option `image`: a target, default `png`; a bundle result goes to `image/`). JSON keys: `name`, `guid` (`0x` hex), `version`, `width`, `height` (of the image written), `quality` (`high` / `low`), `lowResolution` / `highResolution` (`width`, `height`, `tilesX`, `tilesY`), `streamedSize`, `compression` (codec 0-3), `canvas` (`width`, `height`, `origin`, `extent`, `texelOffset`, `scale`; `null` if the scale floats are not positive; canvas size derived from the high resolution), `flags` (`0x..`), `image`.

## rsx quirks
rsx decodes Oodle / RTech streams to `streamedSize` bytes (likely a bug; respak does that for Oodle, RTech uses its own size; backlog). rsx does no bounds checks; respak maps them to `Corrupt`. rsx calls the eight floats "UVs?"; respak reads them as canvas placement (inferred, not confirmed).

## Reuse in respak
`rpak.asset` (`detect_kind`, `rpak_asset`, `mem_offset_of`), `txtr::read_name`, `matl::TextureTarget`, `respak_formats::decode_rgba8`, codecs `rtech` / `snowflake` / `oodle`.

## Tests
Unit (`uiia/tests.rs`): tile grid arithmetic, `untile` is a permutation, implied tables, stored table with copies, transparent image. Snowflake codec tests use real `common.rpak` uiia blocks. `tests/rpak/real.rs` (skipped without the rsx cache): `ui_main_menu.rpak` has one asset `0x35492C0539B480BC.uiia`, version 2. Gaps: no synthetic pak fixture, no test of the whole read edge or of `uiia.json`.

## Open questions
1. Oodle / RTech high resolution untested on real data; Snowflake multi-block path (over 16 MiB) untested (backlog). Header bytes `0x00-0x0F`, `0x14-0x17`, `0x1C` unknown; header width / height (`0x18`) vs the CPU dimensions never compared.
2. Add a `PakBuilder` fixture for the read edge and `uiia.json`.
