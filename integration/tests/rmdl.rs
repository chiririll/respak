//! A model: `rmdl -> cast` and `rmdl -> rmdl.json` over synthetic paks (slice 18).

use opendal::{services::Memory, Operator};
use respak_core::{CancellationToken, ConvertRequest, Engine, ErrorKind, Event, OutputResult};
use respak_formats::FormatsPlugin;
use respak_respawn::testing::rmdl::{blend_vertex, flags, normal_z, studio, vg, VgMeshSpec};
use respak_respawn::testing::PakBuilder;
use respak_respawn::RespawnPlugin;
use serde_json::{json, Value};

const WALL: u64 = 0xAAAA;
const FLOOR: u64 = 0xBBBB;
const MODEL: &str = "paks/lvl.rpak/0x0000000000000100.mdl_";

struct Env {
    input: Operator,
    output: Operator,
}

fn mem() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

fn engine(env: &Env) -> Engine {
    Engine::builder()
        .input(env.input.clone())
        .output(env.output.clone())
        .plugin(FormatsPlugin)
        .plugin(RespawnPlugin::new())
        .build()
        .unwrap()
}

/// The vertex groups of the test model: 2 LODs. LOD 0: a skinned mesh of 3 vertices (`Vec3`
/// positions, extra-weight blend, 1 UV layer) and a `Vector64` mesh of 4 vertices. LOD 1: a
/// plain mesh of 3 vertices and an empty mesh.
fn model_vg() -> Vec<u8> {
    let mut skinned = Vec::new();
    for (i, bone) in [0u8, 1, 1].iter().enumerate() {
        let mut v = blend_vertex(32767, 0, [*bone, 0, 0, 0]);
        v[0..4].copy_from_slice(&(i as f32).to_le_bytes());
        skinned.extend(v);
        skinned.extend_from_slice(&(0.25 * i as f32).to_le_bytes());
        skinned.extend_from_slice(&0.5f32.to_le_bytes());
    }
    let mut packed = Vec::new();
    for i in 0..4u64 {
        packed.extend_from_slice(&((1024 * (i + 1)) | (2048 << 21) | (4096 << 42)).to_le_bytes());
        packed.extend_from_slice(&normal_z().to_le_bytes());
        packed.extend_from_slice(&0.0f32.to_le_bytes());
        packed.extend_from_slice(&1.0f32.to_le_bytes());
    }
    let mut plain = Vec::new();
    for i in 0..3 {
        plain.extend_from_slice(
            &[
                (i as f32).to_le_bytes(),
                1.0f32.to_le_bytes(),
                2.0f32.to_le_bytes(),
            ]
            .concat(),
        );
        plain.extend_from_slice(&normal_z().to_le_bytes());
    }
    vg(&[
        vec![
            VgMeshSpec {
                flags: flags(1, true, false, 1),
                stride: 32,
                vert_count: 3,
                verts: skinned,
                indices: vec![0, 1, 2],
                extra: vec![],
            },
            VgMeshSpec {
                flags: flags(2, false, false, 1),
                stride: 20,
                vert_count: 4,
                verts: packed,
                indices: vec![0, 1, 2, 2, 1, 3],
                extra: vec![],
            },
        ],
        vec![
            VgMeshSpec {
                flags: flags(1, false, false, 0),
                stride: 16,
                vert_count: 3,
                verts: plain,
                indices: vec![0, 1, 2],
                extra: vec![],
            },
            VgMeshSpec {
                flags: 0,
                stride: 0,
                vert_count: 0,
                verts: vec![],
                indices: vec![],
                extra: vec![],
            },
        ],
    ])
}

/// `paks/lvl.rpak`: two materials (no textures) and the model, of asset version `major`.
async fn env_with(major: u32) -> Env {
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let group = model_vg();
    let mut pak = PakBuilder::new();
    pak.matl(WALL, "material/wall", 0, &[]);
    pak.matl(FLOOR, "material/floor", 0, &[]);
    pak.rmdl(
        0x100,
        "mdl/props/crate.rmdl",
        major,
        &studio(major, major >= 20, group.len() as u32),
        &group,
    );
    env.input
        .write("paks/lvl.rpak", pak.build(8))
        .await
        .unwrap();
    env
}

