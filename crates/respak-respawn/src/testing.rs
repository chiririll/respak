//! Builders for synthetic `.rpak` files (versions 6, 7, 8, uncompressed), for tests in this and
//! other crates. No game files. Enabled by the `testing` feature.
#![allow(missing_docs, dead_code)]

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

// ---- textures -------------------------------------------------------------------------------

/// A raw asset: where its header and data live, as `(page, offset)`.
pub struct RawAsset {
    pub guid: u64,
    pub kind: [u8; 4],
    pub version: u32,
    pub head: (u32, u32),
    pub data: Option<(u32, u32)>,
    pub header_size: u32,
}

/// Builds a pak from explicit pages, a pointer table and raw asset entries (uncompressed).
pub fn build_raw(version: u16, pages: &[Vec<u8>], pointers: &[(u32, u32)], assets: &[RawAsset]) -> Vec<u8> {
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let entry_len = if version == 8 { 0x50 } else { 0x48 };
    let mut body = Vec::new();
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
        put(&mut e, 0x20, &(-1i64).to_le_bytes());
        if version == 8 {
            put(&mut e, 0x28, &(-1i64).to_le_bytes());
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
    for p in pages {
        body.extend_from_slice(p);
    }

    let total = (header_len + body.len()) as i64;
    let mut h = vec![0u8; header_len];
    put(&mut h, 0, b"RPak");
    put(&mut h, 4, &version.to_le_bytes());
    put(&mut h, 0x18, &total.to_le_bytes());
    let (np, nptr, na) = (pages.len() as u32, pointers.len() as i32, assets.len() as i32);
    match version {
        6 => {
            put(&mut h, 0x34, &1i32.to_le_bytes());
            put(&mut h, 0x38, &(np as i32).to_le_bytes());
            put(&mut h, 0x3C, &nptr.to_le_bytes());
            put(&mut h, 0x40, &na.to_le_bytes());
        }
        7 => {
            put(&mut h, 0x28, &total.to_le_bytes());
            put(&mut h, 0x3A, &1u16.to_le_bytes());
            put(&mut h, 0x3C, &(np as u16).to_le_bytes());
            put(&mut h, 0x40, &nptr.to_le_bytes());
            put(&mut h, 0x44, &na.to_le_bytes());
        }
        _ => {
            put(&mut h, 0x30, &total.to_le_bytes());
            put(&mut h, 0x4C, &1u16.to_le_bytes());
            put(&mut h, 0x4E, &(np as u16).to_le_bytes());
            put(&mut h, 0x54, &nptr.to_le_bytes());
            put(&mut h, 0x58, &na.to_le_bytes());
        }
    }
    h.extend_from_slice(&body);
    h
}

/// A texture to put in a pak.
#[derive(Clone)]
pub struct TexSpec {
    pub guid: u64,
    /// The asset (header) version: 8, 9 or 10.
    pub version: u32,
    pub name: Option<&'static str>,
    pub width: u16,
    pub height: u16,
    /// `eTextureFormat`.
    pub format: u16,
    pub array: u8,
    pub optional: u8,
    pub streamed: u8,
    pub permanent: u8,
    pub swizzle: u8,
    /// The CPU data bytes, exactly as the permanent mips are stored (smallest first).
    pub data: Vec<u8>,
}

impl TexSpec {
    pub fn new(guid: u64, version: u32, w: u16, h: u16, format: u16, permanent: u8, data: Vec<u8>) -> Self {
        Self {
            guid,
            version,
            name: None,
            width: w,
            height: h,
            format,
            array: 1,
            optional: 0,
            streamed: 0,
            permanent,
            swizzle: 0,
            data,
        }
    }

    /// The 56-byte header with the name pointer slot set to `{page 1, name_offset}` (or null).
    fn header(&self, name_offset: Option<u32>) -> Vec<u8> {
        let mut h = vec![0u8; 56];
        let slot = match name_offset {
            Some(o) => [1u32.to_le_bytes(), o.to_le_bytes()].concat(),
            None => [u32::MAX.to_le_bytes(), u32::MAX.to_le_bytes()].concat(),
        };
        if self.version == 8 {
            put(&mut h, 0, &self.guid.to_le_bytes());
            put(&mut h, 8, &slot);
            put(&mut h, 0x10, &self.width.to_le_bytes());
            put(&mut h, 0x12, &self.height.to_le_bytes());
            put(&mut h, 0x16, &self.format.to_le_bytes());
            h[0x1C] = self.swizzle;
            h[0x1D] = self.optional;
            h[0x1E] = self.array;
            h[0x21] = self.permanent;
            h[0x22] = self.streamed;
        } else {
            put(&mut h, 0, &slot);
            put(&mut h, 8, &self.format.to_le_bytes());
            put(&mut h, 0x0A, &self.width.to_le_bytes());
            put(&mut h, 0x0C, &self.height.to_le_bytes());
            h[0x10] = self.array;
            h[0x12] = self.swizzle;
            h[0x18] = self.permanent;
            h[0x19] = self.streamed;
            h[0x1A] = self.optional;
        }
        h
    }
}

/// A pak with `txtr` assets: page 0 holds the headers, page 1 the names and CPU data.
pub fn build_tex_pak(pak_version: u16, textures: &[TexSpec]) -> Vec<u8> {
    let mut headers = Vec::new();
    let mut misc = Vec::new();
    let mut pointers = Vec::new();
    let mut assets = Vec::new();
    for t in textures {
        let name_offset = t.name.map(|n| {
            let at = misc.len() as u32;
            misc.extend_from_slice(n.as_bytes());
            misc.push(0);
            while misc.len() % 16 != 0 {
                misc.push(0);
            }
            at
        });
        let data_offset = misc.len() as u32;
        misc.extend_from_slice(&t.data);
        while misc.len() % 16 != 0 {
            misc.push(0);
        }
        let head_offset = headers.len() as u32;
        headers.extend_from_slice(&t.header(name_offset));
        if name_offset.is_some() {
            pointers.push((0, head_offset + if t.version == 8 { 8 } else { 0 }));
        }
        assets.push(RawAsset {
            guid: t.guid,
            kind: *b"txtr",
            version: t.version,
            head: (0, head_offset),
            data: Some((1, data_offset)),
            header_size: 56,
        });
    }
    build_raw(pak_version, &[headers, misc], &pointers, &assets)
}
