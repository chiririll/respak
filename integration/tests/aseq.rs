//! An animation: `aseq -> cast` over synthetic paks (slice 19), in-process and through the CLI.

use std::process::ExitCode;

use clap::Parser;
use opendal::{services::Memory, Operator};
use respak_cli::{run, Cli};
use respak_core::{CancellationToken, ConvertRequest, Engine, ErrorKind, OutputResult};
use respak_formats::FormatsPlugin;
use respak_respawn::testing::aseq::{
    bone, quat64, raw_track, section, value_ptr, vec48, Anim, Seq, Weights,
};
use respak_respawn::testing::{rmdl, PakBuilder};
use respak_respawn::RespawnPlugin;
use serde_json::{json, Value};

const SEQ: u64 = 0x10;
const DATA_A: u64 = 0x20;
const DATA_B: u64 = 0x21;
const RIG: u64 = 0x50;
const ASEQ: &str = "paks/anim.rpak/0x0000000000000010.aseq";
const ROT: f32 = 0.000_191_753_45;

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

/// Bone 0 with a tracked position (X, scale 0.5) and rotation (pitch), bone 1 with a constant rotation.
fn walk_section() -> Vec<u8> {
    let mut data = 0.5f32.to_le_bytes().to_vec();
    data.extend(value_ptr(8, 0b100, 0, 0));
    data.extend(value_ptr(14, 0b010, 0, 0));
    data.extend(raw_track(&[10, 20, 30, 40]));
    data.extend(raw_track(&[1000, 2000, 3000, 4000]));
    section(
        true,
        &[(3, bone(0x6, &data)), (2, bone(0, &quat64([0.5; 4])))],
    )
}

/// A constant position of bone 0 only.
fn still_section(position: [f32; 3]) -> Vec<u8> {
    section(true, &[(1, bone(0, &vec48(position))), (0, vec![])])
}

fn studio() -> Vec<u8> {
    rmdl::studio(18, false, 0)
}

/// `paks/anim.rpak` with the rig, two `asqd` and a sequence of two blends.
async fn env_with(seq: Seq, blobs: &[(u64, Vec<u8>)]) -> Env {
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let mut pak = PakBuilder::new();
    pak.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    for (guid, blob) in blobs {
        pak.asqd(*guid, blob);
    }
    pak.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &respak_respawn::testing::aseq::seq_block(&seq),
        &[],
        None,
    );
    env.input
        .write("paks/anim.rpak", pak.build(8))
        .await
        .unwrap();
    env
}

fn walk() -> Seq {
    let mut looping = Anim::new("walk_b", 2, DATA_B);
    looping.flags |= 0x1;
    Seq::new("walk", vec![Anim::new("walk_a", 4, DATA_A), looping])
}

async fn walk_env() -> Env {
    env_with(
        walk(),
        &[
            (DATA_A, walk_section()),
            (DATA_B, still_section([7.0, 8.0, 9.0])),
        ],
    )
    .await
}

fn request(to: Value, path: &str) -> ConvertRequest {
    serde_json::from_value(
        json!({ "version": 1, "outputs": [{ "from": ASEQ, "to": to, "path": path }] }),
    )
    .unwrap()
}

async fn convert(engine: &Engine, to: Value, path: &str) -> OutputResult {
    let report = engine
        .convert(&request(to, path), &CancellationToken::new())
        .await
        .unwrap();
    report.outputs.into_iter().next().unwrap().result
}

async fn files(engine: &Engine, to: Value) -> Vec<String> {
    match convert(engine, to, "out/walk").await {
        OutputResult::Ok { files, .. } => files,
        other => panic!("{other:?}"),
    }
}

// ---- a small Cast reader ----------------------------------------------------------------------

#[derive(Debug)]
struct Node {
    id: String,
    props: Vec<(String, String, Vec<u8>)>,
    children: Vec<Node>,
}

impl Node {
    fn prop(&self, name: &str) -> &[u8] {
        &self
            .props
            .iter()
            .find(|p| p.0 == name)
            .unwrap_or_else(|| panic!("no property {name} in {}", self.id))
            .2
    }
    fn has(&self, name: &str) -> bool {
        self.props.iter().any(|p| p.0 == name)
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
        let ty = String::from_utf8_lossy(&b[p..p + 2])
            .trim_end_matches('\0')
            .to_owned();
        let name_len = u16::from_le_bytes([b[p + 2], b[p + 3]]) as usize;
        let arr = u32_at(b, p + 4) as usize;
        let name = String::from_utf8(b[p + 8..p + 8 + name_len].to_vec()).unwrap();
        let start = p + 8 + name_len;
        let len = match ty.as_str() {
            "s" => b[start..].iter().position(|&c| c == 0).unwrap() + 1,
            "b" => arr,
            "h" => 2 * arr,
            "i" | "f" => 4 * arr,
            "l" | "d" | "v2" => 8 * arr,
            "v3" => 12 * arr,
            "v4" => 16 * arr,
            other => panic!("type {other:?}"),
        };
        props.push((name, ty, b[start..start + len].to_vec()));
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

fn near(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len(), "{a:?} != {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-4, "{a:?} != {b:?}");
    }
}

/// The curve for `bone` and key property `key`.
fn curve<'a>(anim: &'a Node, bone: &str, key: &str) -> &'a Node {
    anim.kids("curv")
        .into_iter()
        .find(|c| {
            c.prop("nn") == format!("{bone}\0").as_bytes()
                && c.prop("kp") == format!("{key}\0").as_bytes()
        })
        .unwrap_or_else(|| panic!("no curve {bone}.{key}"))
}

