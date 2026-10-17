//! The rpak container on synthetic paks (versions 6, 7, 8).

#[path = "../common/mod.rs"]
mod common;

use common::*;
use respak_core::{DiscoverOptions, ErrorKind};
use respak_respawn::{string_to_guid, PakHeader, Tables};

fn assets() -> Vec<TestAsset> {
    vec![
        TestAsset::new(0x1111_2222_3333_4444, b"txtr", 10, b"TEXTURE-HEADER-BYTES"),
        TestAsset::new(0xAAAA_BBBB_CCCC_DDDD, b"matl", 15, b"material header"),
        TestAsset::new(0x0000_0000_0000_00FF, b"ui\0\0", 2, b"ui!"),
    ]
}

fn depth(n: u32) -> DiscoverOptions {
    DiscoverOptions {
        depth: n,
        ..Default::default()
    }
}

mod lists;
mod lookup;
mod real;
