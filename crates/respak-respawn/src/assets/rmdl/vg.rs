//! The vertex data ("VG"): mesh headers, vertex layouts and the decoding of a mesh into a part.

use respak_core::capability::MeshPart;
use respak_core::reader::Reader;
use respak_core::{Error, Result};

use super::block::reader;
use super::unsupported;

/// A mesh header of the VG (`vg::rev4::MeshHeader_t`) with the data ranges resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VgMesh {
    pub(super) flags: u64,
    pub(super) vert_count: usize,
    pub(super) stride: usize,
    pub(super) verts: usize,
    pub(super) indices: usize,
    pub(super) index_count: usize,
    pub(super) extra: usize,
    pub(super) extra_len: usize,
}

/// The mesh headers of LOD `lod_in_group` of a decompressed rev4 group.
pub(super) fn vg_meshes(group: &[u8], lod_in_group: u32) -> Result<Vec<VgMesh>> {
    let b = reader(group);
    let lod = 4 + b.u32(4)? as usize + lod_in_group as usize * 8;
    let mesh_count = usize::from(b.u8(lod)?);
    let first = lod + 4 + b.u32(lod + 4)? as usize;
    (0..mesh_count)
        .map(|i| {
            let m = first + i * 48;
            let packed = b.u32(m + 20)?;
            Ok(VgMesh {
                flags: b.u64(m)?,
                vert_count: b.u32(m + 8)? as usize,
                stride: usize::from(b.u16(m + 12)?),
                verts: m + 24 + b.u32(m + 24)? as usize,
                indices: m + 16 + b.u32(m + 16)? as usize,
                index_count: (packed & 0x0FFF_FFFF) as usize,
                extra: m + 32 + b.u32(m + 32)? as usize,
                extra_len: b.u32(m + 36)? as usize,
            })
        })
        .collect()
}

/// How a vertex is laid out, from the mesh flags.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct VertexLayout {
    /// 1: `Vec3` f32, 2: `Vector64`.
    pub(super) position: u8,
    pub(super) blend: bool,
    pub(super) color: bool,
    pub(super) uv_layers: usize,
}

const VERT_COLOR: u64 = 0x10;
const VERT_BLENDINDICES: u64 = 0x1000;
const VERT_BLENDWEIGHTS_UNPACKED: u64 = 0x2000;
const VERT_BLENDWEIGHTS_PACKED: u64 = 0x4000;

/// The layout of a vertex with `flags` that is `stride` bytes long. Anything that does not add
/// up to the stride is a layout we do not know.
pub(super) fn vertex_layout(flags: u64, stride: usize) -> Result<VertexLayout> {
    let position = (flags & 3) as u8;
    let pos_len = match position {
        1 => 12,
        2 => 8,
        3 => return Err(unsupported("vertex positions in 48 bit form")),
        _ => return Err(unsupported("a vertex without positions")),
    };
    // `VERT_UNK` adds no bytes (rsx: "flag gets ignored"); the stride check below guards the guess.
    if flags & VERT_BLENDWEIGHTS_UNPACKED != 0 {
        return Err(unsupported(format!("vertex flags {flags:#x}")));
    }
    let blend = flags & (VERT_BLENDINDICES | VERT_BLENDWEIGHTS_PACKED) != 0;
    let color = flags & VERT_COLOR != 0;
    let mut uv_layers = 0;
    for slot in 0..8 {
        match (flags >> (24 + 4 * slot)) & 0xF {
            0 => {}
            2 => uv_layers += 1,
            other => {
                return Err(unsupported(format!(
                    "texcoord format {other} in slot {slot}"
                )))
            }
        }
    }
    let size = pos_len + if blend { 8 } else { 0 } + 4 + if color { 4 } else { 0 } + uv_layers * 8;
    if size != stride {
        return Err(unsupported(format!(
            "vertex flags {flags:#x} describe {size} bytes, the stride is {stride}"
        )));
    }
    Ok(VertexLayout {
        position,
        blend,
        color,
        uv_layers,
    })
}

/// What `decode_mesh` needs to know about the model.
pub(super) struct DecodeCx<'a> {
    pub(super) bone_map: &'a [u16],
    pub(super) bone_count: usize,
    pub(super) extra_weights: bool,
    pub(super) wide_bones: bool,
}

impl DecodeCx<'_> {
    fn bone(&self, i: usize) -> Result<u16> {
        if self.bone_map.is_empty() {
            return Ok(i as u16);
        }
        self.bone_map
            .get(i)
            .copied()
            .ok_or_else(|| Error::corrupt(format!("rmdl: vertex bone {i} is outside the bone map")))
    }
}

