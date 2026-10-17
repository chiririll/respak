use crate::builder::PakBuilder;
use crate::bytes::put;
pub use crate::bytes::{put16, put32, put_f};

pub const VERT_COLOR: u64 = 0x10;
pub const VERT_BLEND: u64 = 0x1000 | 0x4000;

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
