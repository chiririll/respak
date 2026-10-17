use respak_core::capability::{Bone, MaterialTexture, MeshMaterial, MeshPart};
use respak_core::ErrorKind;

use super::anim::root_fixup;
use super::*;

/// A parsed node, for tests.
#[derive(Debug)]
struct Parsed {
    id: String,
    size: u32,
    hash: u64,
    props: Vec<(Vec<u8>, String, u32, Vec<u8>)>,
    children: Vec<Parsed>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn type_len(ty: &[u8]) -> usize {
    match ty {
        b"b\0" => 1,
        b"h\0" => 2,
        b"i\0" | b"f\0" => 4,
        b"l\0" | b"d\0" => 8,
        b"v2" => 8,
        b"v3" => 12,
        b"v4" => 16,
        _ => panic!("type {ty:?}"),
    }
}

fn parse_node(b: &[u8], at: usize) -> (Parsed, usize) {
    let id = String::from_utf8(b[at..at + 4].to_vec()).unwrap();
    let size = u32_at(b, at + 4);
    let hash = u64::from_le_bytes(b[at + 8..at + 16].try_into().unwrap());
    let (pc, cc) = (u32_at(b, at + 16), u32_at(b, at + 20));
    let mut p = at + 24;
    let mut props = Vec::new();
    for _ in 0..pc {
        let ty = b[p..p + 2].to_vec();
        let name_len = u16::from_le_bytes([b[p + 2], b[p + 3]]) as usize;
        let arr = u32_at(b, p + 4);
        let name = String::from_utf8(b[p + 8..p + 8 + name_len].to_vec()).unwrap();
        let start = p + 8 + name_len;
        let len = if ty == b"s\0" {
            b[start..].iter().position(|&c| c == 0).unwrap() + 1
        } else {
            type_len(&ty) * arr as usize
        };
        props.push((ty, name, arr, b[start..start + len].to_vec()));
        p = start + len;
    }
    let mut children = Vec::new();
    for _ in 0..cc {
        let (c, next) = parse_node(b, p);
        children.push(c);
        p = next;
    }
    assert_eq!(
        p - at,
        size as usize,
        "node {id}: nodeSize covers header, properties and children"
    );
    (
        Parsed {
            id,
            size,
            hash,
            props,
            children,
        },
        p,
    )
}

fn parse(bytes: &[u8]) -> Parsed {
    assert_eq!(&bytes[0..4], b"cast");
    assert_eq!(u32_at(bytes, 4), 1);
    assert_eq!(u32_at(bytes, 8), 1);
    assert_eq!(u32_at(bytes, 12), 0);
    let (root, end) = parse_node(bytes, 16);
    assert_eq!(end, bytes.len());
    root
}

fn prop<'a>(n: &'a Parsed, name: &str) -> &'a (Vec<u8>, String, u32, Vec<u8>) {
    n.props
        .iter()
        .find(|p| p.1 == name)
        .unwrap_or_else(|| panic!("no property {name} in {}", n.id))
}

