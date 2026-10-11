//! `run` in-process, on `services::Memory` operators.

use std::process::ExitCode;

use clap::Parser;
use opendal::{services::Memory, Operator};
use respak_cli::{run, Cli};
use respak_core::{Engine, ErrorKind};
use respak_formats::FormatsPlugin;

struct Env {
    engine: Engine,
    input: Operator,
    output: Operator,
}

fn env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine =
        Engine::builder().input(input.clone()).output(output.clone()).plugin(FormatsPlugin).build().unwrap();
    Env { engine, input, output }
}

fn png_1x1() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 1, 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&[1, 2, 3, 4]).unwrap();
    out
}

struct Run {
    code: Option<ExitCode>,
    error: Option<anyhow::Error>,
    out: String,
    err: String,
}

async fn invoke(env: &Env, args: &[&str]) -> Run {
    let cli = Cli::try_parse_from(["respak", "--root", "in", "--out", "out"].iter().chain(args)).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = run(cli, &env.engine, &mut out, &mut err).await;
    let (code, error) = match result {
        Ok(code) => (Some(code), None),
        Err(e) => (None, Some(e)),
    };
    Run { code, error, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}

#[tokio::test]
async fn convert_writes_the_output_and_prints_the_report() {
    let env = env();
    env.input.write("icon.png", png_1x1()).await.unwrap();
    let run = invoke(&env, &["convert", "icon.png", "--to", r#"{"type":"dds"}"#]).await;

    assert_eq!(run.code, Some(ExitCode::SUCCESS));
    assert!(run.err.is_empty(), "{}", run.err);
    let report: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(report["outputs"][0]["result"]["Ok"]["files"][0], "icon.dds");
    assert_eq!(&env.output.read("icon.dds").await.unwrap().to_vec()[..4], b"DDS ");
}

#[tokio::test]
async fn a_failing_convert_exits_non_zero_and_reports_on_stderr() {
    let env = env();
    let run = invoke(&env, &["convert", "missing.png", "--to", r#"{"type":"dds"}"#]).await;

    assert_eq!(run.code, Some(ExitCode::FAILURE));
    assert!(run.err.contains("missing.png"), "{}", run.err);
}

#[tokio::test]
async fn path_overrides_the_output_path() {
    let env = env();
    env.input.write("icon.png", png_1x1()).await.unwrap();
    let run = invoke(&env, &["convert", "icon.png", "--to", r#"{"type":"dds"}"#, "--path", "x/y.bin"]).await;

    assert_eq!(run.code, Some(ExitCode::SUCCESS));
    assert!(env.output.exists("x/y.bin").await.unwrap());
    assert!(!env.output.exists("icon.dds").await.unwrap());
}

#[tokio::test]
async fn malformed_target_json_is_an_invalid_request() {
    let env = env();
    let run = invoke(&env, &["convert", "icon.png", "--to", "{not json"]).await;

    let error = run.error.expect("must fail");
    let error = error.downcast_ref::<respak_core::Error>().expect("a respak error");
    assert!(matches!(error.kind(), ErrorKind::InvalidRequest { .. }), "{error}");
}

fn dds_1x1() -> Vec<u8> {
    let mut d = ddsfile::Dds::new_d3d(ddsfile::NewD3dParams {
        height: 1,
        width: 1,
        depth: None,
        format: ddsfile::D3DFormat::A8B8G8R8,
        mipmap_levels: None,
        caps2: None,
    })
    .unwrap();
    d.data = vec![1, 2, 3, 4];
    let mut out = Vec::new();
    d.write(&mut out).unwrap();
    out
}

#[tokio::test]
async fn formats_prints_the_graph() {
    let env = env();
    let run = invoke(&env, &["formats"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS));
    let json: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let names: Vec<_> = json["formats"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["png", "dds"]);
}

#[tokio::test]
async fn discover_prints_one_entry_per_path() {
    let env = env();
    env.input.write("a.png", png_1x1()).await.unwrap();
    env.input.write("b.dds", dds_1x1()).await.unwrap();
    let run = invoke(&env, &["discover", "a.png", "b.dds"]).await;

    assert_eq!(run.code, Some(ExitCode::SUCCESS));
    let json: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(json[0]["path"], "a.png");
    assert_eq!(json[0]["entries"][0]["format"], "png");
    assert_eq!(json[0]["entries"][0]["targets"][0]["type"], "dds");
    assert_eq!(json[1]["entries"][0]["format"], "dds");
}

#[tokio::test]
async fn discover_of_a_missing_path_fails_and_still_prints_the_others() {
    let env = env();
    env.input.write("a.png", png_1x1()).await.unwrap();
    let run = invoke(&env, &["discover", "a.png", "gone.png"]).await;

    assert_eq!(run.code, Some(ExitCode::FAILURE));
    assert!(run.err.contains("gone.png"), "{}", run.err);
    let json: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn convert_accepts_via() {
    let env = env();
    env.input.write("a.png", png_1x1()).await.unwrap();

    // png -> dds is a single edge, so via [png] (the start counts) is fine ...
    let ok = invoke(&env, &["convert", "a.png", "--to", r#"{"type":"dds","via":["png"]}"#]).await;
    assert_eq!(ok.code, Some(ExitCode::SUCCESS));
    // ... and a format that is not on the path is not.
    let bad = invoke(&env, &["convert", "a.png", "--to", r#"{"type":"dds","via":["nope"]}"#]).await;
    assert_eq!(bad.code, Some(ExitCode::FAILURE));
    assert!(bad.err.contains("no conversion path"), "{}", bad.err);
}

fn rpak_env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(FormatsPlugin)
        .plugin(respak_respawn::RespawnPlugin::new())
        .build()
        .unwrap();
    Env { engine, input, output }
}

#[tokio::test]
async fn an_rpak_texture_converts_to_dds_inside_the_container_path() {
    use respak_respawn::testing::{build_tex_pak, TexSpec};
    // 8x8 BC1 with 2 permanent mips: 32 bytes + 8 bytes (padded to 16 each), smallest first.
    let mut data = vec![2u8; 16];
    data.extend(vec![1u8; 32]);
    let mut t = TexSpec::new(0x42, 9, 8, 8, 0, 2, data);
    t.name = Some("ui/thing");
    let env = rpak_env();
    env.input.write("paks/common.rpak", build_tex_pak(8, &[t])).await.unwrap();

    let run = invoke(&env, &["convert", "paks/common.rpak/0x0000000000000042.txtr", "--to", r#"{"type":"dds"}"#]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let report: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(report["outputs"][0]["result"]["Ok"]["files"][0], "0x0000000000000042.dds");

    let bytes = env.output.read("0x0000000000000042.dds").await.unwrap().to_vec();
    let dds = ddsfile::Dds::read(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(dds.header10.as_ref().map(|h| h.dxgi_format), Some(ddsfile::DxgiFormat::BC1_UNorm));
    assert_eq!((dds.get_width(), dds.get_height(), dds.get_num_mipmap_levels()), (8, 8, 2));
    let mut expected = vec![1u8; 32];
    expected.extend(vec![2u8; 8]);
    assert_eq!(dds.data, expected);
}

#[tokio::test]
async fn discover_in_an_rpak_lists_textures_as_convertible() {
    use respak_respawn::testing::{build_tex_pak, TexSpec};
    let env = rpak_env();
    let t = TexSpec::new(0x42, 10, 4, 4, 31, 1, vec![9u8; 64]);
    env.input.write("c.rpak", build_tex_pak(8, &[t])).await.unwrap();
    let run = invoke(&env, &["discover", "c.rpak", "--depth", "1"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let json: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let e = &json[0]["entries"][0];
    assert_eq!((e["format"].as_str(), e["version"].as_str()), (Some("txtr"), Some("10")));
    let targets: Vec<_> = e["targets"].as_array().unwrap().iter().map(|t| t["type"].as_str().unwrap()).collect();
    assert_eq!(targets, ["png", "dds"]);
}
