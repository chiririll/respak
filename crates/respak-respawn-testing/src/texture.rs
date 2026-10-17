use crate::bytes::put;
use crate::raw::{build_raw_with, RawAsset};

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
