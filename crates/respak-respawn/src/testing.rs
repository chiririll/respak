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
        Self {
            guid,
            kind: *kind,
            version,
            header: header.to_vec(),
        }
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

// ---- patch edit streams ---------------------------------------------------------------------

/// Builds an edit stream (the part of a patch file that rebuilds the pages it does not store).
/// Both Huffman tables use eight equal 3-bit codes: command `c` and size exponent `e` (0 to 7) are
/// coded as the 3 bits of the number, so a count `n` (1 to 255) takes `3 + 3 + log2(n)` bits.
#[derive(Default)]
pub struct EditScript {
    bits: Vec<bool>,
    replacement: Vec<u8>,
}

impl EditScript {
    pub fn new() -> Self {
        Self::default()
    }

    fn put(&mut self, value: u64, n: u32) {
        for i in 0..n {
            self.bits.push(value >> i & 1 == 1);
        }
    }

    fn code(&mut self, value: u8) {
        // The 3-bit code of `value`, stored bit-reversed because the stream is read LSB first.
        let reversed = (0..3).fold(0u64, |r, i| r | u64::from(value >> i & 1) << (2 - i));
        self.put(reversed, 3);
    }

    fn counted(&mut self, command: u8, n: u64) -> &mut Self {
        assert!((1..256).contains(&n));
        let e = 63 - n.leading_zeros() as u8;
        self.code(command);
        self.code(e);
        self.put(n - (1 << e), u32::from(e));
        self
    }

    /// Command 0: copy `n` bytes of the files.
    pub fn copy(&mut self, n: u64) -> &mut Self {
        self.counted(0, n)
    }

    /// Command 1: skip `n` bytes of the files.
    pub fn skip(&mut self, n: u64) -> &mut Self {
        self.counted(1, n)
    }

    /// Command 2: insert bytes (the files stay where they are).
    pub fn insert(&mut self, bytes: &[u8]) -> &mut Self {
        self.replacement.extend_from_slice(bytes);
        self.counted(2, bytes.len() as u64)
    }

    /// Command 3: bytes that replace the same number of bytes of the files.
    pub fn replace(&mut self, bytes: &[u8]) -> &mut Self {
        self.replacement.extend_from_slice(bytes);
        self.counted(3, bytes.len() as u64)
    }

    /// Command 4, 5 or 6: one byte (two for 6) of replacement, then a copy of 3, 7 or 6 bytes.
    pub fn patched_copy(&mut self, command: u8, replacement: &[u8]) -> &mut Self {
        assert!(
            (4..=6).contains(&command) && replacement.len() == if command == 6 { 2 } else { 1 }
        );
        self.replacement.extend_from_slice(replacement);
        self.code(command);
        self
    }

    /// A command by number, with no count (for streams that must be rejected).
    pub fn raw_command(&mut self, command: u8) -> &mut Self {
        self.code(command);
        self
    }