fn bone(name: &str, parent: Option<u32>) -> Bone {
    Bone {
        name: name.into(),
        parent,
        position: [1.0, 2.0, 3.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    }
}

fn part(vertices: usize) -> MeshPart {
    MeshPart {
        material: 7,
        positions: (0..vertices).map(|i| [i as f32, 0.0, 0.0]).collect(),
        normals: vec![[0.0, 0.0, 1.0]; vertices],
        uvs: vec![vec![[0.5, 0.25]; vertices]],
        triangles: vec![[0, 1, 2]],
        ..MeshPart::default()
    }
}

#[test]
fn skeleton_only_file() {
    let mut w = CastWriter::new();
    w.model("m", &[bone("root", None), bone("child", Some(0))]);
    let root = parse(&w.finish());
    assert_eq!(root.id, "root");
    let modl = &root.children[0];
    assert_eq!(modl.id, "modl");
    let skel = &modl.children[0];
    assert_eq!(skel.id, "skel");
    assert_eq!(skel.children.len(), 2);
    let b1 = &skel.children[1];
    assert_eq!(b1.id, "bone");
    assert_eq!(prop(b1, "n").3, b"child\0");
    assert_eq!(prop(b1, "p").3, 0u32.to_le_bytes());
    assert_eq!(prop(&skel.children[0], "p").3, u32::MAX.to_le_bytes());
    assert_eq!(prop(b1, "lp").0, b"v3");
    assert_eq!(prop(b1, "lr").0, b"v4");
    assert_eq!(prop(b1, "lr").3.len(), 16);
}

#[test]
fn mesh_streams_and_reversed_winding() {
    let mut w = CastWriter::new();
    w.model("m", &[bone("root", None)]);
    let mut p = part(3);
    p.influences = 2;
    p.bone_indices = vec![0, 0, 0, 0, 0, 0];
    p.bone_weights = vec![1.0, 0.0, 0.5, 0.5, 1.0, 0.0];
    p.colors = vec![[1, 2, 3, 4]; 3];
    w.mesh("m_mat", &p, 1).unwrap();
    let root = parse(&w.finish());
    let mesh = root.children[0]
        .children
        .iter()
        .find(|c| c.id == "mesh")
        .unwrap();
    assert_eq!(prop(mesh, "vp").2, 3);
    assert_eq!(prop(mesh, "vp").3.len(), 36);
    assert_eq!(prop(mesh, "u0").0, b"v2");
    assert_eq!(prop(mesh, "wb").0, b"b\0");
    assert_eq!(prop(mesh, "wb").2, 6);
    assert_eq!(prop(mesh, "wv").3.len(), 24);
    assert_eq!(prop(mesh, "vc").3, vec![1, 2, 3, 4, 1, 2, 3, 4, 1, 2, 3, 4]);
    assert_eq!(prop(mesh, "f").3, vec![2, 0, 1, 0, 0, 0]);
    assert_eq!(prop(mesh, "ul").3, 1u16.to_le_bytes());
    assert_eq!(prop(mesh, "mi").3, 2u16.to_le_bytes());
    assert_eq!(prop(mesh, "m").3, 7u64.to_le_bytes());
}

#[test]
fn more_than_256_bones_use_16_bit_indices() {
    let mut w = CastWriter::new();
    w.model("m", &[]);
    let mut p = part(3);
    p.influences = 1;
    p.bone_indices = vec![300, 1, 2];
    p.bone_weights = vec![1.0; 3];
    w.mesh("x", &p, 400).unwrap();
    let root = parse(&w.finish());
    let mesh = root.children[0]
        .children
        .iter()
        .find(|c| c.id == "mesh")
        .unwrap();
    assert_eq!(prop(mesh, "wb").0, b"h\0");
    assert_eq!(&prop(mesh, "wb").3[..2], &300u16.to_le_bytes());
}

#[test]
fn unskinned_mesh_has_no_weight_streams() {
    let mut w = CastWriter::new();
    w.model("m", &[]);
    w.mesh("x", &part(3), 0).unwrap();
    let root = parse(&w.finish());
    let mesh = root.children[0]
        .children
        .iter()
        .find(|c| c.id == "mesh")
        .unwrap();
    assert!(mesh
        .props
        .iter()
        .all(|p| p.1 != "wb" && p.1 != "wv" && p.1 != "mi" && p.1 != "vc"));
}

#[test]
fn too_many_vertices_and_bad_streams_are_errors() {
    let mut w = CastWriter::new();
    let big = MeshPart {
        positions: vec![[0.0; 3]; 65537],
        normals: vec![[0.0; 3]; 65537],
        ..MeshPart::default()
    };
    assert!(w.mesh("big", &big, 0).is_err());
    let mut short = part(3);
    short.normals.pop();
    assert!(w.mesh("short", &short, 0).is_err());
    let mut out_of_range = part(3);
    out_of_range.triangles = vec![[0, 1, 3]];
    assert!(w.mesh("range", &out_of_range, 0).is_err());
    let mut weights = part(3);
    weights.influences = 2;
    assert!(w.mesh("weights", &weights, 0).is_err());
    assert!(w.mesh_count() == 0, "a failed mesh adds nothing");
}

#[test]
fn material_is_written_once_with_known_slots_only() {
    let mat = MeshMaterial {
        guid: 7,
        name: "wall".into(),
        textures: vec![
            MaterialTexture {
                slot: "albedoTexture".into(),
                guid: 100,
                texture: None,
            },
            MaterialTexture {
                slot: "weirdTexture".into(),
                guid: 101,
                texture: None,
            },
            MaterialTexture {
                slot: "normalTexture".into(),
                guid: 102,
                texture: None,
            },
        ],
    };
    let files = vec![
        (100, "textures/wall/albedoTexture.png".to_owned()),
        (101, "x.png".to_owned()),
    ];
    let mut w = CastWriter::new();
    w.model("m", &[]);
    w.material(&mat, &files);
    w.material(&mat, &files);
    let root = parse(&w.finish());
    let matls: Vec<_> = root.children[0]
        .children
        .iter()
        .filter(|c| c.id == "matl")
        .collect();
    assert_eq!(matls.len(), 1);
    let m = matls[0];
    assert_eq!(m.hash, 7);
    assert_eq!(prop(m, "n").3, b"wall\0");
    assert_eq!(prop(m, "t").3, b"pbr\0");
    assert_eq!(prop(m, "albedo").3, 100u64.to_le_bytes());
    assert!(m.props.iter().all(|p| p.1 != "normal"), "no file, no slot");
    assert_eq!(m.children.len(), 1);
    assert_eq!(m.children[0].id, "file");
    assert_eq!(m.children[0].hash, 100);
    assert_eq!(
        prop(&m.children[0], "p").3,
        b"textures/wall/albedoTexture.png\0"
    );
}

#[test]
fn children_come_in_skel_matl_mesh_order() {
    let mut w = CastWriter::new();
    w.model("m", &[bone("r", None)]);
    w.mesh("a", &part(3), 1).unwrap();
    w.material(
        &MeshMaterial {
            guid: 1,
            name: "x".into(),
            textures: vec![],
        },
        &[],
    );
    let root = parse(&w.finish());
    let ids: Vec<_> = root.children[0]
        .children
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids, ["skel", "matl", "mesh"]);
    assert!(root.size > 0);
    assert_ne!(root.children[0].hash, 0);
}

