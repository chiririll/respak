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