/// Influences of every vertex, flat, with how many each vertex has.
#[derive(Default)]
struct Influences {
    flat: Vec<(u16, f32)>,
    counts: Vec<u8>,
}

/// Decodes one VG mesh into a part.
pub(super) fn decode_mesh(
    group: &[u8],
    m: &VgMesh,
    material: u64,
    d: &DecodeCx,
) -> Result<MeshPart> {
    let layout = vertex_layout(m.flags, m.stride)?;
    let b = reader(group);
    let verts = b.get(
        m.verts,
        m.vert_count
            .checked_mul(m.stride)
            .ok_or_else(|| Error::corrupt("rmdl: vertex buffer size overflows"))?,
    )?;
    let extra = reader(b.get(m.extra, m.extra_len)?);
    let v = reader(verts);

    let n = m.vert_count;
    let mut part = MeshPart {
        material,
        ..MeshPart::default()
    };
    part.positions.reserve(n);
    part.normals.reserve(n);
    part.uvs = vec![Vec::with_capacity(n); layout.uv_layers];
    let mut skin = Influences {
        flat: Vec::new(),
        counts: Vec::with_capacity(n),
    };

    for i in 0..n {
        let mut at = i * m.stride;
        at = read_position(&v, layout.position, at, &mut part)?;
        let start = skin.flat.len();
        if layout.blend {
            at = read_blend(&v, &extra, d, i, at, &mut skin.flat)?;
        } else {
            skin.flat.push((0, 1.0));
        }
        let influences = skin.flat.len() - start;
        if influences > usize::from(u8::MAX) {
            return Err(Error::corrupt(format!(
                "rmdl: vertex {i} has {influences} influences"
            )));
        }
        skin.counts.push(influences as u8);
        at = read_normal(&v, i, at, &mut part)?;
        read_color_and_uvs(&v, &layout, at, &mut part)?;
    }

    // Only a skinned mesh carries bone streams.
    if layout.blend {
        write_bone_streams(&mut part, &skin, d, n)?;
    }
    read_triangles(&b, m, &mut part)?;
    Ok(part)
}

/// Reads the position of the vertex at `at`, returns the offset after it.
fn read_position(v: &Reader, kind: u8, at: usize, part: &mut MeshPart) -> Result<usize> {
    Ok(match kind {
        1 => {
            part.positions.push(v.floats::<3>(at)?);
            at + 12
        }
        _ => {
            part.positions.push(unpack_vector64(v.u64(at)?));
            at + 8
        }
    })
}

/// Reads the blend weights and indices of vertex `i` into `flat`, returns the offset after them.
fn read_blend(
    v: &Reader,
    extra: &Reader,
    d: &DecodeCx,
    i: usize,
    at: usize,
    flat: &mut Vec<(u16, f32)>,
) -> Result<usize> {
    let (w0, w1) = (v.u16(at)?, v.u16(at + 2)?);
    let idx = v.get(at + 4, 4)?;
    let remaining_start = 32767i32;
    if d.extra_weights {
        let (first, last, count) = if d.wide_bones {
            let packed = u32::from_le_bytes(idx.try_into().expect("4 bytes"));
            (
                (packed & 0x3FF) as usize,
                ((packed >> 10) & 0x3FF) as usize,
                (packed >> 24) as usize,
            )
        } else {
            (
                usize::from(idx[0]),
                usize::from(idx[1]),
                usize::from(idx[3]),
            )
        };
        flat.push((d.bone(first)?, unpack_weight(w0)));
        let mut remaining = remaining_start - i32::from(w0);
        for k in 1..count {
            let e = usize::from(w1) + (k - 1);
            let weight = extra.i16(e * 4)?;
            let bone = extra.u16(e * 4 + 2)?;
            flat.push((d.bone(usize::from(bone))?, unpack_weight(weight as u16)));
            remaining -= i32::from(weight);
        }
        if count > 0 {
            flat.push((
                d.bone(last)?,
                unpack_weight(remaining.clamp(0, 32767) as u16),
            ));
        }
    } else {
        let count = usize::from(idx[3]);
        if count > 2 {
            return Err(Error::corrupt(format!(
                "rmdl: vertex {i} has {} influences",
                count + 1
            )));
        }
        let w = [w0, w1];
        let mut remaining = remaining_start;
        for k in 0..count {
            flat.push((d.bone(usize::from(idx[k]))?, unpack_weight(w[k])));
            remaining -= i32::from(w[k]);
        }
        flat.push((
            d.bone(usize::from(idx[count]))?,
            unpack_weight(remaining.clamp(0, 32767) as u16),
        ));
    }
    Ok(at + 8)
}

