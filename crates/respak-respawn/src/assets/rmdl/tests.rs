use respak_core::{ErrorKind, Version};

use super::block::fix_offset;
use super::header::{
    variant, RmdlHeader, StudioLayout, ASSET_HEADER_LEN, CPU_LEN, CREATED_19_1, CREATED_19_3,
};
use super::studio::{Studio, StudioMesh};
use super::vg::{
    decode_mesh, unpack_normal32, unpack_vector64, unpack_weight, vertex_layout, vg_meshes,
    DecodeCx, VertexLayout,
};
use respak_respawn_testing::rmdl::{
    blend_vertex, flags, normal_z, put16, studio, vg_single as vg_group,
};

fn studio_block(major: u32, wide: bool) -> Vec<u8> {
    studio(major, wide, 100)
}

#[test]
fn fix_offset_both_parities() {
    assert_eq!(fix_offset(0), 0);
    assert_eq!(fix_offset(0x0100), 0x0100);
    assert_eq!(fix_offset(0x0101), 0x1000);
    assert_eq!(fix_offset(0x0003), 0x0020);
    assert_eq!(fix_offset(0xFFFF), 0xFFFE << 4);
}

#[test]
fn vector64_known_values() {
    assert_eq!(unpack_vector64(0), [-1024.0, -1024.0, -2048.0]);
    // x = 1024 steps (= 1.0), y = 2048 steps, z = 4096 steps.
    let packed = 1024u64 | (2048u64 << 21) | (4096u64 << 42);
    assert_eq!(unpack_vector64(packed), [-1023.0, -1022.0, -2044.0]);
    let max = unpack_vector64(u64::MAX);
    assert!((max[0] - 1023.999).abs() < 0.01 && (max[2] - 2047.999).abs() < 0.01);
}

#[test]
fn normal32_axis_aligned() {
    let pack = |idx1: u32, neg: u32| (idx1 << 29) | (neg << 28) | (256 << 19) | (256 << 10);
    for (idx1, neg) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)] {
        let n = unpack_normal32(pack(idx1, neg)).unwrap();
        for (axis, v) in n.iter().enumerate() {
            let want = if axis as u32 == idx1 {
                if neg == 1 {
                    -1.0
                } else {
                    1.0
                }
            } else {
                0.0
            };
            assert!(
                (v - want).abs() < 1e-6,
                "idx1 {idx1} neg {neg} axis {axis}: {v}"
            );
        }
    }
    assert!(unpack_normal32(3 << 29).is_none());
    let n = unpack_normal32((2 << 29) | (300 << 19) | (200 << 10)).unwrap();
    assert!(
        (n.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5,
        "unit length"
    );
}

#[test]
fn weights() {
    assert_eq!(unpack_weight(32767), 1.0);
    assert_eq!(unpack_weight(0), 1.0 / 32768.0);
}