fn values(anim: &Node, bone: &str, key: &str) -> Vec<f32> {
    floats(curve(anim, bone, key).prop("kv"))
}

/// The Hamilton product, for the expected rotations.
fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The yaw fix-up quaternion (-90 degrees about Z).
fn yaw_minus_90() -> [f32; 4] {
    let (s, c) = (-std::f32::consts::FRAC_PI_4).sin_cos();
    [0.0, 0.0, s, c]
}

fn pitch_quats() -> Vec<[f32; 4]> {
    [1000.0f32, 2000.0, 3000.0, 4000.0]
        .iter()
        .map(|v| {
            let half = v * ROT * 0.5;
            [0.0, half.sin(), 0.0, half.cos()]
        })
        .collect()
}

// ---- tests --------------------------------------------------------------------------------------

#[tokio::test]
async fn an_aseq_converts_to_one_cast_per_blend() {
    let env = walk_env().await;
    let written = files(&engine(&env), json!({ "type": "cast" })).await;
    assert_eq!(written, ["out/walk/walk_0.cast", "out/walk/walk_1.cast"]);

    let root = cast_of(&env, &written[0]).await;
    assert_eq!(root.children.len(), 1);
    let anim = &root.children[0];
    assert_eq!(anim.id, "anim");
    assert_eq!(floats(anim.prop("fr")), [30.0]);
    assert_eq!(anim.prop("lo"), [0]);
    let skel = anim.kids("skel")[0];
    let bones = skel.kids("bone");
    assert_eq!(bones.len(), 2);
    assert_eq!(bones[0].prop("n"), b"root\0");
    assert_eq!(bones[1].prop("n"), b"arm\0");
    assert_eq!(bones[1].prop("p"), 0u32.to_le_bytes());
    assert_eq!(
        anim.kids("curv").len(),
        8,
        "tx, ty, tz and rq for two bones; no scale"
    );

    // bone 0 with the root fix-up: position (x, y, z) -> (y, -x, z), the yaw turned by -90 degrees
    near(&values(anim, "root", "tx"), &[2.0; 4]);
    near(&values(anim, "root", "ty"), &[-6.0, -11.0, -16.0, -21.0]);
    near(&values(anim, "root", "tz"), &[3.0; 4]);
    let rq = floats(curve(anim, "root", "rq").prop("kv"));
    for (f, q) in pitch_quats().iter().enumerate() {
        near(&rq[4 * f..4 * f + 4], &mul(yaw_minus_90(), *q));
    }
    assert_eq!(
        curve(anim, "root", "rq")
            .props
            .iter()
            .find(|p| p.0 == "kv")
            .unwrap()
            .1,
        "v4"
    );
    assert_eq!(curve(anim, "root", "tx").prop("kb"), [0, 1, 2, 3]);
    assert_eq!(curve(anim, "root", "tx").prop("m"), b"absolute\0");
    assert!(!curve(anim, "root", "tx").has("ab"));

    // bone 1 has constant channels, written once per frame
    near(&values(anim, "arm", "tx"), &[2.0; 4]);
    near(&values(anim, "arm", "tz"), &[3.0; 4]);
    near(&values(anim, "arm", "rq"), &[0.5; 16]);

    // the second blend: looping, a constant position
    let second = cast_of(&env, &written[1]).await;
    let anim = &second.children[0];
    assert_eq!(anim.prop("lo"), [1]);
    near(&values(anim, "root", "tx"), &[8.0, 8.0]);
    near(&values(anim, "root", "ty"), &[-7.0, -7.0]);
    near(&values(anim, "root", "tz"), &[9.0, 9.0]);
}