/// Reads the normal of vertex `i`, returns the offset after it.
fn read_normal(v: &Reader, i: usize, at: usize, part: &mut MeshPart) -> Result<usize> {
    part.normals.push(
        unpack_normal32(v.u32(at)?)
            .ok_or_else(|| Error::corrupt(format!("rmdl: vertex {i} has an invalid normal")))?,
    );
    Ok(at + 4)
}

/// Reads the colour and the UV layers of a vertex.
fn read_color_and_uvs(
    v: &Reader,
    layout: &VertexLayout,
    mut at: usize,
    part: &mut MeshPart,
) -> Result<()> {
    if layout.color {
        part.colors.push(v.get(at, 4)?.try_into().expect("4 bytes"));
        at += 4;
    }
    for layer in &mut part.uvs {
        layer.push(v.floats::<2>(at)?);
        at += 8;
    }
    Ok(())
}

/// Pads every vertex to the largest influence count and checks the bones against the skeleton.
fn write_bone_streams(
    part: &mut MeshPart,
    skin: &Influences,
    d: &DecodeCx,
    n: usize,
) -> Result<()> {
    let max = skin.counts.iter().copied().max().unwrap_or(0) as usize;
    part.influences = max as u32;
    part.bone_indices.reserve(n * max);
    part.bone_weights.reserve(n * max);
    let mut cursor = 0;
    for &c in &skin.counts {
        for k in 0..max {
            let (bone, weight) = if k < usize::from(c) {
                skin.flat[cursor + k]
            } else {
                (0, 0.0)
            };
            if usize::from(bone) >= d.bone_count.max(1) {
                return Err(Error::corrupt(format!(
                    "rmdl: vertex bone {bone} but the skeleton has {} bones",
                    d.bone_count
                )));
            }
            part.bone_indices.push(bone);
            part.bone_weights.push(weight);
        }
        cursor += usize::from(c);
    }
    Ok(())
}

/// Reads the triangle list of a mesh, checking every index against the vertex count.
fn read_triangles(b: &Reader, m: &VgMesh, part: &mut MeshPart) -> Result<()> {
    let n = m.vert_count;
    if !m.index_count.is_multiple_of(3) {
        return Err(Error::corrupt(format!(
            "rmdl: {} indices are not whole triangles",
            m.index_count
        )));
    }
    let raw = b.get(m.indices, m.index_count * 2)?;
    for tri in raw.chunks_exact(6) {
        let t = [0, 2, 4].map(|o| u32::from(u16::from_le_bytes([tri[o], tri[o + 1]])));
        if t.iter().any(|&i| i as usize >= n) {
            return Err(Error::corrupt(format!(
                "rmdl: a triangle refers to vertex {} of {n}",
                t.iter().max().expect("3 values")
            )));
        }
        part.triangles.push(t);
    }
    Ok(())
}

/// Unpacks a `Vector64` position (x, y: 21 bits, z: 22 bits, steps of 1/1024).
pub(super) fn unpack_vector64(packed: u64) -> [f32; 3] {
    const STEP: f32 = 0.000_976_562_5;
    let x = (packed & 0x1F_FFFF) as f32;
    let y = ((packed >> 21) & 0x1F_FFFF) as f32;
    let z = (packed >> 42) as f32;
    [x * STEP - 1024.0, y * STEP - 1024.0, z * STEP - 2048.0]
}

/// Unpacks a `Normal32` into a unit normal (the tangent is not needed). `None` if the index of
/// the dropped component is 3.
pub(super) fn unpack_normal32(packed: u32) -> Option<[f32; 3]> {
    let i1 = ((packed >> 29) & 3) as usize;
    if i1 > 2 {
        return None;
    }
    let negative = (packed >> 28) & 1 != 0;
    let v1 = if negative { -255.0 } else { 255.0 };
    let v2 = ((packed >> 19) & 0x1FF) as f32 - 256.0;
    let v3 = ((packed >> 10) & 0x1FF) as f32 - 256.0;
    let i2 = ((0x124u32 >> (2 * i1 + 2)) & 3) as usize;
    let i3 = ((0x124u32 >> (2 * i1 + 4)) & 3) as usize;
    let len = (255.0f32 * 255.0 + v2 * v2 + v3 * v3).sqrt();
    let mut n = [0.0; 3];
    n[i1] = v1 / len;
    n[i2] = v2 / len;
    n[i3] = v3 / len;
    Some(n)
}

/// A weight as stored (`UNPACKWEIGHT`): `(w + 1) / 32768`, so 32767 is 1.0.
pub(super) fn unpack_weight(w: u16) -> f32 {
    (f32::from(w) + 1.0) / 32768.0
}
