//! Tile table, de-swizzling and composition of the pixel data.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tile {
    pub(super) offset: u32,
    pub(super) opcode: u32,
}

pub(super) fn tile_size(opcode: u32) -> usize {
    if opcode == BC7_OP {
        BC7_TILE
    } else {
        BC1_TILE
    }
}

/// The tile table of `quality`. `data` is the pak's data or the decoded stream; the stored table
/// is at its start.
pub(super) fn tiles(
    header: &UiiaHeader,
    quality: Quality,
    res: &Resolution,
    data: &[u8],
) -> Result<Vec<Tile>> {
    let count = res.tiles() as usize;
    let mut out = Vec::with_capacity(count);
    if header.has_table(quality) {
        let table = data
            .get(..count * 4)
            .ok_or_else(|| Error::corrupt("uiia tile table runs past its data"))?;
        for c in table.chunks_exact(4) {
            let v = u32::from_le_bytes(c.try_into().expect("4 bytes"));
            out.push(Tile {
                offset: v & 0x00FF_FFFF,
                opcode: v >> 24,
            });
        }
        // A copy takes the (already resolved) tile it points at.
        for i in 0..count {
            if out[i].opcode == COPY_OP {
                out[i] = *out
                    .get(out[i].offset as usize)
                    .ok_or_else(|| Error::corrupt("uiia tile copies a tile that does not exist"))?;
            }
        }
    } else {
        let op = header.implied_op(quality);
        for i in 0..count {
            out.push(Tile {
                offset: (i * tile_size(op)) as u32,
                opcode: op,
            });
        }
    }
    Ok(out)
}

/// The bytes of `data` the tiles and the table of `quality` need.
pub(super) fn needed_len(
    header: &UiiaHeader,
    quality: Quality,
    res: &Resolution,
    tiles: &[Tile],
) -> u64 {
    let table = if header.has_table(quality) {
        u64::from(res.tiles()) * 4
    } else {
        0
    };
    tiles
        .iter()
        .filter(|t| t.opcode == BC1_OP || t.opcode == BC7_OP)
        .map(|t| u64::from(t.offset) + tile_size(t.opcode) as u64)
        .max()
        .unwrap_or(0)
        .max(table)
}

/// Puts the blocks of `buf` (in the order the tiles lay them out) at their swizzled places
/// (rsx `DoTilingWork`). `wb` x `hb` is the image in 4x4 blocks of `bytes` bytes.
pub(super) fn untile(buf: &[u8], wb: u32, hb: u32, bytes: usize) -> Result<Vec<u8>> {
    let (wb, hb) = (i64::from(wb), i64::from(hb));
    if wb % 8 != 0 || buf.len() as i64 != wb * hb * bytes as i64 {
        return Err(Error::corrupt("uiia tiles do not fill a whole image"));
    }
    // One step of the swizzle: where block `t` of rows of `w` goes (rsx `b_N`).
    let step = |t: i64, w: i64| (t % (2 * w)) / 2 + w * ((t % (2 * w)) % 2) + 2 * w * (t / (2 * w));
    let mut out = vec![0u8; buf.len()];
    let mut at = 0usize;
    for y in 0..hb {
        for x in 0..wb {
            let (mut mx, mut my) = (x, y);

            let w2 = wb / 2;
            let b2 = step(mx / 2 + my * w2, w2);
            let mut c2 = mx % 2 + 2 * (b2 % w2);
            mx = b2 / w2;
            my = c2 / 4;
            c2 %= 4;

            let w4 = wb / 4;
            let b4 = step(my + mx / 2 * w4, w4);
            let mut c4 = mx % 2 + 2 * (b4 / w4);
            // rsx also sets `mx = b4 / w4` here; the last stage overwrites it unread.
            my = c4 / 4;
            c4 %= 4;

            let w8 = wb / 8;
            let b8 = step((c2 + 4 * (b4 % w4)) / 8 + my * w8, w8);
            my = c4 + 4 * (b8 / w8);
            mx = (c2 + 4 * (b4 % w4)) % 8 + 8 * (b8 % w8);

            let dst = (my * wb + mx) as usize * bytes;
            let block = buf
                .get(at..at + bytes)
                .ok_or_else(|| Error::corrupt("uiia tiles are cut short"))?;
            out.get_mut(dst..dst + bytes)
                .ok_or_else(|| Error::corrupt("uiia swizzle leaves the image"))?
                .copy_from_slice(block);
            at += bytes;
        }
    }
    Ok(out)
}

