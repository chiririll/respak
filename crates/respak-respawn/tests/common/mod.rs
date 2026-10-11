//! Builds synthetic `.rpak` files (versions 6, 7, 8, uncompressed) for tests. No game files.
#![allow(dead_code)]

use opendal::{services::Memory, Operator};
use respak_core::Engine;
use respak_respawn::RespawnPlugin;

/// One asset to put in a synthetic pak. Its `header` bytes go into the single page.
#[derive(Clone)]
pub struct TestAsset {
    pub guid: u64,
    pub kind: [u8; 4],
    pub version: u32,
    pub header: Vec<u8>,
}

impl TestAsset {
    pub fn new(guid: u64, kind: &[u8; 4], version: u32, header: &[u8]) -> Self {
        Self { guid, kind: *kind, version, header: header.to_vec() }
    }
}

/// Knobs for the odd cases.
#[derive(Default, Clone)]
pub struct Options {
    pub flags: u16,
    pub patch_count: i16,
    pub starpaks: Vec<String>,
    /// Added to every asset's header size field (to make it not fit its page).
    pub header_size_extra: u32,
}

fn put(b: &mut [u8], at: usize, bytes: &[u8]) {
    b[at..at + bytes.len()].copy_from_slice(bytes);
}

pub fn build_pak(version: u16, assets: &[TestAsset]) -> Vec<u8> {
    build_pak_with(version, assets, &Options::default())
}

pub fn build_pak_with(version: u16, assets: &[TestAsset], opts: &Options) -> Vec<u8> {
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let entry_len = if version == 8 { 0x50 } else { 0x48 };

    let mut starpaks = Vec::new();
    for s in &opts.starpaks {
        starpaks.extend_from_slice(s.as_bytes());
        starpaks.push(0);
    }
    let page: Vec<u8> = assets.iter().flat_map(|a| a.header.clone()).collect();

    let mut body = Vec::new();
    body.extend_from_slice(&starpaks);
    // one segment: flags, align, size
    body.extend_from_slice(&1i32.to_le_bytes());
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&(page.len() as i64).to_le_bytes());
    // one page: segment, align, size
    body.extend_from_slice(&0i32.to_le_bytes());
    body.extend_from_slice(&16i32.to_le_bytes());
    body.extend_from_slice(&(page.len() as u32).to_le_bytes());
    // no pointers; the asset table
    let mut offset = 0u32;
    for a in assets {
        let mut e = vec![0u8; entry_len];
        put(&mut e, 0, &a.guid.to_le_bytes());
        put(&mut e, 0x10, &0u32.to_le_bytes());
        put(&mut e, 0x14, &offset.to_le_bytes());
        put(&mut e, 0x18, &u32::MAX.to_le_bytes());
        put(&mut e, 0x1C, &u32::MAX.to_le_bytes());
        put(&mut e, 0x20, &(-1i64).to_le_bytes());
        let size = a.header.len() as u32 + opts.header_size_extra;
        if version == 8 {
            put(&mut e, 0x28, &(-1i64).to_le_bytes());
            put(&mut e, 0x44, &size.to_le_bytes());
            e[0x48] = a.version as u8;
            put(&mut e, 0x4C, &a.kind);
        } else {
            put(&mut e, 0x3C, &size.to_le_bytes());
            put(&mut e, 0x40, &a.version.to_le_bytes());
            put(&mut e, 0x44, &a.kind);
        }
        body.extend_from_slice(&e);
        offset += a.header.len() as u32;
    }
    body.extend_from_slice(&page);

    let total = (header_len + body.len()) as i64;
    let mut h = vec![0u8; header_len];
    put(&mut h, 0, b"RPak");
    put(&mut h, 4, &version.to_le_bytes());
    put(&mut h, 6, &opts.flags.to_le_bytes());
    put(&mut h, 0x18, &total.to_le_bytes());
    let n_assets = assets.len() as i32;
    match version {
        6 => {
            put(&mut h, 0x30, &(starpaks.len() as i32).to_le_bytes());
            put(&mut h, 0x34, &1i32.to_le_bytes());
            put(&mut h, 0x38, &1i32.to_le_bytes());
            put(&mut h, 0x40, &n_assets.to_le_bytes());
        }
        7 => {
            put(&mut h, 0x28, &total.to_le_bytes());
            put(&mut h, 0x38, &(starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x3A, &1u16.to_le_bytes());
            put(&mut h, 0x3C, &1u16.to_le_bytes());
            put(&mut h, 0x3E, &opts.patch_count.to_le_bytes());
            put(&mut h, 0x44, &n_assets.to_le_bytes());
        }
        _ => {
            put(&mut h, 0x30, &total.to_le_bytes());
            put(&mut h, 0x48, &(starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x4C, &1u16.to_le_bytes());
            put(&mut h, 0x4E, &1u16.to_le_bytes());
            put(&mut h, 0x50, &opts.patch_count.to_le_bytes());
            put(&mut h, 0x58, &n_assets.to_le_bytes());
        }
    }
    h.extend_from_slice(&body);
    h
}

pub struct Env {
    pub engine: Engine,
    pub input: Operator,
}

pub fn env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder().input(input.clone()).output(output).plugin(RespawnPlugin::new()).build().unwrap();
    Env { engine, input }
}
