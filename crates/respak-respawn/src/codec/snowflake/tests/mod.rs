//! Unit tests; `vectors.txt` lists real blocks for the ignored test.

use super::simd::set32;
use super::*;

#[test]
fn small_streams_are_stored() {
    // Header: 6 bits n = 2, 2 bits = 2 (decoded size 5), then zero fields (7 + 4 + 4 + 2 + 3 bits).
    let mut stream = vec![130u8, 0, 0, 0];
    stream.extend_from_slice(b"hello");
    assert_eq!(decompress(&stream).unwrap(), b"hello");
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    for seed in 0..64u8 {
        let junk: Vec<u8> = (0..4096u32)
            .map(|i| (i as u8).wrapping_mul(seed | 1).wrapping_add(seed))
            .collect();
        let _ = decompress(&junk);
    }
}

#[test]
fn set32_orders_lanes_like_sse() {
    assert_eq!(
        set32(0x00070006, 0x00050004, 0x00030002, 0x00010000),
        [0, 1, 2, 3, 4, 5, 6, 7]
    );
}

/// Real blocks from Apex `common.rpak` (`uiia` tiles), captured before the module was split.
/// The inputs are game data and are not in the repo: set `SNOWFLAKE_VECTORS` to a directory of
/// `NNNN.in` files (the stored blocks) and run with `--ignored`.
#[test]
#[ignore = "needs SNOWFLAKE_VECTORS"]
fn real_blocks_decode_to_the_recorded_hash() {
    fn fnv(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        })
    }
    let dir = std::env::var("SNOWFLAKE_VECTORS").expect("SNOWFLAKE_VECTORS");
    let manifest = include_str!("vectors.txt");
    let mut n = 0;
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let mut f = line.split(' ');
        let (name, len, hash) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let input = std::fs::read(format!("{dir}/{name}.in")).unwrap();
        let out = decompress(&input).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(out.len(), len.parse::<usize>().unwrap(), "{name}: length");
        assert_eq!(format!("{:016x}", fnv(&out)), hash, "{name}: content");
        n += 1;
    }
    assert!(n > 0);
}
