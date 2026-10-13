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
    /// Starpak reference (`-1` = none): `index | offset`, offset 4096-aligned.
    pub starpak: i64,
    pub opt_starpak: i64,
}

/// Builds a pak from explicit pages, a pointer table and raw asset entries (uncompressed).
pub fn build_raw(version: u16, pages: &[Vec<u8>], pointers: &[(u32, u32)], assets: &[RawAsset]) -> Vec<u8> {
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
    let list = |paths: &[&str]| paths.iter().flat_map(|p| p.bytes().chain([0])).collect::<Vec<u8>>();
    let (starpaks, opt_starpaks) = (list(starpaks), if version == 8 { list(opt_starpaks) } else { Vec::new() });
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let entry_len = if version == 8 { 0x50 } else { 0x48 };
    let mut body = Vec::new();
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
            put(&mut h, 0x40, &nptr.to_le_bytes());
            put(&mut h, 0x44, &na.to_le_bytes());
        }
        _ => {
            put(&mut h, 0x30, &total.to_le_bytes());
            put(&mut h, 0x48, &(starpaks.len() as i16).to_le_bytes());
            put(&mut h, 0x4A, &(opt_starpaks.len() as i16).to_le_bytes());
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
    /// Codec bits and compressed sizes (v9/v10 only).
    pub codecs: u16,
    pub compressed: [u16; 7],
    /// Reference into the pak's mandatory starpak 0 (`streamed.starpak`) / optional starpak 0
    /// (`opt.starpak`): the 4096-aligned entry offset. `None` = no reference.
    pub stream_at: Option<u64>,
    pub opt_at: Option<u64>,
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
            codecs: 0,
            compressed: [0; 7],
            stream_at: None,
            opt_at: None,
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
            let at = if self.version == 9 { 0x1C } else { 0x1E };
            put(&mut h, at, &self.codecs.to_le_bytes());
            for (i, c) in self.compressed.iter().enumerate() {
                put(&mut h, at + 2 + 2 * i, &c.to_le_bytes());
            }
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
            starpak: t.stream_at.map_or(-1, |o| o as i64),
            opt_starpak: t.opt_at.map_or(-1, |o| o as i64),
        });
    }
    build_raw_with(
        pak_version,
        &[headers, misc],
        &pointers,
        &assets,
        &[r"paks\Win64\streamed.starpak"],
        &["paks/Win64/opt.starpak"],
    )
}

/// A starpak file: the entries at their offsets (zero-filled between), then the table of
/// contents and its count.
pub fn build_starpak(entries: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (offset, bytes) in entries {
        out.resize(*offset as usize, 0);
        out.extend_from_slice(bytes);
    }
    while out.len() % 4096 != 0 {
        out.push(0);
    }
    for (offset, bytes) in entries {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    }
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    out
}

// ---- Oodle ----------------------------------------------------------------------------------

/// An Oodle stream of stored (uncompressed) Kraken blocks. A block starts a new segment (restart
/// bit) when its index is a multiple of `restart_blocks`; 0 means only the first one does.
pub fn oodle_stored(data: &[u8], restart_blocks: usize) -> Vec<u8> {
    const BLOCK: usize = 0x40000;
    let mut out = Vec::new();
    for (i, chunk) in data.chunks(BLOCK).enumerate() {
        let restart = i == 0 || (restart_blocks != 0 && i % restart_blocks == 0);
        out.push(0x4C | if restart { 0x80 } else { 0 });
        out.push(0x06);
        out.extend_from_slice(chunk);
    }
    out
}

/// Turns a pak built uncompressed into an Oodle one: the body becomes stored blocks and the
/// header says so (`compressed size` is the body alone). The body is padded with zeros to
/// `pad_to` bytes first, to make several blocks.
pub fn oodle_pak(plain: &[u8], restart_blocks: usize, pad_to: usize) -> Vec<u8> {
    let version = u16::from_le_bytes([plain[4], plain[5]]);
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let mut body = plain[header_len..].to_vec();
    if body.len() < pad_to {
        body.resize(pad_to, 0);
    }
    let stream = oodle_stored(&body, restart_blocks);
    let mut out = plain[..header_len].to_vec();
    let flags = u16::from_le_bytes([out[6], out[7]]) | (1 << 9);
    put(&mut out, 6, &flags.to_le_bytes());
    put(&mut out, 0x18, &(stream.len() as i64).to_le_bytes());
    let decompressed = (header_len + body.len()) as i64;
    put(&mut out, if version == 8 { 0x30 } else { 0x28 }, &decompressed.to_le_bytes());
    out.extend_from_slice(&stream);
    out
}

// ---- general paks, materials, shaders ---------------------------------------------------------

/// Builds a pak with arbitrary assets: page 0 holds the headers, page 1 everything else.
#[derive(Default)]
pub struct PakBuilder {
    heads: Vec<u8>,
    misc: Vec<u8>,
    pointers: Vec<(u32, u32)>,
    assets: Vec<RawAsset>,
}

impl PakBuilder {
    /// An empty pak.
    pub fn new() -> Self {
        Self::default()
    }

    fn align(&mut self) {
        while !self.misc.len().is_multiple_of(16) {
            self.misc.push(0);
        }
    }

    /// Appends `bytes` to page 1 (16-byte aligned); returns their offset in the page.
    pub fn data(&mut self, bytes: &[u8]) -> u32 {
        self.align();
        let at = self.misc.len() as u32;
        self.misc.extend_from_slice(bytes);
        self.align();
        at
    }

