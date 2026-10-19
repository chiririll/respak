# msnp · Material snapshot
> **Verified against** Apex `paks/Win64/common_early.rpak` (48 assets) and `mp_lobby.rpak` (62 assets): version 1, header size 0x40, field offsets, the packed blend-state decode, the flag words and the `shaderSet` guid.
**rsx sources:** `game/rtech/assets/material_snapshot.{h,cpp}` (`MaterialSnapshotAssetHeader_v1_t`, `LoadMaterialSnapshotAsset`, `PostLoadMaterialSnapshotAsset`, `PreviewMaterialSnapshotAsset`), `material.h` (`MaterialDXState_t`, `MaterialBlendState_t`, `DF_*` / `RF_*` flags), `material.cpp` (`MatPreview_DXState`) · **Games / versions:** Apex Legends (not in Titanfall 2), asset version 1 only · **Priority hint:** low, S. Inspection only: no export in rsx. Needed together with `matl` v22+ (`matl.md`, layout B stores the snapshot guid at 0x08).

## Summary
A shared set of Direct3D pipeline state (blend, depth/stencil, rasterizer) plus a shader set guid, used by materials that reference a snapshot instead of carrying their own DX state. rsx only shows it in the UI; it has no export. A user would convert it to JSON.

## Where it lives
rpak asset, FourCC `msnp` (rsx binding `'pnsm'`), header alignment 16 (the header is 0x40 bytes). rsx path prefix `material_snapshot` (`game/asset.h:181`). No name field: `SetAssetNameFromCache()` (external guid cache), otherwise `<guid>.msnp`. Referenced from materials by guid (`matl` snapshot guid).

## Versions and detection
Only version 1. rsx asserts `headerStructSize == sizeof(MaterialSnapshotAssetHeader_v1_t)` (0x40) and rejects other versions. Proposed: score 10 for kind `msnp`; `Version { major: asset.version, minor: 0 }`; version 1 with header size 0x40 accepted, else `UnsupportedVersion { msnp, <v> }`.

## Layout
Little-endian. `MaterialSnapshotAssetHeader_v1_t`, 0x40 bytes, no pointers:

| Off | Size | Type | Field | Meaning |
|---|---|---|---|---|
| 0x00 | 0x20 | `u32[8]` | `blendStates` | one packed `MaterialBlendState_t` per render target (8 = `D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT`) |
| 0x20 | 4 | u32 | `blendStateMask` | meaning unconfirmed (probably which render targets are active) |
| 0x24 | 2 | u16 | `depthStencilFlags` | `DF_*` bits below |
| 0x26 | 2 | u16 | `rasterizerFlags` | `RF_*` bits below |
| 0x28 | 8 | bytes | `unk_28` | "there is data here in newer versions!" (unknown; rsx comment) |
| 0x30 | 8 | u64 | `shaderSet` | guid of a `shds` asset |
| 0x38 | 8 | u64 | `unk_38` | unknown |

`MaterialDXState_t` is the first 0x30 bytes (rsx type is `align(16)`, `static_assert(sizeof == 0x30)`). The v12 material variant has the same prefix with only 4 blend states (0x20 bytes) and no `unk_28`; the snapshot always uses the 8-state form.

Packed blend state (u32, MSVC bitfields from the least significant bit):

| Bits | Field | Value |
|---|---|---|
| 0 | `unk` | unknown |
| 1 | `blendEnable` | bool |
| 2-6 | `srcBlend` | `D3D11_BLEND - 1` (add 1 to get the D3D11 value) |
| 7-11 | `destBlend` | `D3D11_BLEND - 1` |
| 12-14 | `blendOp` | `D3D11_BLEND_OP - 1` |
| 15-19 | `srcBlendAlpha` | `D3D11_BLEND - 1` |
| 20-24 | `destBlendAlpha` | `D3D11_BLEND - 1` |
| 25-27 | `blendOpAlpha` | `D3D11_BLEND_OP - 1` |
| 28-31 | `renderTargetWriteMask` | D3D11 write mask |

`depthStencilFlags` (u16, `D3D11_DEPTH_STENCIL_DESC` creation flags): bit0 `DF_DEPTH_ENABLE`; bits1-3 depth func (`LESS` 0b0010, `EQUAL` 0b0100, `GREATER` 0b1000, `LESS_EQUAL` 0b0110, `NOT_EQUAL` 0b1010, `GREATER_EQUAL` 0b1100, `ALWAYS` 0b1110, `NEVER` 0); bit4 `DF_DEPTH_WRITE_MASK_ALL` (0x10); bit7 `DF_STENCIL_ENABLE` (0x80). Stencil ops (commented in source, unverified): fail = `((f >> 8) & 7) + 1`, depth-fail = `((f >> 11) & 7) + 1`, pass = `((f >> 14) & 7) + 1`.

