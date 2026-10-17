use crate::bytes::put;

/// A raw asset: where its header and data live, as `(page, offset)`.
pub struct RawAsset {
    pub guid: u64,
    pub kind: [u8; 4],
    pub version: u32,
    pub head: (u32, u32),
    pub data: Option<(u32, u32)>,
    pub header_size: u32,
    /// Starpak reference (`-1` = none): `index | offset`, offset 4096-aligned.
    pub starpak: i64,
    pub opt_starpak: i64,
}

/// Builds a pak from explicit pages, a pointer table and raw asset entries (uncompressed).
pub fn build_raw(
    version: u16,
    pages: &[Vec<u8>],
    pointers: &[(u32, u32)],
    assets: &[RawAsset],
) -> Vec<u8> {
    build_raw_with(version, pages, pointers, assets, &[], &[])
}

/// Like [`build_raw`], with the pak's starpak path lists (stored as given, so Windows-style
/// paths can be tested).
pub fn build_raw_with(
    version: u16,
    pages: &[Vec<u8>],
    pointers: &[(u32, u32)],
    assets: &[RawAsset],
    starpaks: &[&str],
    opt_starpaks: &[&str],
) -> Vec<u8> {
    build_raw_inner(
        version,
        pages,
        pointers,
        assets,
        starpaks,
        opt_starpaks,
        None,
    )
}

/// The patch part of a patch file: which pages it does not store, which files are below it, and
/// the edit stream (see [`EditScript`]).
pub struct PatchSpec {
    /// The first this many pages are not stored in the file.
    pub patch_pages: u32,
    /// File index of each lower file: 0 is `<stem>.rpak`, n is `<stem>(nn).rpak`.
    pub indices: Vec<u16>,
    /// The edit stream.
    pub stream: Vec<u8>,
}

/// Builds a patch file: the tables are those of the final pak (`pages` are all its pages), but only
/// the pages from `patch.patch_pages` on are stored. Version 7 or 8.
pub fn build_patch_file(
    version: u16,
    pages: &[Vec<u8>],
    pointers: &[(u32, u32)],
    assets: &[RawAsset],
    patch: &PatchSpec,
) -> Vec<u8> {
    build_raw_inner(version, pages, pointers, assets, &[], &[], Some(patch))
}

fn build_raw_inner(
    version: u16,
    pages: &[Vec<u8>],
    pointers: &[(u32, u32)],
    assets: &[RawAsset],
    starpaks: &[&str],
    opt_starpaks: &[&str],
    patch: Option<&PatchSpec>,
) -> Vec<u8> {
    let list = |paths: &[&str]| {
        paths
            .iter()
            .flat_map(|p| p.bytes().chain([0]))
            .collect::<Vec<u8>>()
    };
    let (starpaks, opt_starpaks) = (
        list(starpaks),
        if version == 8 {
            list(opt_starpaks)
        } else {
            Vec::new()
        },
    );
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let entry_len = if version == 8 { 0x50 } else { 0x48 };
    let mut body = Vec::new();
    if let Some(patch) = patch {
        body.extend_from_slice(&(patch.stream.len() as i32).to_le_bytes());
        body.extend_from_slice(&(patch.patch_pages as i32).to_le_bytes());
        for _ in &patch.indices {
            body.extend_from_slice(&[0u8; 16]);
        }
        for i in &patch.indices {
            body.extend_from_slice(&i.to_le_bytes());
        }
    }
    body.extend_from_slice(&starpaks);
    body.extend_from_slice(&opt_starpaks);
    body.extend_from_slice(&1i32.to_le_bytes());
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&(pages.iter().map(Vec::len).sum::<usize>() as i64).to_le_bytes());
    for p in pages {
        body.extend_from_slice(&0i32.to_le_bytes());
        body.extend_from_slice(&16i32.to_le_bytes());
        body.extend_from_slice(&(p.len() as u32).to_le_bytes());
    }
    for (page, offset) in pointers {
        body.extend_from_slice(&page.to_le_bytes());
        body.extend_from_slice(&offset.to_le_bytes());
    }
    for a in assets {
        let mut e = vec![0u8; entry_len];
        put(&mut e, 0, &a.guid.to_le_bytes());
        put(&mut e, 0x10, &a.head.0.to_le_bytes());
        put(&mut e, 0x14, &a.head.1.to_le_bytes());
        let (dp, doff) = a.data.unwrap_or((u32::MAX, u32::MAX));
        put(&mut e, 0x18, &dp.to_le_bytes());
        put(&mut e, 0x1C, &doff.to_le_bytes());
        put(&mut e, 0x20, &a.starpak.to_le_bytes());
        if version == 8 {
            put(&mut e, 0x28, &a.opt_starpak.to_le_bytes());
            put(&mut e, 0x44, &a.header_size.to_le_bytes());
            e[0x48] = a.version as u8;
            put(&mut e, 0x4C, &a.kind);
        } else {
            put(&mut e, 0x3C, &a.header_size.to_le_bytes());
            put(&mut e, 0x40, &a.version.to_le_bytes());
            put(&mut e, 0x44, &a.kind);
        }
        body.extend_from_slice(&e);
    }
    if let Some(patch) = patch {
        body.extend_from_slice(&patch.stream);
    }
    for p in pages
        .get(patch.map_or(0, |p| p.patch_pages as usize)..)
        .unwrap_or_default()
    {
        body.extend_from_slice(p);
    }

    let total = (header_len + body.len()) as i64;
    let mut h = vec![0u8; header_len];
    put(&mut h, 0, b"RPak");
    put(&mut h, 4, &version.to_le_bytes());
    put(&mut h, 0x18, &total.to_le_bytes());
    let patch_count = patch.map_or(0, |p| p.indices.len() as i16);
    let (np, nptr, na) = (
        pages.len() as u32,
        pointers.len() as i32,
        assets.len() as i32,
    );
    match version {
        6 => {
            put(&mut h, 0x30, &(starpaks.len() as i32).to_le_bytes());
            put(&mut h, 0x34, &1i32.to_le_bytes());
            put(&mut h, 0x38, &(np as i32).to_le_bytes());
            put(&mut h, 0x3C, &nptr.to_le_bytes());
            put(&mut h, 0x40, &na.to_le_bytes());
        }
        7 => {
            put(&mut h, 0x28, &total.to_le_bytes());
            put(&mut h, 0x38, &(starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x3A, &1u16.to_le_bytes());
            put(&mut h, 0x3C, &(np as u16).to_le_bytes());
            put(&mut h, 0x3E, &patch_count.to_le_bytes());
            put(&mut h, 0x40, &nptr.to_le_bytes());
            put(&mut h, 0x44, &na.to_le_bytes());
        }
        _ => {
            put(&mut h, 0x30, &total.to_le_bytes());
            put(&mut h, 0x48, &(starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x4A, &(opt_starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x4C, &1u16.to_le_bytes());
            put(&mut h, 0x4E, &(np as u16).to_le_bytes());
            put(&mut h, 0x50, &patch_count.to_le_bytes());
            put(&mut h, 0x54, &nptr.to_le_bytes());
            put(&mut h, 0x58, &na.to_le_bytes());
        }
    }
    h.extend_from_slice(&body);
    h
}