#[test]
fn variants() {
    assert_eq!(variant(16, 0, None).unwrap(), Version::new(16));
    assert_eq!(variant(20, 0, None).unwrap(), Version::new(20));
    assert_eq!(variant(19, 0, None).unwrap(), Version::with_minor(19, 0));
    assert_eq!(
        variant(19, CREATED_19_1, None).unwrap(),
        Version::with_minor(19, 1)
    );
    assert_eq!(
        variant(19, 0, Some(244)).unwrap(),
        Version::with_minor(19, 2)
    );
    assert_eq!(
        variant(19, CREATED_19_1, Some(244)).unwrap(),
        Version::with_minor(19, 2)
    );
    assert_eq!(
        variant(19, CREATED_19_3, Some(244)).unwrap(),
        Version::with_minor(19, 3)
    );
    assert_eq!(
        variant(19, CREATED_19_3, None).unwrap(),
        Version::with_minor(19, 3)
    );
    for bad in [8, 15, 21] {
        assert!(matches!(
            variant(bad, 0, None).unwrap_err().kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
    }
    assert_eq!(
        StudioLayout::for_version(Version::new(16)).unwrap(),
        StudioLayout::V16
    );
    assert_eq!(
        StudioLayout::for_version(Version::new(18)).unwrap(),
        StudioLayout::V17
    );
    assert_eq!(
        StudioLayout::for_version(Version::with_minor(19, 1)).unwrap(),
        StudioLayout::V17
    );
    assert_eq!(
        StudioLayout::for_version(Version::with_minor(19, 2)).unwrap(),
        StudioLayout::V19_2
    );
    assert_eq!(
        StudioLayout::for_version(Version::new(20)).unwrap(),
        StudioLayout::V19_2
    );
}

/// A 96-byte asset header and 16-byte CPU data.
fn headers(studio_len: i32) -> (Vec<u8>, Vec<u8>) {
    let mut h = vec![0u8; ASSET_HEADER_LEN];
    h[0..8].copy_from_slice(&0x1000u64.to_le_bytes());
    h[8..16].copy_from_slice(&0x2000u64.to_le_bytes());
    h[0x18..0x20].copy_from_slice(&u64::MAX.to_le_bytes());
    h[0x20..0x28].copy_from_slice(&0x3000u64.to_le_bytes());
    h[0x28..0x2C].copy_from_slice(&2u32.to_le_bytes());
    h[0x2C..0x30].copy_from_slice(&8192u32.to_le_bytes());
    h[0x30..0x34].copy_from_slice(&(-1.0f32).to_le_bytes());
    h[0x3C..0x40].copy_from_slice(&1.0f32.to_le_bytes());
    h[0x4A..0x4C].copy_from_slice(&3u16.to_le_bytes());
    h[0x50..0x58].copy_from_slice(&0x4000u64.to_le_bytes());
    let mut c = vec![0u8; CPU_LEN];
    c[12..16].copy_from_slice(&studio_len.to_le_bytes());
    (h, c)
}

#[test]
fn asset_header() {
    let (h, c) = headers(500);
    let p = RmdlHeader::parse(20, &h, &c, 0, None).unwrap();
    assert_eq!(p.version, Version::new(20));
    assert_eq!(p.layout, StudioLayout::V19_2);
    assert_eq!(
        (p.studio_offset, p.studio_len, p.streaming_size),
        (0x1000, 500, 8192)
    );
    assert_eq!(p.name_offset, Some(0x2000));
    assert_eq!(p.static_data, None);
    assert_eq!(p.bounds, ([-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    assert_eq!(p.anim_rigs, (Some(0x3000), 2));
    assert_eq!(p.anim_seqs, (Some(0x4000), 3));

    assert!(matches!(
        RmdlHeader::parse(15, &h, &c, 0, None).unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
    assert!(matches!(
        RmdlHeader::parse(16, &h[..90], &c, 0, None)
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
    assert!(matches!(
        RmdlHeader::parse(16, &h, &c[..8], 0, None)
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
    let (h0, c0) = headers(0);
    assert!(matches!(
        RmdlHeader::parse(16, &h0, &c0, 0, None).unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));
    let mut no_studio = h.clone();
    no_studio[0..8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        RmdlHeader::parse(16, &no_studio, &c, 0, None)
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
}

fn check_studio(major: u32, layout: StudioLayout) {
    let s = Studio::parse(
        major,
        layout,
        &studio_block(major, layout == StudioLayout::V19_2),
    )
    .unwrap();
    assert_eq!(s.name, "model");
    assert_eq!(s.flags, 0x4000);
    assert_eq!(s.bones.len(), 2);
    assert_eq!(
        (s.bones[0].name.as_str(), s.bones[0].parent),
        ("root", None)
    );
    assert_eq!(
        (s.bones[1].name.as_str(), s.bones[1].parent),
        ("arm", Some(0))
    );
    assert_eq!(s.bones[1].position, [2.0, 2.0, 3.0]);
    assert_eq!(s.bones[1].rotation, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.bones[1].scale, [1.0; 3]);
    assert_eq!(s.materials, vec![0xAAAA, 0xBBBB]);
    assert_eq!(s.skins, vec![vec![0, 1], vec![1, 0]]);
    assert_eq!(s.bodyparts.len(), 1);
    assert_eq!(s.bodyparts[0].name, "weapon");
    assert_eq!(
        s.bodyparts[0].models[0].meshes,
        vec![
            StudioMesh {
                material: 0,
                mesh_id: 0
            },
            StudioMesh {
                material: 1,
                mesh_id: 1
            }
        ]
    );
    assert_eq!(s.lod_switch, vec![10.0, 50.0]);
    assert_eq!(s.groups.len(), 1);
    assert_eq!(
        (
            s.groups[0].size_compressed,
            s.groups[0].lod_count,
            s.groups[0].lod_map
        ),
        (100, 2, 0b11)
    );
    assert_eq!(s.lod_slots, vec![Some((0, 0)), Some((0, 1))]);
    assert_eq!(s.bone_map, vec![1, 0]);
    // skin 1 swaps the materials
    assert_eq!(
        s.material_of(0, &s.bodyparts[0].models[0].meshes[0])
            .unwrap(),
        0xAAAA
    );
    assert_eq!(
        s.material_of(1, &s.bodyparts[0].models[0].meshes[0])
            .unwrap(),
        0xBBBB
    );
    assert!(matches!(
        s.material_of(2, &s.bodyparts[0].models[0].meshes[0])
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound { .. }
    ));
}

#[test]
fn studio_v16() {
    check_studio(16, StudioLayout::V16);
}

#[test]
fn studio_v18() {
    check_studio(18, StudioLayout::V17);
}

#[test]
fn studio_v19_linear_bones() {
    check_studio(19, StudioLayout::V17);
}

#[test]
fn studio_v20_wide_fields() {
    check_studio(20, StudioLayout::V19_2);
}

#[test]
fn studio_is_bounded() {
    let block = studio_block(16, false);
    for len in [0, 10, 0x74, 0x120, 0x290, 0x328] {
        assert!(
            Studio::parse(16, StudioLayout::V16, &block[..len]).is_err(),
            "len {len}"
        );
    }
    let mut bad_parent = block.clone();
    bad_parent[0x140 + 116..0x140 + 118].copy_from_slice(&5i16.to_le_bytes());
    assert!(matches!(
        Studio::parse(16, StudioLayout::V16, &bad_parent)
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
    let mut too_many = block;
    put16(&mut too_many, 0x74, 5000);
    assert!(matches!(
        Studio::parse(16, StudioLayout::V16, &too_many)
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
}

#[test]
fn layouts() {
    assert_eq!(
        vertex_layout(flags(1, true, true, 2), 12 + 8 + 4 + 4 + 16).unwrap(),
        VertexLayout {
            position: 1,
            blend: true,
            color: true,
            uv_layers: 2
        }
    );
    assert_eq!(
        vertex_layout(flags(2, false, false, 1), 8 + 4 + 8)
            .unwrap()
            .position,
        2
    );
    assert!(matches!(
        vertex_layout(flags(3, false, false, 0), 10)
            .unwrap_err()
            .kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
    assert!(matches!(
        vertex_layout(flags(0, false, false, 0), 4)
            .unwrap_err()
            .kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
    // stride that does not match
    assert!(matches!(
        vertex_layout(flags(1, false, false, 0), 20)
            .unwrap_err()
            .kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
    // an unknown texcoord format
    assert!(vertex_layout(flags(1, false, false, 0) | (5 << 24), 16).is_err());
    // VERT_UNK (0x40) takes no bytes, but a stride that does not fit is still refused
    assert_eq!(
        vertex_layout(flags(1, false, false, 0) | 0x40, 16)
            .unwrap()
            .position,
        1
    );
    assert!(vertex_layout(flags(1, false, false, 0) | 0x40, 24).is_err());
    assert!(
        vertex_layout(flags(1, false, false, 0) | 0x2000, 16).is_err(),
        "unpacked weights"
    );
}

fn cx_plain<'a>(map: &'a [u16], bones: usize, extra: bool, wide: bool) -> DecodeCx<'a> {
    DecodeCx {
        bone_map: map,
        bone_count: bones,
        extra_weights: extra,
        wide_bones: wide,
    }
}

#[test]
fn decodes_an_unskinned_mesh() {
    // Vector64 position, no blend, normal, 1 UV: 8 + 4 + 8 = 20
    let f = flags(2, false, false, 1);
    let mut verts = Vec::new();
    for i in 0..3u64 {
        verts.extend_from_slice(&((1024 * (i + 1)) | (2048 << 21) | (4096 << 42)).to_le_bytes());
        verts.extend_from_slice(&normal_z().to_le_bytes());
        verts.extend_from_slice(&(0.25f32 * i as f32).to_le_bytes());
        verts.extend_from_slice(&0.5f32.to_le_bytes());
    }
    let g = vg_group(f, 20, 3, &verts, &[0, 1, 2], &[]);
    let headers = vg_meshes(&g, 0).unwrap();
    assert_eq!(headers.len(), 1);
    let part = decode_mesh(&g, &headers[0], 9, &cx_plain(&[], 1, false, false)).unwrap();
    assert_eq!(part.material, 9);
    assert_eq!(
        part.positions,
        vec![
            [-1023.0, -1022.0, -2044.0],
            [-1022.0, -1022.0, -2044.0],
            [-1021.0, -1022.0, -2044.0]
        ]
    );
    assert!(part.normals.iter().all(|n| (n[2] - 1.0).abs() < 1e-6));
    assert_eq!(part.uvs, vec![vec![[0.0, 0.5], [0.25, 0.5], [0.5, 0.5]]]);
    assert_eq!(part.triangles, vec![[0, 1, 2]]);
    assert_eq!(part.influences, 0);
    assert!(part.bone_indices.is_empty());
}

#[test]
fn decodes_blend_weights() {
    let f = flags(1, true, false, 0);
    let mut verts = Vec::new();
    verts.extend(blend_vertex(0, 0, [0, 0, 0, 0])); // one bone, remaining weight 1.0
    verts.extend(blend_vertex(16383, 0, [0, 1, 0, 1])); // two bones
    verts.extend(blend_vertex(10000, 12000, [1, 0, 1, 2])); // three bones
    let g = vg_group(f, 24, 3, &verts, &[0, 1, 2], &[]);
    let h = vg_meshes(&g, 0).unwrap();
    let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 2, false, false)).unwrap();
    assert_eq!(part.influences, 3);
    assert_eq!(part.bone_indices, vec![0, 0, 0, 0, 1, 0, 1, 0, 1]);
    let w = &part.bone_weights;
    assert_eq!(w[0], unpack_weight(32767)); // 1.0
    assert_eq!(w[1], 0.0, "padding");
    for v in 0..3 {
        let sum: f32 = w[v * 3..v * 3 + 3].iter().sum();
        assert!((sum - 1.0).abs() < 4.0 / 32768.0, "vertex {v}: {sum}");
    }
    // The bone map redirects, and a bone outside it is corrupt.
    let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[1, 0], 2, false, false)).unwrap();
    assert_eq!(part.bone_indices[3..6], [1, 0, 0]);
    let err = decode_mesh(&g, &h[0], 1, &cx_plain(&[1], 2, false, false)).unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
    // a skeleton too small for the bones
    let err = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 1, false, false)).unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
}

#[test]
fn decodes_extra_weights() {
    let f = flags(1, true, false, 0);
    // 256-bone form: first 2, last 5, unk, count 3 -> weights: first, extras[w1..w1+1], last
    let extra: Vec<u8> = [(8000i16, 3i16), (4000, 4)]
        .iter()
        .flat_map(|(w, b)| w.to_le_bytes().into_iter().chain(b.to_le_bytes()))
        .collect();
    let verts = blend_vertex(10000, 0, [2, 5, 0, 3]);
    let g = vg_group(f, 24, 1, &verts, &[], &extra);
    let h = vg_meshes(&g, 0).unwrap();
    let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 8, true, false)).unwrap();
    assert_eq!(part.influences, 4);
    assert_eq!(part.bone_indices, vec![2, 3, 4, 5]);
    let sum: f32 = part.bone_weights.iter().sum();
    assert!((sum - 1.0).abs() < 8.0 / 32768.0, "{sum}");
    assert_eq!(part.bone_weights[0], unpack_weight(10000));
    assert_eq!(part.bone_weights[1], unpack_weight(8000));

    // 1024-bone form: first 700, last 900, count 2
    let packed: u32 = 700 | (900 << 10) | (2 << 24);
    let mut idx = packed.to_le_bytes();
    let verts = blend_vertex(20000, 0, idx);
    idx = [0; 4];
    let _ = idx;
    let extra: Vec<u8> = [(6000i16, 800i16)]
        .iter()
        .flat_map(|(w, b)| w.to_le_bytes().into_iter().chain(b.to_le_bytes()))
        .collect();
    let g = vg_group(f, 24, 1, &verts, &[], &extra);
    let h = vg_meshes(&g, 0).unwrap();
    let part = decode_mesh(&g, &h[0], 1, &cx_plain(&[], 1024, true, true)).unwrap();
    assert_eq!(part.bone_indices, vec![700, 800, 900]);
}

#[test]
fn mesh_decoding_rejects_bad_data() {
    let f = flags(1, false, false, 0);
    let mut verts = Vec::new();
    for _ in 0..3 {
        verts.extend_from_slice(&[0u8; 12]);
        verts.extend_from_slice(&normal_z().to_le_bytes());
    }
    let cx = cx_plain(&[], 1, false, false);
    // not whole triangles
    let g = vg_group(f, 16, 3, &verts, &[0, 1], &[]);
    assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
    // a triangle past the vertices
    let g = vg_group(f, 16, 3, &verts, &[0, 1, 3], &[]);
    assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
    // a vertex buffer that does not fit
    let g = vg_group(f, 16, 4, &verts, &[0, 1, 2], &[]);
    assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
    // a stride that does not match the flags
    let g = vg_group(f, 20, 3, &verts, &[0, 1, 2], &[]);
    assert!(decode_mesh(&g, &vg_meshes(&g, 0).unwrap()[0], 1, &cx).is_err());
    // a group header past the data
    assert!(vg_meshes(&g[..6], 0).is_err());
    assert!(vg_meshes(&g, 3).is_err());
}
