use super::*;

#[test]
fn tile_grid() {
    // No extra pixel bits: (n + 2 + 29) / 31.
    let r = Resolution::new(62, 31, 0);
    assert_eq!((r.tiles_x, r.tiles_y), (3, 2));
    let r = Resolution::new(62, 31, 0xC0);
    assert_eq!((r.tiles_x, r.tiles_y), (2, 1));
}

#[test]
fn untile_visits_every_block_once() {
    for (wb, hb) in [(8, 8), (16, 8), (32, 16), (16, 24)] {
        let buf: Vec<u8> = (0..wb * hb).flat_map(|i: u32| i.to_le_bytes()).collect();
        let out = untile(&buf, wb, hb, 4).unwrap();
        let mut seen: Vec<u32> = out
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        seen.sort_unstable();
        assert!(
            seen.iter().copied().eq(0..wb * hb),
            "{wb}x{hb} is not a permutation"
        );
    }
}

#[test]
fn implied_tables() {
    let mut h = UiiaHeader {
        version: 2,
        flags: 0b10, // low BC7, no stored table
        streamed_size: 0,
        low: Resolution::new(31, 31, 0xC0),
        high: Resolution::new(31, 31, 0xC0),
        layout: [0.0; 8],
    };
    let res = h.low;
    let t = tiles(&h, Quality::Low, &res, &[]).unwrap();
    assert_eq!(
        t,
        [Tile {
            offset: 0,
            opcode: BC7_OP
        }]
    );
    h.flags = 0;
    assert_eq!(
        tiles(&h, Quality::Low, &res, &[]).unwrap()[0].opcode,
        BC1_OP
    );
}

#[test]
fn stored_table_copies_tiles() {
    let h = UiiaHeader {
        version: 2,
        flags: 0b11,
        streamed_size: 0,
        low: Resolution::new(62, 31, 0xC0),
        high: Resolution::new(62, 31, 0xC0),
        layout: [0.0; 8],
    };
    let mut data = Vec::new();
    data.extend_from_slice(&(BC1_OP << 24 | 8).to_le_bytes());
    data.extend_from_slice(&(COPY_OP << 24).to_le_bytes());
    let t = tiles(&h, Quality::Low, &h.low, &data).unwrap();
    assert_eq!(
        t[1],
        Tile {
            offset: 8,
            opcode: BC1_OP
        }
    );
    assert_eq!(needed_len(&h, Quality::Low, &h.low, &t), 8 + 512);
}

#[test]
fn transparent_image() {
    let res = Resolution::new(31, 31, 0xC0);
    let tiles = [Tile {
        offset: 0,
        opcode: BC1_OP,
    }];
    // A BC1 tile of all-transparent blocks: 512 bytes of the pattern.
    let data: Vec<u8> = [0u8, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF].repeat(64);
    let rgba = compose(&res, &tiles, &data).unwrap();
    assert_eq!(rgba.len(), 31 * 31 * 4);
    assert!(rgba.chunks_exact(4).all(|p| p[3] == 0));
}