`rasterizerFlags` (u16): bit0 `RF_FILL_WIREFRAME`; bits1-2 cull (`NONE` 0b0010, `FRONT` 0b0100, `BACK` 0b0110); bit3 `RF_INVERT_FACE_DIR` (FrontCounterClockwise); presets in bits4-6 (value `(f & 0x70)`): `DECAL` 0x10, `SHADOWMAP` 0x20, `TIGHTSHADOWMAP` 0x40, `UI` 0x50, `ZFILL` 0x60, which select DepthBias / DepthBiasClamp / SlopeScaledDepthBias; bit7 `RF_SCISSOR_ENABLE` (0x80). Constants of the presets are not in rsx.

## Verified on real data
- All 110 snapshots: asset version 1, entry header size 64 (0x40).
- `shaderSet` (0x30) is the guid of a `shds` asset **in the same pak** in 110 of 110 cases.
- `depthStencilFlags` values seen: `0x17` (depth enable + LESS_EQUAL + write mask), `0x07`, `0x05` (EQUAL), `0x04`, `0x00`; `rasterizerFlags`: `0x06` (cull BACK), `0x02` (cull NONE), `0x16` (DECAL + BACK), `0x26`/`0x22` (SHADOWMAP + BACK/NONE), `0x46`/`0x42` (TIGHTSHADOWMAP + BACK/NONE). These match the bit layout above.
- Blend states decode sensibly with the `D3D11 - 1` encoding: `0xF0000000` = disabled, ZERO/ZERO/ADD, write mask 15; `0xF0138286` = enable, src ONE, dst INV_SRC_ALPHA, ADD, alpha INV_DEST_ALPHA/ONE, write mask 15 (premultiplied alpha); `0xF0138292` = src SRC_ALPHA; `0x80500002` = enabled, write mask 8 (alpha only). Targets 1-7 are normally copies of target 0 (when the snapshot is opaque) or a fixed set; unused states are all-zero.

## Data outside the header
None. The header is the whole asset (no CPU pages beyond it, no pointers, no stream).

## Dependencies
`shaderSet` (offset 0x30) is a `shds` asset guid; rsx resolves it with `FindAssetByGUID` in PostLoad and shows its name or "unloaded". Not needed for the converter output beyond the guid. Maps to `rpak.guid` kind `shds` (project.md 5.5), optional dependency. Reverse: materials point at the snapshot through their `snapshot` guid.

## Decoding
1. Check version 1 and header size 0x40.
2. Read the 0x40 bytes as above; split each `blendStates[i]` with shifts and masks; add 1 to the five enum fields to get D3D11 numbers.
3. Name the enums with the D3D11 tables (`D3D11_BLEND` 1..19, `D3D11_BLEND_OP` 1..5).
4. Keep raw values too (`unk`, `blendStateMask`, `unk_28`, `unk_38`).

## Output
rsx: none (export function is null, preview only: shader set, and the DX state tree printing raw blend value, flags and the two flag words in decimal). Proposed respak: `Metadata` (kind `msnp`, guid, dependency on the shader set) and a JSON edge `msnp -> msnp.json`:
```json
{ "version": 1, "shaderSet": "0x...", "blendStateMask": 0,
  "blendStates": [ { "raw": "0x..", "blendEnable": false, "srcBlend": "ONE", "destBlend": "ZERO", "blendOp": "ADD",
                      "srcBlendAlpha": "ONE", "destBlendAlpha": "ZERO", "blendOpAlpha": "ADD", "writeMask": 15 } ],
  "depthStencilFlags": 0, "rasterizerFlags": 0, "unk28": "0x..", "unk38": "0x.." }
```

## rsx quirks
- No name field; named from cache.
- The preview prints blend field strings with `[value + 1]` over the D3D11 names table (consistent with the minus-one encoding).
- `unk_28` is described as holding data in newer versions, but it is unread.
- `blendStateMask`'s meaning is not documented.
- Material flags (`DF_*`, `RF_*`) are defined in `material.h` and only partly decoded.

## Reuse in respak
`matl` (the snapshot guid, the DX-state helpers if respak has any: none known), `rpak.asset`, `rpak.guid`. New: header parser and enum naming tables (could be shared with a future `matl` JSON that also prints DX state).

## Tests
Synthetic: a 0x40 header with all-zero blend (enable false, writeMask 0), one with `renderTargetWriteMask = 0xF`, `srcBlend` = 1, `destBlend` = 5; flags `DF_DEPTH_ENABLE | LESS_EQUAL`; wrong header size and version 2 rejected. Real: `paks/Win64/common_early.rpak` and `paks/Win64/mp_lobby.rpak` (many `msnp`; also `ui.rpak`, `startup.rpak`, `common.rpak`).

## Open questions
1. Meaning of `blendStateMask` (seen 0, 1, 5), `unk_28` (seen `0, 1, 2, 3, 4, 0xF` as a little-endian u64) and `unk_38` (seen `0, 2, 0x200, 0x500`). The blend-state `unk` bit 0 was 0 in every state of all 110 snapshots, so its meaning is not observable here.
2. Whether respak should expose DX state as raw numbers or D3D11 names (suggest both; the D3D11 name decode below matches real data).
3. Is `msnp` worth a slice before `matl` JSON output of DX state?