    /// Appends a NUL-terminated string to page 1; returns its offset in the page.
    pub fn string(&mut self, s: &str) -> u32 {
        self.data(&[s.as_bytes(), &[0]].concat())
    }

    /// The `.mem` offset of page-1 offset `at`, once the pak is built (page 0 is the headers,
    /// so this is only valid after all assets are added: call it from tests via the built size).
    pub fn page1_at(&self, at: u32) -> u64 {
        self.heads.len() as u64 + u64::from(at)
    }

    /// Makes the 8 bytes at page-1 offset `slot` a pointer to page-1 offset `target`.
    pub fn data_pointer(&mut self, slot: u32, target: u32) {
        self.misc[slot as usize..slot as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        self.misc[slot as usize + 4..slot as usize + 8].copy_from_slice(&target.to_le_bytes());
        self.pointers.push((1, slot));
    }

    /// Adds an asset. `pointers` are `(slot offset in header, target offset in page 1)`; `data`
    /// is the page-1 offset of the asset's CPU data.
    pub fn asset(&mut self, guid: u64, kind: &[u8; 4], version: u32, mut header: Vec<u8>, pointers: &[(usize, u32)], data: Option<u32>) {
        for &(slot, target) in pointers {
            header[slot..slot + 4].copy_from_slice(&1u32.to_le_bytes());
            header[slot + 4..slot + 8].copy_from_slice(&target.to_le_bytes());
        }
        let head_offset = self.heads.len() as u32;
        for &(slot, _) in pointers {
            self.pointers.push((0, head_offset + slot as u32));
        }
        let header_size = header.len() as u32;
        self.heads.extend_from_slice(&header);
        while !self.heads.len().is_multiple_of(16) {
            self.heads.push(0);
        }
        self.assets.push(RawAsset {
            guid,
            kind: *kind,
            version,
            head: (0, head_offset),
            data: data.map(|d| (1, d)),
            header_size,
            starpak: -1,
            opt_starpak: -1,
        });
    }

    /// A `matl` of asset version 16 (240-byte header) with the given texture slots (0 = empty).
    pub fn matl(&mut self, guid: u64, name: &str, shader_set: u64, textures: &[u64]) {
        let mut h = vec![0u8; 240];
        put(&mut h, 0x10, &guid.to_le_bytes());
        put(&mut h, 0x58, &shader_set.to_le_bytes());
        put(&mut h, 0x72, &512u16.to_le_bytes());
        put(&mut h, 0x74, &256u16.to_le_bytes());
        put(&mut h, 0x76, &1u16.to_le_bytes());
        let name_at = self.string(name);
        let array: Vec<u8> = textures.iter().flat_map(|g| g.to_le_bytes()).chain([0; 8]).collect();
        let array_at = self.data(&array);
        self.asset(
            guid,
            b"matl",
            16,
            h,
            &[(0x18, name_at), (0x60, array_at), (0x68, array_at + 8 * textures.len() as u32)],
            None,
        );
    }

    /// A `shds` (asset version 12, 80-byte header) whose pixel shader is `pixel`.
    pub fn shds(&mut self, guid: u64, pixel: u64) {
        let mut h = vec![0u8; 80];
        put(&mut h, 0x48, &pixel.to_le_bytes());
        self.asset(guid, b"shds", 12, h, &[], None);
    }

    /// A `shdr` (asset version 12) whose first variant is `bytecode`.
    pub fn shdr(&mut self, guid: u64, bytecode: &[u8]) {
        let code_at = self.data(bytecode);
        let entry = vec![0u8; 16];
        let entry_at = self.data(&entry);
        self.data_pointer(entry_at, code_at);
        self.misc[entry_at as usize + 8..entry_at as usize + 12].copy_from_slice(&(bytecode.len() as i32).to_le_bytes());
        self.asset(guid, b"shdr", 12, vec![0u8; 32], &[], Some(entry_at));
    }

    /// The pak file.
    pub fn build(self, pak_version: u16) -> Vec<u8> {
        build_raw(pak_version, &[self.heads, self.misc], &self.pointers, &self.assets)
    }
}

/// A DXBC blob whose RDEF chunk declares `(name, resource type, bind point)`; type 2 is a texture.
pub fn dxbc_blob(resources: &[(&str, u32, u32)]) -> Vec<u8> {
    let table = 0x1C_u32;
    let names_at = table + 32 * resources.len() as u32;
    let mut rdef = vec![0u8; names_at as usize];
    rdef[8..12].copy_from_slice(&(resources.len() as u32).to_le_bytes());
    rdef[12..16].copy_from_slice(&table.to_le_bytes());
    let mut names = Vec::new();
    for (i, (name, ty, bind)) in resources.iter().enumerate() {
        let e = table as usize + i * 32;
        rdef[e..e + 4].copy_from_slice(&(names_at + names.len() as u32).to_le_bytes());
        rdef[e + 4..e + 8].copy_from_slice(&ty.to_le_bytes());
        rdef[e + 20..e + 24].copy_from_slice(&bind.to_le_bytes());
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    rdef.extend(names);

    // Two chunks: something else first, then RDEF.
    let other = [0xAAu8; 4];
    let mut out = b"DXBC".to_vec();
    out.extend([0u8; 16]);
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    let first = 0x28u32;
    let second = first + 8 + other.len() as u32;
    out.extend(first.to_le_bytes());
    out.extend(second.to_le_bytes());
    out.extend(b"SFI0");
    out.extend((other.len() as u32).to_le_bytes());
    out.extend(other);
    out.extend(b"RDEF");
    out.extend((rdef.len() as u32).to_le_bytes());
    out.extend(rdef);
    out
}