#[tokio::test]
async fn the_root_fixup_option_keeps_bone_0_as_stored() {
    let env = walk_env().await;
    let written = files(
        &engine(&env),
        json!({ "type": "cast", "root_fixup": "none" }),
    )
    .await;
    let anim = &cast_of(&env, &written[0]).await.children[0];
    near(&values(anim, "root", "tx"), &[6.0, 11.0, 16.0, 21.0]);
    near(&values(anim, "root", "ty"), &[2.0; 4]);
    near(&values(anim, "root", "tz"), &[3.0; 4]);
    let rq = floats(curve(anim, "root", "rq").prop("kv"));
    for (f, q) in pitch_quats().iter().enumerate() {
        near(&rq[4 * f..4 * f + 4], q);
    }
    // other bones are never touched
    near(&values(anim, "arm", "rq"), &[0.5; 16]);
}

#[tokio::test]
async fn blend_picks_one_file_and_a_missing_blend_is_invalid() {
    let env = walk_env().await;
    let written = files(&engine(&env), json!({ "type": "cast", "blend": 1 })).await;
    assert_eq!(written, ["out/walk/walk_1.cast"]);
    let OutputResult::Err { error } = convert(
        &engine(&env),
        json!({ "type": "cast", "blend": 2 }),
        "out/x",
    )
    .await
    else {
        panic!("blend 2 does not exist");
    };
    assert!(
        matches!(error.kind(), ErrorKind::InvalidOptions { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn additive_animations_write_their_bone_weights() {
    let mut anim = Anim::new("delta", 2, DATA_A);
    anim.flags |= 0x4;
    let mut seq = Seq::new("delta", vec![anim]);
    seq.weights = Weights::List(vec![0.25, 0.75]);
    let mut data = 1.0f32.to_le_bytes().to_vec();
    data.extend(value_ptr(4, 0b100, 0, 0));
    data.extend(raw_track(&[10, 20]));
    let blob = section(true, &[(1, bone(0x4, &data)), (0, vec![])]);
    let env = env_with(seq, &[(DATA_A, blob)]).await;
    let written = files(
        &engine(&env),
        json!({ "type": "cast", "root_fixup": "none" }),
    )
    .await;
    let anim = &cast_of(&env, &written[0]).await.children[0];
    for key in ["tx", "ty", "tz", "rq"] {
        assert_eq!(curve(anim, "root", key).prop("m"), b"additive\0");
        assert_eq!(floats(curve(anim, "root", key).prop("ab")), [0.25]);
        assert_eq!(floats(curve(anim, "arm", key).prop("ab")), [0.75]);
    }
    near(&values(anim, "root", "tx"), &[10.0, 20.0]);
    near(&values(anim, "root", "ty"), &[0.0, 0.0]);
    near(&values(anim, "arm", "tx"), &[0.0, 0.0]);
    near(
        &values(anim, "arm", "rq"),
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    );
}

#[tokio::test]
async fn scale_curves_are_written_only_for_sequences_with_scale() {
    let mut seq = Seq::new("scaled", vec![Anim::new("scaled", 2, DATA_A)]);
    seq.flags = 0x20000;
    let mut data = vec48([1.0, 2.0, 3.0]).to_vec();
    data.extend(quat64([0.0, 0.0, 0.0, 1.0]));
    data.extend(vec48([2.0, 2.0, 2.0]));
    let blob = section(true, &[(7, bone(0, &data)), (0, vec![])]);
    let env = env_with(seq, &[(DATA_A, blob)]).await;
    let written = files(
        &engine(&env),
        json!({ "type": "cast", "root_fixup": "none" }),
    )
    .await;
    let anim = &cast_of(&env, &written[0]).await.children[0];
    assert_eq!(anim.kids("curv").len(), 14);
    near(&values(anim, "root", "sx"), &[2.0, 2.0]);
    near(&values(anim, "arm", "sz"), &[1.0, 1.0]);
}

#[tokio::test]
async fn a_long_animation_keys_every_frame_with_a_wide_enough_buffer() {
    let frames = 300;
    let mut anim = Anim::new("long", frames, DATA_A);
    anim.flags = 0x20000;
    let values: Vec<i16> = (0..frames as i16).collect();
    // a track block holds up to 255 frames: two blocks, 255 + 45
    let mut track = raw_track(&values[..255]);
    track.extend(raw_track(&values[255..]));
    let mut data = 1.0f32.to_le_bytes().to_vec();
    data.extend(value_ptr(4, 0b100, 0, 0));
    data.extend(track);
    let blob = section(true, &[(1, bone(0x4, &data)), (0, vec![])]);
    let env = env_with(Seq::new("long", vec![anim]), &[(DATA_A, blob)]).await;
    let written = files(
        &engine(&env),
        json!({ "type": "cast", "root_fixup": "none" }),
    )
    .await;
    let anim = &cast_of(&env, &written[0]).await.children[0];
    let tx = curve(anim, "root", "tx");
    assert_eq!(tx.props.iter().find(|p| p.0 == "kb").unwrap().1, "h");
    assert_eq!(tx.prop("kb").len(), 600);
    let xs = floats(tx.prop("kv"));
    assert_eq!(
        (xs.len(), xs[0], xs[254], xs[255], xs[299]),
        (300, 1.0, 255.0, 256.0, 300.0),
        "the rest pose x is 1"
    );
}

#[tokio::test]
async fn errors_are_reported_per_output() {
    // no rig
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let mut pak = PakBuilder::new();
    pak.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &respak_respawn::testing::aseq::seq_block(&walk()),
        &[],
        None,
    );
    env.input
        .write("paks/anim.rpak", pak.build(8))
        .await
        .unwrap();
    let OutputResult::Err { error } =
        convert(&engine(&env), json!({ "type": "cast" }), "out/walk").await
    else {
        panic!("no rig");
    };
    assert!(
        matches!(error.kind(), ErrorKind::MissingDependency { .. }),
        "{error}"
    );

    // no asqd
    let env = env_with(walk(), &[]).await;
    let OutputResult::Err { error } =
        convert(&engine(&env), json!({ "type": "cast" }), "out/walk").await
    else {
        panic!("no asqd");
    };
    assert!(
        matches!(error.kind(), ErrorKind::MissingDependency { .. }),
        "{error}"
    );

    // a version below 12.1
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let mut pak = PakBuilder::new();
    pak.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    pak.aseq(
        SEQ,
        "anim/walk.rseq",
        11,
        &respak_respawn::testing::aseq::seq_block(&walk()),
        &[],
        None,
    );
    env.input
        .write("paks/anim.rpak", pak.build(8))
        .await
        .unwrap();
    let OutputResult::Err { error } =
        convert(&engine(&env), json!({ "type": "cast" }), "out/walk").await
    else {
        panic!("version 11");
    };
    assert!(
        matches!(error.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{error}"
    );
}

#[tokio::test]
async fn a_cancelled_convert_writes_nothing() {
    let env = walk_env().await;
    let token = CancellationToken::new();
    token.cancel();
    let report = engine(&env)
        .convert(&request(json!({ "type": "cast" }), "out/walk"), &token)
        .await
        .unwrap();
    let OutputResult::Err { error } = &report.outputs[0].result else {
        panic!("{:?}", report.outputs[0].result)
    };
    assert!(matches!(error.kind(), ErrorKind::Cancelled), "{error}");
    assert!(!env.output.exists("out/walk/walk_0.cast").await.unwrap());
}

#[tokio::test]
async fn the_cli_binds_a_sequence_to_an_explicit_rig() {
    let env = Env {
        input: mem(),
        output: mem(),
    };
    let path = "anim/rig_by_path";
    let guid = respak_respawn::string_to_guid(path);
    let mut pak = PakBuilder::new();
    // the rig lists no sequence: only `--rig` brings the two together
    pak.arig(guid, path, 5, &studio(), &[]);
    pak.asqd(DATA_A, &walk_section());
    pak.asqd(DATA_B, &still_section([7.0, 8.0, 9.0]));
    pak.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &respak_respawn::testing::aseq::seq_block(&walk()),
        &[],
        None,
    );
    env.input
        .write("paks/anim.rpak", pak.build(8))
        .await
        .unwrap();

    let invoke = |args: Vec<&'static str>, env: &Env, rig: Option<&'static str>| {
        let mut all = vec!["respak", "--root", "in", "--out", "out"];
        if let Some(rig) = rig {
            all.extend(["--rig", rig]);
        }
        all.extend(args);
        let cli = Cli::try_parse_from(all).unwrap();
        let plugin = RespawnPlugin::new()
            .rig(cli.rig.clone().unwrap_or_else(|| "auto".into()))
            .studio(cli.studio.clone().unwrap_or_else(|| "auto".into()));
        let engine = Engine::builder()
            .input(env.input.clone())
            .output(env.output.clone())
            .plugin(FormatsPlugin)
            .plugin(plugin)
            .build()
            .unwrap();
        async move {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = run(cli, &engine, &CancellationToken::new(), &mut out, &mut err)
                .await
                .unwrap();
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        }
    };
    let convert = vec!["convert", ASEQ, "--to", r#"{"type":"cast","blend":0}"#];

    let (code, out, _) = invoke(convert.clone(), &env, None).await;
    assert_eq!(code, ExitCode::FAILURE, "auto finds no rig: {out}");
    let (code, out, err) = invoke(convert, &env, Some("anim/rig_by_path")).await;
    assert_eq!(code, ExitCode::SUCCESS, "{err}");
    let report: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        report["outputs"][0]["result"]["Ok"]["files"][0],
        "0x0000000000000010/walk_0.cast"
    );
    let anim = &cast_of(&env, "0x0000000000000010/walk_0.cast")
        .await
        .children[0];
    assert_eq!(anim.kids("curv").len(), 8);
}