/// Decodes all tiles of one kind to an RGBA8 image of `tiles_x * 32` x `tiles_y * 32`. Tiles of
/// another kind are the transparent tile of this one.
pub(super) fn decode_kind(
    res: &Resolution,
    tiles: &[Tile],
    data: &[u8],
    opcode: u32,
) -> Result<Vec<u8>> {
    let size = tile_size(opcode);
    // Transparent tile: BC1 `0000 FFFFFFFF` blocks, or BC7 mode 4 blocks (rsx `UIImageTileBc1/7`).
    let block: &[u8] = if opcode == BC7_OP {
        &[0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    } else {
        &[0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    };
    let mut buf = Vec::with_capacity(tiles.len() * size);
    for t in tiles {
        if t.opcode == opcode {
            let at = t.offset as usize;
            buf.extend_from_slice(
                data.get(at..at + size)
                    .ok_or_else(|| Error::corrupt("uiia tile runs past its data"))?,
            );
        } else {
            for _ in 0..size / block.len() {
                buf.extend_from_slice(block);
            }
        }
    }
    let (w, h) = (res.tiles_x * TILE_PX, res.tiles_y * TILE_PX);
    let swizzled = untile(&buf, w / 4, h / 4, block.len())?;
    let format = if opcode == BC7_OP {
        PixelFormat::Bc7
    } else {
        PixelFormat::Bc1
    };
    decode_rgba8(format, w, h, &swizzled)
}

/// The finished RGBA8 image of one resolution (rsx `CreateTextureForImage`).
pub(super) fn compose(res: &Resolution, tiles: &[Tile], data: &[u8]) -> Result<Vec<u8>> {
    let has = |op| tiles.iter().any(|t| t.opcode == op);
    let (bc1, bc7) = (
        has(BC1_OP).then(|| decode_kind(res, tiles, data, BC1_OP)),
        has(BC7_OP).then(|| decode_kind(res, tiles, data, BC7_OP)),
    );
    let stride = (res.tiles_x * TILE_PX) as usize * 4;
    let shifted = match (bc1, bc7) {
        (None, None) => return Err(Error::corrupt("uiia has no tiles")),
        (Some(bc1), None) => bc1?,
        (None, Some(bc7)) => bc7?,
        (Some(bc1), Some(bc7)) => {
            let (mut base, bc7) = (bc1?, bc7?);
            for i in tiles
                .iter()
                .enumerate()
                .filter(|(_, t)| t.opcode == BC7_OP)
                .map(|(i, _)| i)
            {
                let (x, y) = (i as u32 % res.tiles_x, i as u32 / res.tiles_x);
                for row in 0..TILE_PX {
                    let at = ((y * TILE_PX + row) as usize) * stride + (x * TILE_PX) as usize * 4;
                    base[at..at + TILE_PX as usize * 4]
                        .copy_from_slice(&bc7[at..at + TILE_PX as usize * 4]);
                }
            }
            base
        }
    };
    // Drop the shared border pixel of every tile.
    let mut out = vec![0u8; res.width as usize * res.height as usize * 4];
    for y in 0..res.tiles_y {
        let size_y = if y * VISIBLE_PX + 30 < res.height {
            VISIBLE_PX
        } else {
            res.height.saturating_sub(y * VISIBLE_PX)
        };
        for x in 0..res.tiles_x {
            let size_x = if x * VISIBLE_PX + 30 < res.width {
                VISIBLE_PX
            } else {
                res.width.saturating_sub(x * VISIBLE_PX)
            };
            for row in 0..size_y {
                let src = ((y * TILE_PX + row) as usize) * stride + (x * TILE_PX) as usize * 4;
                let dst = (((y * VISIBLE_PX + row) * res.width) + x * VISIBLE_PX) as usize * 4;
                out[dst..dst + size_x as usize * 4]
                    .copy_from_slice(&shifted[src..src + size_x as usize * 4]);
            }
        }
    }
    Ok(out)
}