#[test]
fn lod_select() {
    assert_eq!(LodSelect::default().resolve(3).unwrap(), vec![0]);
    assert_eq!(
        LodSelect::All(AllLods::All).resolve(3).unwrap(),
        vec![0, 1, 2]
    );
    assert_eq!(
        LodSelect::List(vec![2, 0, 2]).resolve(3).unwrap(),
        vec![0, 2]
    );
    assert!(matches!(
        LodSelect::One(3).resolve(3).unwrap_err().kind(),
        ErrorKind::InvalidOptions { .. }
    ));
    let parsed: CastOptions = serde_json::from_str(r#"{"lods":"all","skin":2}"#).unwrap();
    assert_eq!(parsed.lods, LodSelect::All(AllLods::All));
    let parsed: CastOptions = serde_json::from_str(r#"{"lods":[0,1]}"#).unwrap();
    assert_eq!(parsed.lods, LodSelect::List(vec![0, 1]));
    let parsed: CastOptions = serde_json::from_str(r#"{"lods":1}"#).unwrap();
    assert_eq!(parsed.lods, LodSelect::One(1));
}

#[test]
fn stems() {
    assert_eq!(stem_of("mdl/props/crate_01.rmdl"), "crate_01");
    assert_eq!(stem_of("plain"), "plain");
}

fn curves(root: &Parsed) -> Vec<&Parsed> {
    root.children[0]
        .children
        .iter()
        .filter(|c| c.id == "curv")
        .collect()
}

#[test]
fn animation_file_has_anim_node_skeleton_and_curves() {
    let mut w = CastWriter::new();
    w.animation(
        "walk",
        24.0,
        true,
        &[bone("root", None), bone("child", Some(0))],
    );
    w.curve("root", "tx", "absolute", None, 3, &[1.0, 2.0, 3.0])
        .unwrap();
    w.curve(
        "child",
        "rq",
        "absolute",
        None,
        2,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
    .unwrap();
    let root = parse(&w.finish());
    let anim = &root.children[0];
    assert_eq!(anim.id, "anim");
    assert_ne!(anim.hash, 0);
    assert_eq!(prop(anim, "fr").0, b"f\0");
    assert_eq!(prop(anim, "fr").3, 24.0f32.to_le_bytes());
    assert_eq!(prop(anim, "lo").0, b"b\0");
    assert_eq!(prop(anim, "lo").3, [1]);
    assert!(anim.props.iter().all(|p| p.1 != "n"), "rsx writes no name");
    let ids: Vec<_> = anim.children.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["skel", "curv", "curv"]);
    assert_eq!(anim.children[0].children.len(), 2);

    let c = curves(&root);
    let tx = c[0];
    assert_eq!(prop(tx, "nn").3, b"root\0");
    assert_eq!(prop(tx, "kp").3, b"tx\0");
    assert_eq!(
        (
            prop(tx, "kb").0.as_slice(),
            prop(tx, "kb").2,
            prop(tx, "kb").3.as_slice()
        ),
        (&b"b\0"[..], 3, &[0u8, 1, 2][..])
    );
    assert_eq!(
        (prop(tx, "kv").0.as_slice(), prop(tx, "kv").2),
        (&b"f\0"[..], 3)
    );
    assert_eq!(
        prop(tx, "kv").3,
        [1.0f32, 2.0, 3.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>()
    );
    assert_eq!(prop(tx, "m").3, b"absolute\0");
    assert!(
        tx.props.iter().all(|p| p.1 != "ab"),
        "no weight in absolute mode"
    );

    let rq = c[1];
    assert_eq!(prop(rq, "nn").3, b"child\0");
    assert_eq!(
        (
            prop(rq, "kv").0.as_slice(),
            prop(rq, "kv").2,
            prop(rq, "kv").3.len()
        ),
        (&b"v4"[..], 2, 32)
    );

    let mut looped = CastWriter::new();
    looped.animation("x", 30.0, false, &[]);
    assert_eq!(prop(&parse(&looped.finish()).children[0], "lo").3, [0]);
}

#[test]
fn additive_curves_carry_their_weight() {
    let mut w = CastWriter::new();
    w.animation("delta", 30.0, false, &[bone("root", None)]);
    w.curve("root", "ty", "additive", Some(0.25), 1, &[5.0])
        .unwrap();
    w.curve("root", "tz", "additive", None, 1, &[5.0]).unwrap();
    w.curve("root", "sx", "absolute", Some(0.5), 1, &[5.0])
        .unwrap();
    let root = parse(&w.finish());
    let c = curves(&root);
    assert_eq!(prop(c[0], "m").3, b"additive\0");
    assert_eq!(
        (prop(c[0], "ab").0.as_slice(), prop(c[0], "ab").3.as_slice()),
        (&b"f\0"[..], &0.25f32.to_le_bytes()[..])
    );
    assert_eq!(
        prop(c[1], "ab").3,
        1.0f32.to_le_bytes(),
        "no weight means 1"
    );
    assert!(c[2].props.iter().all(|p| p.1 != "ab"));
}

#[test]
fn key_buffer_width_follows_the_frame_count() {
    for (frames, ty) in [
        (1u32, b"b\0"),
        (255, b"b\0"),
        (256, b"h\0"),
        (65535, b"h\0"),
        (65536, b"i\0"),
    ] {
        let mut w = CastWriter::new();
        w.animation("a", 30.0, false, &[]);
        w.curve(
            "b",
            "tx",
            "absolute",
            None,
            frames,
            &vec![0.5; frames as usize],
        )
        .unwrap();
        let root = parse(&w.finish());
        let kb = prop(curves(&root)[0], "kb");
        assert_eq!(kb.0, ty, "{frames} frames");
        assert_eq!(kb.2, frames);
        let width = match ty {
            b"b\0" => 1,
            b"h\0" => 2,
            _ => 4,
        };
        assert_eq!(kb.3.len(), frames as usize * width);
        // the last key is the last frame
        let last = &kb.3[kb.3.len() - width..];
        let mut bytes = [0u8; 4];
        bytes[..width].copy_from_slice(last);
        assert_eq!(u32::from_le_bytes(bytes), frames - 1);
    }
}

#[test]
fn bad_curves_are_errors_and_add_nothing() {
    let mut w = CastWriter::new();
    w.animation("a", 30.0, false, &[]);
    assert!(
        w.curve("b", "tx", "absolute", None, 3, &[1.0, 2.0])
            .is_err(),
        "too few values"
    );
    assert!(
        w.curve("b", "rq", "absolute", None, 2, &[0.0; 4]).is_err(),
        "a quaternion has four floats"
    );
    assert!(
        w.curve("b", "tw", "absolute", None, 1, &[0.0]).is_err(),
        "unknown key"
    );
    assert!(
        w.curve("b", "tx", "relative", None, 1, &[0.0]).is_err(),
        "unknown mode"
    );
    assert!(curves(&parse(&w.finish())).is_empty());
}

#[test]
fn zero_frames_make_empty_curves() {
    let mut w = CastWriter::new();
    w.animation("a", 30.0, false, &[]);
    w.curve("b", "tx", "absolute", None, 0, &[]).unwrap();
    let root = parse(&w.finish());
    assert_eq!(prop(curves(&root)[0], "kb").2, 0);
}

fn euler(roll: f32, pitch: f32, yaw: f32) -> [f32; 4] {
    let (sr, cr) = (roll * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    let (sy, cy) = (yaw * 0.5).sin_cos();
    [
        sr * cp * cy - cr * sp * sy,
        cr * sp * cy + sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}

fn near(a: &[f32], b: &[f32]) {
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-5, "{a:?} != {b:?}");
    }
}

#[test]
fn root_fixup_turns_the_yaw_by_minus_90_and_swaps_the_plane() {
    let (p, q) = root_fixup([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(p, [2.0, -1.0, 3.0]);
    let s = std::f32::consts::FRAC_1_SQRT_2;
    near(&q, &[0.0, 0.0, -s, s]);

    // the same as lowering the Source yaw by 90 degrees
    let (_, q) = root_fixup([0.0; 3], euler(0.3, 0.2, 0.5));
    near(&q, &euler(0.3, 0.2, 0.5 - std::f32::consts::FRAC_PI_2));
    let (_, q) = root_fixup([0.0; 3], euler(0.0, 0.0, std::f32::consts::FRAC_PI_2));
    near(&q, &[0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn blend_select() {
    assert_eq!(BlendSelect::default().resolve(3).unwrap(), vec![0, 1, 2]);
    assert_eq!(
        BlendSelect::default().resolve(0).unwrap(),
        Vec::<u32>::new()
    );
    assert_eq!(BlendSelect::One(1).resolve(3).unwrap(), vec![1]);
    assert!(matches!(
        BlendSelect::One(3).resolve(3).unwrap_err().kind(),
        ErrorKind::InvalidOptions { .. }
    ));
    let parsed: CastOptions = serde_json::from_str(r#"{"blend":2,"root_fixup":"none"}"#).unwrap();
    assert_eq!(
        (parsed.blend, parsed.root_fixup),
        (BlendSelect::One(2), RootFixup::None)
    );
    let parsed: CastOptions = serde_json::from_str(r#"{"blend":"all"}"#).unwrap();
    assert_eq!(
        (parsed.blend, parsed.root_fixup),
        (BlendSelect::default(), RootFixup::Rsx)
    );
    assert!(serde_json::from_str::<CastOptions>(r#"{"root_fixup":"flip"}"#).is_err());
    assert!(serde_json::from_str::<CastOptions>(r#"{"blend":"some"}"#).is_err());
}