    /// The stream: the two tables, the offset word, the bits, the replacement data.
    pub fn finish(&self) -> Vec<u8> {
        // Entry: symbol << 3 | (code length - 1). Symbols 0 to 7 in order get the codes 0 to 7.
        let table: Vec<u8> = (0..8u8).map(|symbol| symbol << 3 | 2).collect();
        let mut word = vec![false; 24];
        word.extend(&self.bits);
        let mut bits: Vec<u8> = word
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |b, (i, &set)| b | u8::from(set) << i)
            })
            .collect();
        while bits.len() < 8 {
            bits.push(0);
        }
        let offset = bits.len() as u32;
        bits[..3].copy_from_slice(&offset.to_le_bytes()[..3]);
        let mut out = table.clone();
        out.extend(&table);
        out.extend(&bits);
        out.extend(&self.replacement);
        out
    }
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
    pub fn new(
        guid: u64,
        version: u32,
        w: u16,
        h: u16,
        format: u16,
        permanent: u8,
        data: Vec<u8>,
    ) -> Self {
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

/// Appends one texture to the header page (`headers`), the data page (`misc`), the pointer table
/// and the asset table.
fn push_texture(
    t: &TexSpec,
    headers: &mut Vec<u8>,
    misc: &mut Vec<u8>,
    pointers: &mut Vec<(u32, u32)>,
    assets: &mut Vec<RawAsset>,
) {
    let name_offset = t.name.map(|n| {
        let at = misc.len() as u32;
        misc.extend_from_slice(n.as_bytes());
        misc.push(0);
        while !misc.len().is_multiple_of(16) {
            misc.push(0);
        }
        at
    });
    let data_offset = misc.len() as u32;
    misc.extend_from_slice(&t.data);
    while !misc.len().is_multiple_of(16) {
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

const TEX_STARPAKS: [&str; 1] = [r"paks\Win64\streamed.starpak"];
const TEX_OPT_STARPAKS: [&str; 1] = ["paks/Win64/opt.starpak"];

/// A pak with `txtr` assets: page 0 holds the headers, page 1 the names and CPU data.
pub fn build_tex_pak(pak_version: u16, textures: &[TexSpec]) -> Vec<u8> {
    let (mut headers, mut misc, mut pointers, mut assets) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in textures {
        push_texture(t, &mut headers, &mut misc, &mut pointers, &mut assets);
    }
    build_raw_with(
        pak_version,
        &[headers, misc],
        &pointers,
        &assets,
        &TEX_STARPAKS,
        &TEX_OPT_STARPAKS,
    )
}

/// One sprite of a [`UimgSpec`].
#[derive(Clone)]
pub struct SpriteSpec {
    /// Path in the names blob; `None` points the entry at an empty string.
    pub path: Option<&'static str>,
    pub hash: u32,
    /// Min x, min y, size x, size y as fractions of the atlas.
    pub uv: [f32; 4],
    pub layout: [f32; 8],
    pub render: (u16, u16),
}

/// A UI image atlas to put in a pak.
#[derive(Clone)]
pub struct UimgSpec {
    pub guid: u64,
    /// The asset version: 10 or 11.
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub atlas_guid: u64,
    pub sprites: Vec<SpriteSpec>,
    /// Whether the names pointer is set.
    pub names: bool,
}

/// A pak with `txtr` and `uimg` assets (the textures first). Page 0 holds the headers, page 1 the
/// arrays, names and CPU data.
pub fn build_uimg_pak(pak_version: u16, textures: &[TexSpec], atlases: &[UimgSpec]) -> Vec<u8> {
    let (mut headers, mut misc, mut pointers, mut assets) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for t in textures {
        push_texture(t, &mut headers, &mut misc, &mut pointers, &mut assets);
    }
    let pad = |misc: &mut Vec<u8>| {
        while !misc.len().is_multiple_of(16) {
            misc.push(0);
        }
    };
    for a in atlases {
        let n = a.sprites.len();
        let (stride, mut names_blob, mut hashes) =
            (if a.version == 10 { 8 } else { 12 }, Vec::new(), Vec::new());
        let (mut offsets, mut dims, mut bounds) = (Vec::new(), Vec::new(), Vec::new());
        for s in &a.sprites {
            let at = names_blob.len() as u32;
            if let Some(path) = s.path {
                names_blob.extend_from_slice(path.as_bytes());
            }
            names_blob.push(0);
            let mut entry = vec![0u8; stride];
            put(&mut entry, 0, &s.hash.to_le_bytes());
            if a.version == 10 {
                put(&mut entry, 6, &(at as u16).to_le_bytes());
            } else {
                put(&mut entry, 4, &at.to_le_bytes());
            }
            hashes.extend(entry);
            offsets.extend(s.layout.iter().flat_map(|f| f.to_le_bytes()));
            dims.extend_from_slice(&s.render.0.to_le_bytes());
            dims.extend_from_slice(&s.render.1.to_le_bytes());
            bounds.extend(s.uv.iter().flat_map(|f| f.to_le_bytes()));
        }
        let mut place = |bytes: &[u8]| {
            let at = misc.len() as u32;
            misc.extend_from_slice(bytes);
            pad(&mut misc);
            at
        };
        let (offsets_at, dims_at, hashes_at, names_at, cpu_at) = (
            place(&offsets),
            place(&dims),
            place(&hashes),
            place(&names_blob),
            place(&bounds),
        );

        let mut h = vec![0u8; 0x40];
        put(&mut h, 0, &(1.0 / f32::from(a.width)).to_le_bytes());
        put(&mut h, 4, &(1.0 / f32::from(a.height)).to_le_bytes());
        put(&mut h, 8, &a.width.to_le_bytes());
        put(&mut h, 0x0A, &a.height.to_le_bytes());
        put(&mut h, 0x0C, &(n as u16).to_le_bytes());
        put(&mut h, 0x20, &[0xFF; 8]);
        put(&mut h, 0x38, &a.atlas_guid.to_le_bytes());
        let head_offset = headers.len() as u32;
        let slots = [
            (0x10, Some(offsets_at)),
            (0x18, Some(dims_at)),
            (0x28, Some(hashes_at)),
            (0x30, a.names.then_some(names_at)),
        ];
        for (slot, target) in slots {
            match target {
                Some(t) => {
                    put(&mut h, slot, &1u32.to_le_bytes());
                    put(&mut h, slot + 4, &t.to_le_bytes());
                    pointers.push((0, head_offset + slot as u32));
                }
                None => put(&mut h, slot, &[0xFF; 8]),
            }
        }
        headers.extend(h);
        assets.push(RawAsset {
            guid: a.guid,
            kind: *b"uimg",
            version: a.version,
            head: (0, head_offset),
            data: Some((1, cpu_at)),
            header_size: 0x40,
            starpak: -1,
            opt_starpak: -1,
        });
    }
    build_raw_with(
        pak_version,
        &[headers, misc],
        &pointers,
        &assets,
        &TEX_STARPAKS,
        &TEX_OPT_STARPAKS,
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
    put(
        &mut out,
        if version == 8 { 0x30 } else { 0x28 },
        &decompressed.to_le_bytes(),
    );
    out.extend_from_slice(&stream);
    out
}

// ---- general paks, materials, shaders ---------------------------------------------------------

/// A cell of [`PakBuilder::dtbl`]; `Str` is any string-typed cell, `Null` a null string pointer.
pub enum DtblCell<'a> {
    Bool(bool),
    Int(i32),
    Float(f32),
    Vector([f32; 3]),
    Str(&'a str),
    Null,
}

/// Sets the pak header's `createdTime` of a built pak.
pub fn set_created_time(pak: &mut [u8], time: u64) {
    put(pak, 0x08, &time.to_le_bytes());
}

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
    pub fn asset(
        &mut self,
        guid: u64,
        kind: &[u8; 4],
        version: u32,
        mut header: Vec<u8>,
        pointers: &[(usize, u32)],
        data: Option<u32>,
    ) {
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
        let array: Vec<u8> = textures
            .iter()
            .flat_map(|g| g.to_le_bytes())
            .chain([0; 8])
            .collect();
        let array_at = self.data(&array);
        self.asset(
            guid,
            b"matl",
            16,
            h,
            &[
                (0x18, name_at),
                (0x60, array_at),
                (0x68, array_at + 8 * textures.len() as u32),
            ],
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
        self.misc[entry_at as usize + 8..entry_at as usize + 12]
            .copy_from_slice(&(bytecode.len() as i32).to_le_bytes());
        self.asset(guid, b"shdr", 12, vec![0u8; 32], &[], Some(entry_at));
    }

    /// A `dtbl` of asset `version` (0 or 1); `wide` uses the V1.1 column record (0x18 bytes).
    /// Columns are `(name, type)`; cells are packed in column order, the row stride is their sum.
    pub fn dtbl(
        &mut self,
        guid: u64,
        version: u32,
        wide: bool,
        columns: &[(&str, i32)],
        rows: &[Vec<DtblCell>],
    ) {
        let size = |ty: i32| match ty {
            0 => 1,
            1 | 2 => 4,
            3 => 12,
            _ => 8,
        };
        let offsets: Vec<u32> = columns
            .iter()
            .scan(0, |at, &(_, ty)| {
                Some(std::mem::replace(at, *at + size(ty)))
            })
            .collect();
        let stride = columns.iter().map(|&(_, ty)| size(ty)).sum::<u32>();
        let record = if wide { 0x18 } else { 0x10 };

        let names: Vec<u32> = columns.iter().map(|(name, _)| self.string(name)).collect();
        let mut records = vec![0u8; columns.len() * record];
        for (i, &(_, ty)) in columns.iter().enumerate() {
            let r = &mut records[i * record..(i + 1) * record];
            let at = if wide { 0x10 } else { 0x08 };
            put(r, at, &ty.to_le_bytes());
            put(r, at + 4, &offsets[i].to_le_bytes());
        }
        let records_at = self.data(&records);
        for (i, &name) in names.iter().enumerate() {
            self.data_pointer(records_at + (i * record) as u32, name);
        }

        let mut block = vec![0u8; rows.len() * stride as usize];
        let mut strings = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let at = r * stride as usize + offsets[c] as usize;
                match cell {
                    DtblCell::Bool(b) => block[at] = u8::from(*b),
                    DtblCell::Int(n) => put(&mut block, at, &n.to_le_bytes()),
                    DtblCell::Float(f) => put(&mut block, at, &f.to_le_bytes()),
                    DtblCell::Vector(v) => {
                        for (k, f) in v.iter().enumerate() {
                            put(&mut block, at + 4 * k, &f.to_le_bytes());
                        }
                    }
                    DtblCell::Str(s) => strings.push((at as u32, self.string(s))),
                    DtblCell::Null => {}
                }
            }
        }
        let rows_at = self.data(&block);
        for (at, target) in strings {
            self.data_pointer(rows_at + at, target);
        }

        let mut h = vec![0u8; if version == 0 { 0x20 } else { 0x28 }];
        put(&mut h, 0, &(columns.len() as i32).to_le_bytes());
        put(&mut h, 4, &(rows.len() as i32).to_le_bytes());
        put(
            &mut h,
            if version == 0 { 0x18 } else { 0x20 },
            &stride.to_le_bytes(),
        );
        self.asset(
            guid,
            b"dtbl",
            version,
            h,
            &[(0x08, records_at), (0x10, rows_at)],
            None,
        );
    }

    /// The pak file.
    pub fn build(self, pak_version: u16) -> Vec<u8> {
        build_raw(
            pak_version,
            &[self.heads, self.misc],
            &self.pointers,
            &self.assets,
        )
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

// ---- rmdl models ------------------------------------------------------------------------------

/// Builders for synthetic `mdl_` assets (studio block and vertex data), asset versions 16 to 20.
pub mod rmdl {
    pub const VERT_COLOR: u64 = 0x10;
    pub const VERT_BLEND: u64 = 0x1000 | 0x4000;

    pub fn put16(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put32(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn put_f(b: &mut [u8], at: usize, v: &[f32]) {
        for (i, f) in v.iter().enumerate() {
            b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&f.to_le_bytes());
        }
    }

    /// A v16/v17-layout (or v19_2 when `wide`) studio block with 2 bones (split form unless
    /// `major >= 19`), 1 body part with 1 model of 2 meshes, 2 materials, 2 skin families, 2 LODs
    /// in one group and a 2-entry bone map.
    pub fn studio(major: u32, wide: bool, group_len: u32) -> Vec<u8> {
        let shift = if wide { 16 } else { 0 };
        let mut b = vec![0u8; 0x400];
        put32(&mut b, 0, 0x4000);
        b[0x0A..0x0A + 6].copy_from_slice(b"model\0");
        put16(&mut b, 0x74, 2);
        put16(&mut b, 0x76, 0x100); // bonehdr
        put16(&mut b, 0x78, 0x140); // bonedata
                                    // bonehdr[i]: sznameindex at +10, relative to the bonehdr.
        for (i, name) in ["root", "arm"].iter().enumerate() {
            let h = 0x100 + i * 12;
            let at = 0x3A0 + i * 8;
            b[at..at + name.len()].copy_from_slice(name.as_bytes());
            put16(&mut b, h + 10, (at - h) as u16);
        }
        if major <= 18 {
            for i in 0..2usize {
                let d = 0x140 + i * 128;
                put_f(&mut b, d + 64, &[1.0 + i as f32, 2.0, 3.0]);
                put_f(&mut b, d + 76, &[0.0, 0.0, 0.0, 1.0]);
                put_f(&mut b, d + 104, &[1.0, 1.0, 1.0]);
                b[d + 116..d + 118].copy_from_slice(&((i as i16) - 1).to_le_bytes());
            }
        } else {
            for i in 0..2usize {
                let d = 0x140 + i * 16;
                b[d..d + 2].copy_from_slice(&((i as i16) - 1).to_le_bytes());
            }
            // linear bone block at 0x1C0: offsets relative to the block
            let lb = 0x1C0;
            put16(&mut b, 0xCA + shift, lb as u16);
            put16(&mut b, lb + 6, 0x20); // pos at lb + 0x20
            put16(&mut b, lb + 8, 0x40); // quat
            put16(&mut b, lb + 16, 0x60); // scale
            for i in 0..2usize {
                put_f(&mut b, lb + 0x20 + i * 12, &[1.0 + i as f32, 2.0, 3.0]);
                put_f(&mut b, lb + 0x40 + i * 16, &[0.0, 0.0, 0.0, 1.0]);
                put_f(&mut b, lb + 0x60 + i * 12, &[1.0, 1.0, 1.0]);
            }
        }
        // materials: 2 guids at 0x230; skins: 2 families x 2
        put16(&mut b, 0x90, 2);
        put16(&mut b, 0x92, 0x270);
        b[0x270..0x278].copy_from_slice(&0xAAAAu64.to_le_bytes());
        b[0x278..0x280].copy_from_slice(&0xBBBBu64.to_le_bytes());
        put16(&mut b, 0x96, 2);
        put16(&mut b, 0x98, 0x280);
        for (i, v) in [0u16, 1, 1, 0].iter().enumerate() {
            put16(&mut b, 0x280 + i * 2, *v);
        }
        // body part at 0x250: name at 0x270, models at 0x280 (relative to the body part)
        put16(&mut b, 0x9A, 1);
        put16(&mut b, 0x9C, 0x290);
        put16(&mut b, 0x290, 0x20); // sznameindex
        b[0x2B0..0x2B7].copy_from_slice(b"weapon\0");
        put16(&mut b, 0x292, 0x30); // modelindex -> 0x2C0
        put32(&mut b, 0x290 + 8, 1);
        // model at 0x280: 2 meshes at 0x290 (relative to the model)
        put16(&mut b, 0x2C0 + 4, 2);
        put16(&mut b, 0x2C0 + 8, 0x10);
        for (k, (mat, id)) in [(0u16, 0u16), (1, 1)].iter().enumerate() {
            put16(&mut b, 0x2D0 + k * 20, *mat);
            put16(&mut b, 0x2D0 + k * 20 + 2, *id);
        }
        // bone state map (relative to its field at 0xB0): 0x2C0
        put16(&mut b, 0xB0, 0x300 - 0xB0);
        put16(&mut b, 0xB2, 2);
        if wide {
            put16(&mut b, 0x300, 1);
            put16(&mut b, 0x302, 0);
        } else {
            b[0x300] = 1;
            b[0x301] = 0;
        }
        // LOD switch (relative to its field): 2 floats at 0x2D0
        put16(&mut b, 0xB8 + shift, (0x310 - (0xB8 + shift)) as u16);
        put16(&mut b, 0xBA + shift, 2);
        put_f(&mut b, 0x310, &[10.0, 50.0]);
        // one group at 0x2E0 holding both LODs
        put16(&mut b, 0xB4 + shift, (0x320 - (0xB4 + shift)) as u16);
        put16(&mut b, 0xB6 + shift, 1);
        put32(&mut b, 0x320 + 4, group_len);
        put32(&mut b, 0x320 + 8, group_len);
        b[0x320 + 12] = 0;
        b[0x320 + 13] = 0;
        b[0x320 + 14] = 2;
        b[0x320 + 15] = 0b11;
        b
    }

    /// Flags for: a position form (1 = `Vec3`, 2 = `Vector64`), packed blend, colour, `uvs` UV layers.
    pub fn flags(position: u64, blend: bool, color: bool, uvs: u64) -> u64 {
        let mut f = position;
        if blend {
            f |= VERT_BLEND;
        }
        if color {
            f |= VERT_COLOR;
        }
        for slot in 0..uvs {
            f |= 2 << (24 + 4 * slot);
        }
        f
    }

    /// A `Normal32` for the +Z axis.
    pub fn normal_z() -> u32 {
        (2 << 29) | (256 << 19) | (256 << 10)
    }

    /// A `Vec3` position, packed blend and `Normal32` vertex (24 bytes).
    pub fn blend_vertex(w0: u16, w1: u16, idx: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 12]);
        v.extend_from_slice(&w0.to_le_bytes());
        v.extend_from_slice(&w1.to_le_bytes());
        v.extend_from_slice(&idx);
        v.extend_from_slice(&normal_z().to_le_bytes());
        v
    }

    /// One mesh of a vertex group.
    #[derive(Clone)]
    pub struct VgMeshSpec {
        pub flags: u64,
        pub stride: u16,
        pub vert_count: u32,
        pub verts: Vec<u8>,
        pub indices: Vec<u16>,
        pub extra: Vec<u8>,
    }

    /// A rev4 vertex group holding one LOD per entry of `lods`, each with its meshes.
    pub fn vg(lods: &[Vec<VgMeshSpec>]) -> Vec<u8> {
        let total: usize = lods.iter().map(Vec::len).sum();
        let first_mesh = 8 + 8 * lods.len();
        let mut g = vec![0u8; first_mesh + 48 * total];
        g[1] = lods.len() as u8;
        put32(&mut g, 4, 4); // the LOD array is at 4 + 4
        let mut mesh_index = 0;
        for (i, lod) in lods.iter().enumerate() {
            let lod_at = 8 + 8 * i;
            g[lod_at] = lod.len() as u8;
            let array_at = first_mesh + 48 * mesh_index;
            put32(&mut g, lod_at + 4, (array_at - (lod_at + 4)) as u32);
            for (j, mesh) in lod.iter().enumerate() {
                let m = array_at + 48 * j;
                g[m..m + 8].copy_from_slice(&mesh.flags.to_le_bytes());
                put32(&mut g, m + 8, mesh.vert_count);
                put16(&mut g, m + 12, mesh.stride);
                let verts_at = g.len();
                g.extend_from_slice(&mesh.verts);
                let indices_at = g.len();
                g.extend(mesh.indices.iter().flat_map(|i| i.to_le_bytes()));
                let extra_at = g.len();
                g.extend_from_slice(&mesh.extra);
                put32(&mut g, m + 16, (indices_at - (m + 16)) as u32);
                put32(&mut g, m + 20, mesh.indices.len() as u32);
                put32(&mut g, m + 24, (verts_at - (m + 24)) as u32);
                put32(&mut g, m + 28, mesh.verts.len() as u32);
                put32(&mut g, m + 32, (extra_at - (m + 32)) as u32);
                put32(&mut g, m + 36, mesh.extra.len() as u32);
            }
            mesh_index += lod.len();
        }
        g
    }

    /// A group with one LOD of one mesh.
    pub fn vg_single(
        flags: u64,
        stride: u16,
        vert_count: u32,
        verts: &[u8],
        indices: &[u16],
        extra: &[u8],
    ) -> Vec<u8> {
        vg(&[vec![VgMeshSpec {
            flags,
            stride,
            vert_count,
            verts: verts.to_vec(),
            indices: indices.to_vec(),
            extra: extra.to_vec(),
        }]])
    }
}

impl PakBuilder {
    /// A model of asset version `major` (16 to 20): the 96-byte asset header, CPU data, `studio`
    /// (see [`rmdl::studio`]) and the inline vertex data `vg` (see [`rmdl::vg`]).
    pub fn rmdl(&mut self, guid: u64, name: &str, major: u32, studio: &[u8], vg: &[u8]) {
        self.rmdl_inner(guid, name, major, studio, Some(vg), &[]);
    }

    /// Like [`PakBuilder::rmdl_without_vertex_data`] but listing the animation sequences `seqs`.
    pub fn rmdl_with_sequences(
        &mut self,
        guid: u64,
        name: &str,
        major: u32,
        studio: &[u8],
        seqs: &[u64],
    ) {
        self.rmdl_inner(guid, name, major, studio, None, seqs);
    }

    /// Like [`PakBuilder::rmdl`] but with no vertex data at all (a null inline pointer, no starpak).
    pub fn rmdl_without_vertex_data(&mut self, guid: u64, name: &str, major: u32, studio: &[u8]) {
        self.rmdl_inner(guid, name, major, studio, None, &[]);
    }

    fn rmdl_inner(
        &mut self,
        guid: u64,
        name: &str,
        major: u32,
        studio: &[u8],
        vg: Option<&[u8]>,
        seqs: &[u64],
    ) {
        let studio_at = self.data(studio);
        let name_at = self.string(name);
        let vg_at = vg.map(|v| self.data(v));
        let mut cpu = vec![0u8; 16];
        put(&mut cpu, 12, &(studio.len() as i32).to_le_bytes());
        let cpu_at = self.data(&cpu);
        let mut h = vec![0u8; 96];
        put(
            &mut h,
            0x2C,
            &(vg.map_or(0, <[u8]>::len) as u32).to_le_bytes(),
        );
        put(&mut h, 0x30, &(-1.0f32).to_le_bytes());
        put(&mut h, 0x3C, &1.0f32.to_le_bytes());
        let mut pointers = vec![(0x00, studio_at), (0x08, name_at)];
        pointers.extend(vg_at.map(|at| (0x18, at)));
        if !seqs.is_empty() {
            let list: Vec<u8> = seqs.iter().flat_map(|g| g.to_le_bytes()).collect();
            pointers.push((0x50, self.data(&list)));
            put(&mut h, 0x4A, &(seqs.len() as u16).to_le_bytes());
        }
        self.asset(guid, b"mdl_", major, h, &pointers, Some(cpu_at));
    }
}

// ---- aseq, asqd, arig -------------------------------------------------------------------------

/// Builders for synthetic animation assets: the sequence block (seqdesc v18, animdesc v19.1), the
/// RLE section data of an `asqd`, and the encodings inside it.
pub mod aseq {
    use super::put;

    /// An IEEE half float (normal numbers and zero only, truncating).
    pub fn half(v: f32) -> u16 {
        if v == 0.0 {
            return if v.is_sign_negative() { 0x8000 } else { 0 };
        }
        let bits = v.to_bits();
        let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
        assert!(
            (1..31).contains(&exponent),
            "{v} is not a normal half float"
        );
        (((bits >> 16) & 0x8000) as u16) | ((exponent as u16) << 10) | ((bits >> 13) & 0x3FF) as u16
    }

    /// A `Vector48`.
    pub fn vec48(v: [f32; 3]) -> [u8; 6] {
        let mut out = [0u8; 6];
        for (i, x) in v.iter().enumerate() {
            out[2 * i..2 * i + 2].copy_from_slice(&half(*x).to_le_bytes());
        }
        out
    }

    /// A `Quaternion64` from `[x, y, z, w]`.
    pub fn quat64(q: [f32; 4]) -> [u8; 8] {
        let axis =
            |v: f32| ((v * 1_048_576.5 + 1_048_576.0).round() as i64).clamp(0, 0x1F_FFFF) as u64;
        let packed =
            axis(q[0]) | (axis(q[1]) << 21) | (axis(q[2]) << 42) | (u64::from(q[3] < 0.0) << 63);
        packed.to_le_bytes()
    }

    /// A track block of `valid = 0`: one `i16` per frame.
    pub fn raw_track(values: &[i16]) -> Vec<u8> {
        let mut out = vec![0, values.len() as u8];
        out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
        out
    }

    /// A `mstudioanim_valueptr_t`: `offset` bytes to the first track, `axes` (bit 2 X, 1 Y, 0 Z),
    /// then the track distances in 2-byte units.
    pub fn value_ptr(offset: u16, axes: u8, axis1: u8, axis2: u8) -> [u8; 4] {
        let head = (offset | (u16::from(axes) << 13)).to_le_bytes();
        [head[0], head[1], axis1, axis2]
    }

    /// A bone's header (`size`, `tracked`: 0x1 scale, 0x2 rotation, 0x4 position) and its `data`.
    pub fn bone(tracked: u8, data: &[u8]) -> Vec<u8> {
        let mut out = ((data.len() as u16 + 2) | (u16::from(tracked) << 13))
            .to_le_bytes()
            .to_vec();
        out.extend_from_slice(data);
        out
    }

    /// The bone flag array: 4-bit entries, or 6-bit when `six`.
    pub fn flag_array(flags: &[u8], six: bool) -> Vec<u8> {
        let bits = if six { 6 } else { 4 };
        let mut out = vec![0u8; (bits * flags.len()).div_ceil(8).next_multiple_of(2)];
        for (i, f) in flags.iter().enumerate() {
            let at = bits * i;
            let word = u32::from(*f) << (at & 7);
            out[at >> 3] |= word as u8;
            if let Some(next) = out.get_mut((at >> 3) + 1) {
                *next |= (word >> 8) as u8;
            }
        }
        out
    }

    /// One section: the flag array of `bones` (flags, bone block) and then every non-empty block.
    pub fn section(six: bool, bones: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let flags: Vec<u8> = bones.iter().map(|b| b.0).collect();
        let mut out = flag_array(&flags, six);
        for (_, block) in bones {
            out.extend_from_slice(block);
        }
        out
    }

    /// One animation of a sequence (`mstudioanimdesc_v19_1_t`).
    #[derive(Clone)]
    pub struct Anim {
        pub name: String,
        pub fps: f32,
        /// `0x1` looping, `0x4` delta, `0x20` all zeros, `0x20000` has data, `0x200000` datapoint,
        /// `0x40000` frame movement.
        pub flags: i32,
        pub frames: i32,
        pub asqd: u64,
        pub stall: u16,
        pub section_frames: u16,
        /// The section table: an offset, or `!offset` for a section in `dataExtra`.
        pub sections: Vec<i32>,
        /// A `mstudioframemovement_t` (see [`movement`]).
        pub movement: Option<Vec<u8>>,
    }

    impl Anim {
        /// 30 fps, has data, no sections.
        pub fn new(name: &str, frames: i32, asqd: u64) -> Self {
            Self {
                name: name.into(),
                fps: 30.0,
                flags: 0x20000,
                frames,
                asqd,
                stall: 0,
                section_frames: 0,
                sections: Vec::new(),
                movement: None,
            }
        }
    }

    /// The bone weight list of a sequence.
    #[derive(Clone)]
    pub enum Weights {
        /// `weightlistindex` 1.
        Zero,
        /// `weightlistindex` 3.
        One,
        /// A float per bone.
        List(Vec<f32>),
    }

    /// A sequence (`mstudioseqdesc_v18_t`) with its animations.
    #[derive(Clone)]
    pub struct Seq {
        pub label: String,
        /// `0x20000`: the sequence has scale data.
        pub flags: i32,
        /// `groupsize`; the number of animations when `None`.
        pub group: Option<[u8; 2]>,
        pub weights: Weights,
        pub autolayers: Vec<u64>,
        pub anims: Vec<Anim>,
    }

    impl Seq {
        /// No scale, all weights one, no autolayers.
        pub fn new(label: &str, anims: Vec<Anim>) -> Self {
            Self {
                label: label.into(),
                flags: 0,
                group: None,
                weights: Weights::One,
                autolayers: Vec::new(),
                anims,
            }
        }
    }

    fn put16(b: &mut [u8], at: usize, v: u16) {
        put(b, at, &v.to_le_bytes());
    }

    fn cstr(b: &mut [u8], at: usize, s: &str) {
        put(b, at, s.as_bytes());
    }

    /// The block `aseq` points at: the seqdesc at 0, the animation table at 0x80, the label at
    /// 0xC0, weights at 0x100, autolayers at 0x200 and animation `i` at `0x400 + 0x400 * i`
    /// (animdesc, section table at +0x40, name at +0x80, frame movement at +0x100). Offsets are
    /// even, so `FIX_OFFSET` leaves them alone.
    pub fn seq_block(seq: &Seq) -> Vec<u8> {
        let mut b = vec![0u8; 0x400 + 0x400 * seq.anims.len()];
        put16(&mut b, 0x00, 0xC0);
        cstr(&mut b, 0xC0, &seq.label);
        put(&mut b, 0x04, &seq.flags.to_le_bytes());
        put16(&mut b, 0x2A, 0x80);
        let group = seq.group.unwrap_or([seq.anims.len() as u8, 1]);
        b[0x54] = group[0];
        b[0x55] = group[1];
        match &seq.weights {
            Weights::Zero => put16(&mut b, 0x52, 1),
            Weights::One => put16(&mut b, 0x52, 3),
            Weights::List(list) => {
                put16(&mut b, 0x52, 0x100);
                for (i, w) in list.iter().enumerate() {
                    put(&mut b, 0x100 + 4 * i, &w.to_le_bytes());
                }
            }
        }
        put16(&mut b, 0x4E, seq.autolayers.len() as u16);
        put16(&mut b, 0x50, 0x200);
        for (i, guid) in seq.autolayers.iter().enumerate() {
            put(&mut b, 0x200 + 0x20 * i, &guid.to_le_bytes());
        }
        for (i, a) in seq.anims.iter().enumerate() {
            let at = 0x400 + 0x400 * i;
            put16(&mut b, 0x80 + 2 * i, at as u16);
            put(&mut b, at, &a.fps.to_le_bytes());
            put(&mut b, at + 4, &a.flags.to_le_bytes());
            put(&mut b, at + 8, &a.frames.to_le_bytes());
            put16(&mut b, at + 0xC, 0x80);
            cstr(&mut b, at + 0x80, &a.name);
            if let Some(m) = &a.movement {
                put16(&mut b, at + 0xE, 0x100);
                put(&mut b, at + 0x100, m);
            }
            put(&mut b, at + 0x18, &a.asqd.to_le_bytes());
            put16(&mut b, at + 0x2A, 0x40);
            for (k, s) in a.sections.iter().enumerate() {
                put(&mut b, at + 0x40 + 4 * k, &s.to_le_bytes());
            }
            put16(&mut b, at + 0x2C, a.stall);
            put16(&mut b, at + 0x2E, a.section_frames);
        }
        b
    }

    /// A `mstudioframemovement_t`: `scale` (x, y, z, yaw), the frames per section, and per section
    /// the four tracks (x, y, z, yaw; `None` = no track).
    pub fn movement(
        scale: [f32; 4],
        section_frames: i32,
        sections: &[[Option<Vec<u8>>; 4]],
    ) -> Vec<u8> {
        let mut out = vec![0u8; 0x14 + 2 * sections.len()];
        for (i, s) in scale.iter().enumerate() {
            put(&mut out, 4 * i, &s.to_le_bytes());
        }
        put(&mut out, 0x10, &section_frames.to_le_bytes());
        for (i, tracks) in sections.iter().enumerate() {
            while !out.len().is_multiple_of(2) {
                out.push(0);
            }
            let section_at = out.len();
            put16(&mut out, 0x14 + 2 * i, section_at as u16);
            out.extend([0u8; 8]);
            for (k, track) in tracks.iter().enumerate() {
                if let Some(t) = track {
                    while !out.len().is_multiple_of(2) {
                        out.push(0);
                    }
                    let track_at = out.len() - section_at;
                    put16(&mut out, section_at + 2 * k, track_at as u16);
                    out.extend_from_slice(t);
                }
            }
        }
        out
    }
}

impl PakBuilder {
    /// A sequence of asset `version` (12 to 14; 64-byte header) whose data block is `block` (see
    /// [`aseq::seq_block`]), listing `models`, with `extra` as its `dataExtra`.
    pub fn aseq(
        &mut self,
        guid: u64,
        name: &str,
        version: u32,
        block: &[u8],
        models: &[u64],
        extra: Option<&[u8]>,
    ) {
        let block_at = self.data(block);
        let name_at = self.string(name);
        let mut h = vec![0u8; 64];
        let mut pointers = vec![(0x00, block_at), (0x08, name_at)];
        if !models.is_empty() {
            let list: Vec<u8> = models.iter().flat_map(|g| g.to_le_bytes()).collect();
            pointers.push((0x20, self.data(&list)));
            put(&mut h, 0x18, &(models.len() as u16).to_le_bytes());
        }
        if let Some(extra) = extra {
            pointers.push((0x38, self.data(extra)));
            put(&mut h, 0x1C, &(extra.len() as u32).to_le_bytes());
        }
        self.asset(guid, b"aseq", version, h, &pointers, None);
    }

    /// An `asqd` (asset version 1) holding `blob`.
    pub fn asqd(&mut self, guid: u64, blob: &[u8]) {
        let blob_at = self.data(blob);
        self.asset(guid, b"asqd", 1, vec![0u8; 8], &[(0x00, blob_at)], None);
    }

    /// An `asqd` with no data pointer.
    pub fn asqd_without_data(&mut self, guid: u64) {
        self.asset(guid, b"asqd", 1, vec![0u8; 8], &[], None);
    }

    /// An `arig` of asset `version` (4 to 7; 40-byte header) with the studio block `studio` (see
    /// [`rmdl::studio`]) listing the sequences `seqs`.
    pub fn arig(&mut self, guid: u64, name: &str, version: u32, studio: &[u8], seqs: &[u64]) {
        let studio_at = self.data(studio);
        let name_at = self.string(name);
        let mut h = vec![0u8; 0x28];
        let mut pointers = vec![(0x00, studio_at), (0x08, name_at)];
        if !seqs.is_empty() {
            let list: Vec<u8> = seqs.iter().flat_map(|g| g.to_le_bytes()).collect();
            pointers.push((0x18, self.data(&list)));
        }
        if version == 4 {
            put(&mut h, 0x14, &(seqs.len() as i32).to_le_bytes());
        } else {
            put(&mut h, 0x12, &(seqs.len() as i16).to_le_bytes());
        }
        self.asset(guid, b"arig", version, h, &pointers, None);
    }
}