fn request(to: Value, path: &str) -> ConvertRequest {
    serde_json::from_value(
        json!({ "version": 1, "outputs": [{ "from": MODEL, "to": to, "path": path }] }),
    )
    .unwrap()
}

async fn convert(engine: &Engine, to: Value, path: &str) -> (OutputResult, Vec<Event>) {
    let mut events = engine.events();
    let report = engine
        .convert(&request(to, path), &CancellationToken::new())
        .await
        .unwrap();
    let mut seen = Vec::new();
    while let Ok(e) = events.try_recv() {
        seen.push(e);
    }
    (report.outputs.into_iter().next().unwrap().result, seen)
}

fn warnings(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| {
            if let Event::Warning { message } = e {
                Some(message.as_str())
            } else {
                None
            }
        })
        .filter(|m| !m.starts_with("path "))
        .collect()
}

// ---- a small Cast reader ----------------------------------------------------------------------

#[derive(Debug)]
struct Node {
    id: String,
    props: Vec<(String, Vec<u8>)>,
    children: Vec<Node>,
}

impl Node {
    fn prop(&self, name: &str) -> &[u8] {
        &self
            .props
            .iter()
            .find(|p| p.0 == name)
            .unwrap_or_else(|| panic!("no property {name} in {}", self.id))
            .1
    }
    fn kids(&self, id: &str) -> Vec<&Node> {
        self.children.iter().filter(|c| c.id == id).collect()
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn parse_node(b: &[u8], at: usize) -> (Node, usize) {
    let id = String::from_utf8(b[at..at + 4].to_vec()).unwrap();
    let size = u32_at(b, at + 4) as usize;
    let (pc, cc) = (u32_at(b, at + 16), u32_at(b, at + 20));
    let mut p = at + 24;
    let mut props = Vec::new();
    for _ in 0..pc {
        let ty = &b[p..p + 2];
        let name_len = u16::from_le_bytes([b[p + 2], b[p + 3]]) as usize;
        let arr = u32_at(b, p + 4) as usize;
        let name = String::from_utf8(b[p + 8..p + 8 + name_len].to_vec()).unwrap();
        let start = p + 8 + name_len;
        let len = match ty {
            b"s\0" => b[start..].iter().position(|&c| c == 0).unwrap() + 1,
            b"b\0" => arr,
            b"h\0" => 2 * arr,
            b"i\0" | b"f\0" => 4 * arr,
            b"l\0" | b"d\0" | b"v2" => 8 * arr,
            b"v3" => 12 * arr,
            b"v4" => 16 * arr,
            other => panic!("type {other:?}"),
        };
        props.push((name, b[start..start + len].to_vec()));
        p = start + len;
    }
    let mut children = Vec::new();
    for _ in 0..cc {
        let (c, next) = parse_node(b, p);
        children.push(c);
        p = next;
    }
    assert_eq!(p - at, size, "{id}: nodeSize");
    (
        Node {
            id,
            props,
            children,
        },
        p,
    )
}

async fn cast_of(env: &Env, path: &str) -> Node {
    let bytes = env.output.read(path).await.unwrap().to_vec();
    assert_eq!(&bytes[..4], b"cast");
    let (root, end) = parse_node(&bytes, 16);
    assert_eq!(end, bytes.len());
    root
}

fn floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn u16s(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

// ---- tests --------------------------------------------------------------------------------------

#[tokio::test]
async fn a_model_converts_to_cast_lod_0_by_default() {
    for major in [16, 20] {
        let env = env_with(major).await;
        let (result, events) = convert(&engine(&env), json!({ "type": "cast" }), "out/crate").await;
        let OutputResult::Ok { files, .. } = result else {
            panic!("{result:?}")
        };
        assert_eq!(files, ["out/crate/weapon_0_LOD0.cast"], "v{major}");
        assert!(warnings(&events).is_empty(), "{:?}", warnings(&events));

        let root = cast_of(&env, &files[0]).await;
        let modl = &root.children[0];
        let skel = modl.kids("skel")[0];
        let bones = skel.kids("bone");
        assert_eq!(bones.len(), 2);
        assert_eq!(bones[0].prop("n"), b"root\0");
        assert_eq!(bones[0].prop("p"), u32::MAX.to_le_bytes());
        assert_eq!(bones[1].prop("n"), b"arm\0");
        assert_eq!(bones[1].prop("p"), 0u32.to_le_bytes());
        assert_eq!(floats(bones[1].prop("lp")), [2.0, 2.0, 3.0]);

        let matls = modl.kids("matl");
        let names: Vec<_> = matls.iter().map(|m| m.prop("n").to_vec()).collect();
        assert_eq!(names, [b"wall\0".to_vec(), b"floor\0".to_vec()]);

        let meshes = modl.kids("mesh");
        assert_eq!(meshes.len(), 2);
        // mesh 0: skinned, 3 vertices, bone map [1, 0] sends vertex bones 0, 1, 1 to 1, 0, 0
        assert_eq!(meshes[0].prop("n"), b"weapon_0_LOD0_wall\0");
        assert_eq!(
            floats(meshes[0].prop("vp")),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]
        );
        assert_eq!(meshes[0].prop("wb"), [1, 0, 0]);
        assert_eq!(floats(meshes[0].prop("wv")), [1.0, 1.0, 1.0]);
        assert_eq!(u16s(meshes[0].prop("f")), [2, 1, 0], "winding is reversed");
        assert_eq!(
            floats(meshes[0].prop("u0")),
            [0.0, 0.5, 0.25, 0.5, 0.5, 0.5]
        );
        assert_eq!(meshes[0].prop("m"), WALL.to_le_bytes());
        // mesh 1: Vector64 positions
        assert_eq!(
            floats(meshes[1].prop("vp"))[..3],
            [-1023.0, -1022.0, -2044.0]
        );
        assert_eq!(u16s(meshes[1].prop("f")), [2, 1, 0, 3, 1, 2]);
        assert_eq!(meshes[1].prop("m"), FLOOR.to_le_bytes());
        assert!(meshes[1].props.iter().all(|p| p.0 != "wb"));
    }
}

#[tokio::test]
async fn lods_all_skips_the_empty_mesh_of_a_lower_lod() {
    let env = env_with(16).await;
    let (result, _) = convert(
        &engine(&env),
        json!({ "type": "cast", "lods": "all" }),
        "out/crate",
    )
    .await;
    let OutputResult::Ok { files, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        files,
        [
            "out/crate/weapon_0_LOD0.cast",
            "out/crate/weapon_0_LOD1.cast"
        ]
    );
    let lod1 = cast_of(&env, &files[1]).await;
    let meshes = lod1.children[0].kids("mesh");
    assert_eq!(meshes.len(), 1);
    assert_eq!(
        floats(meshes[0].prop("vp")),
        [0.0, 1.0, 2.0, 1.0, 1.0, 2.0, 2.0, 1.0, 2.0]
    );
    // only the material LOD 1 uses is written
    assert_eq!(lod1.children[0].kids("matl").len(), 1);

    let (result, _) = convert(
        &engine(&env),
        json!({ "type": "cast", "lods": [1] }),
        "out/one",
    )
    .await;
    let OutputResult::Ok { files, .. } = result else {
        panic!("{result:?}")
    };
    assert_eq!(files, ["out/one/weapon_0_LOD1.cast"]);
}

#[tokio::test]
async fn a_skin_swaps_the_materials() {
    let env = env_with(16).await;
    let (result, _) = convert(
        &engine(&env),
        json!({ "type": "cast", "skin": 1 }),
        "out/crate",
    )
    .await;
    let OutputResult::Ok { files, .. } = result else {
        panic!("{result:?}")
    };
    let root = cast_of(&env, &files[0]).await;
    let meshes = root.children[0].kids("mesh");
    assert_eq!(meshes[0].prop("m"), FLOOR.to_le_bytes());
    assert_eq!(meshes[1].prop("m"), WALL.to_le_bytes());
}

#[tokio::test]
async fn options_that_the_model_cannot_satisfy_are_invalid() {
    let env = env_with(16).await;
    for to in [
        json!({ "type": "cast", "skin": 2 }),
        json!({ "type": "cast", "lods": 2 }),
        json!({ "type": "cast", "lods": [0, 5] }),
    ] {
        let (result, _) = convert(&engine(&env), to.clone(), "out/crate").await;
        let OutputResult::Err { error } = result else {
            panic!("{to}: {result:?}")
        };
        assert!(
            matches!(error.kind(), ErrorKind::InvalidOptions { .. }),
            "{to}: {error}"
        );
    }
}

#[tokio::test]
async fn the_metadata_target_describes_the_whole_model() {
    for major in [16, 20] {
        let env = env_with(major).await;
        let (result, _) = convert(&engine(&env), json!({ "type": "rmdl.json" }), "out/crate").await;
        let OutputResult::Ok { files, .. } = result else {
            panic!("{result:?}")
        };
        assert_eq!(files, ["out/crate"], "v{major}");
        let doc: Value =
            serde_json::from_slice(&env.output.read(&files[0]).await.unwrap().to_vec()).unwrap();
        assert_eq!(doc["name"], "mdl/props/crate.rmdl");
        assert_eq!(doc["guid"], "0x0000000000000100");
        assert_eq!(doc["version"], major.to_string());
        assert_eq!(doc["studioName"], "model");
        assert_eq!(doc["bones"][1]["name"], "arm");
        assert_eq!(doc["bones"][1]["parent"], 0);
        assert_eq!(doc["bodyparts"][0]["name"], "weapon");
        assert_eq!(
            doc["bodyparts"][0]["models"][0]["meshes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(doc["lods"], json!([10.0, 50.0]));
        assert_eq!(
            doc["materials"],
            json!(["0x000000000000AAAA", "0x000000000000BBBB"])
        );
        assert_eq!(
            doc["skins"],
            json!([
                ["0x000000000000AAAA", "0x000000000000BBBB"],
                ["0x000000000000BBBB", "0x000000000000AAAA"]
            ])
        );
        assert_eq!(doc["vertexGroups"].as_array().unwrap().len(), 1);
        assert_eq!(doc["animRigs"], json!([]));
    }
}

#[tokio::test]
async fn unsupported_versions_are_reported_as_such() {
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let group = model_vg();
    let mut pak = PakBuilder::new();
    pak.rmdl(
        0x100,
        "mdl/old.rmdl",
        12,
        &studio(16, false, group.len() as u32),
        &group,
    );
    env.input
        .write("paks/lvl.rpak", pak.build(8))
        .await
        .unwrap();
    let (result, _) = convert(&engine(&env), json!({ "type": "cast" }), "out/old").await;
    let OutputResult::Err { error } = result else {
        panic!("{result:?}")
    };
    assert!(
        matches!(error.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn a_model_without_vertex_data_is_a_missing_dependency_for_meshes_only() {
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let group = model_vg();
    let mut pak = PakBuilder::new();
    pak.matl(WALL, "material/wall", 0, &[]);
    pak.matl(FLOOR, "material/floor", 0, &[]);
    // The studio block has a group, but the header's inline VG pointer is null and there is no starpak.
    pak.rmdl_without_vertex_data(
        0x100,
        "mdl/props/crate.rmdl",
        16,
        &studio(16, false, group.len() as u32),
    );
    env.input
        .write("paks/lvl.rpak", pak.build(8))
        .await
        .unwrap();
    let (result, _) = convert(&engine(&env), json!({ "type": "cast" }), "out/crate").await;
    let OutputResult::Err { error } = result else {
        panic!("{result:?}")
    };
    assert!(
        matches!(error.kind(), ErrorKind::MissingDependency { .. }),
        "{error}"
    );
    // the metadata does not need the vertex data
    let (result, _) = convert(&engine(&env), json!({ "type": "rmdl.json" }), "out/meta").await;
    assert!(matches!(result, OutputResult::Ok { .. }), "{result:?}");
}
