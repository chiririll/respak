//! `run` in-process, on `services::Memory` operators.

use std::process::ExitCode;

use clap::Parser;
use opendal::{services::Memory, Operator};
use respak_cli::{run, Cli};
use respak_core::{CancellationToken, Engine, ErrorKind};
use respak_formats::FormatsPlugin;

struct Env {
    engine: Engine,
    input: Operator,
    output: Operator,
}

fn env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(FormatsPlugin)
        .build()
        .unwrap();
    Env {
        engine,
        input,
        output,
    }
}

fn png_1x1() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 1, 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&[1, 2, 3, 4])
        .unwrap();
    out
}

struct Run {
    code: Option<ExitCode>,
    error: Option<anyhow::Error>,
    out: String,
    err: String,
}

async fn invoke(env: &Env, args: &[&str]) -> Run {
    invoke_with(env, &CancellationToken::new(), args).await
}

async fn invoke_with(env: &Env, cancel: &CancellationToken, args: &[&str]) -> Run {
    let cli = Cli::try_parse_from(
        ["respak", "--root", "in", "--out", "out"]
            .iter()
            .chain(args),
    )
    .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = run(cli, &env.engine, cancel, &mut out, &mut err).await;
    let (code, error) = match result {
        Ok(code) => (Some(code), None),
        Err(e) => (None, Some(e)),
    };
    Run {
        code,
        error,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
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
    assert_eq!(
        &env.output.read("icon.dds").await.unwrap().to_vec()[..4],
        b"DDS "
    );
}

#[tokio::test]
async fn a_failing_convert_exits_non_zero_and_reports_on_stderr() {
    let env = env();
    let run = invoke(
        &env,
        &["convert", "missing.png", "--to", r#"{"type":"dds"}"#],
    )
    .await;

    assert_eq!(run.code, Some(ExitCode::FAILURE));
    assert!(run.err.contains("missing.png"), "{}", run.err);
}

#[tokio::test]
async fn path_overrides_the_output_path() {
    let env = env();
    env.input.write("icon.png", png_1x1()).await.unwrap();
    let run = invoke(
        &env,
        &[
            "convert",
            "icon.png",
            "--to",
            r#"{"type":"dds"}"#,
            "--path",
            "x/y.bin",
        ],
    )
    .await;

    assert_eq!(run.code, Some(ExitCode::SUCCESS));
    assert!(env.output.exists("x/y.bin").await.unwrap());
    assert!(!env.output.exists("icon.dds").await.unwrap());
}

#[tokio::test]
async fn malformed_target_json_is_an_invalid_request() {
    let env = env();
    let run = invoke(&env, &["convert", "icon.png", "--to", "{not json"]).await;

    let error = run.error.expect("must fail");
    let error = error
        .downcast_ref::<respak_core::Error>()
        .expect("a respak error");
    assert!(
        matches!(error.kind(), ErrorKind::InvalidRequest { .. }),
        "{error}"
    );
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
    let names: Vec<_> = json["formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["png", "dds", "png.frames", "cast", "wav"]);
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
    let ok = invoke(
        &env,
        &[
            "convert",
            "a.png",
            "--to",
            r#"{"type":"dds","via":["png"]}"#,
        ],
    )
    .await;
    assert_eq!(ok.code, Some(ExitCode::SUCCESS));
    // ... and a format that is not on the path is not.
    let bad = invoke(
        &env,
        &[
            "convert",
            "a.png",
            "--to",
            r#"{"type":"dds","via":["nope"]}"#,
        ],
    )
    .await;
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
    Env {
        engine,
        input,
        output,
    }
}

#[tokio::test]
async fn an_rpak_texture_converts_to_dds_inside_the_container_path() {
    use respak_respawn_testing::{build_tex_pak, TexSpec};
    // 8x8 BC1 with 2 permanent mips: 32 bytes + 8 bytes (padded to 16 each), smallest first.
    let mut data = vec![2u8; 16];
    data.extend(vec![1u8; 32]);
    let mut t = TexSpec::new(0x42, 9, 8, 8, 0, 2, data);
    t.name = Some("ui/thing");
    let env = rpak_env();
    env.input
        .write("paks/common.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();

    let run = invoke(
        &env,
        &[
            "convert",
            "paks/common.rpak/0x0000000000000042.txtr",
            "--to",
            r#"{"type":"dds"}"#,
        ],
    )
    .await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let report: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(
        report["outputs"][0]["result"]["Ok"]["files"][0],
        "0x0000000000000042.dds"
    );

    let bytes = env
        .output
        .read("0x0000000000000042.dds")
        .await
        .unwrap()
        .to_vec();
    let dds = ddsfile::Dds::read(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(
        dds.header10.as_ref().map(|h| h.dxgi_format),
        Some(ddsfile::DxgiFormat::BC1_UNorm)
    );
    assert_eq!(
        (
            dds.get_width(),
            dds.get_height(),
            dds.get_num_mipmap_levels()
        ),
        (8, 8, 2)
    );
    let mut expected = vec![1u8; 32];
    expected.extend(vec![2u8; 8]);
    assert_eq!(dds.data, expected);
}

#[tokio::test]
async fn discover_in_an_rpak_lists_textures_as_convertible() {
    use respak_respawn_testing::{build_tex_pak, TexSpec};
    let env = rpak_env();
    let t = TexSpec::new(0x42, 10, 4, 4, 31, 1, vec![9u8; 64]);
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let run = invoke(&env, &["discover", "c.rpak", "--depth", "1"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let json: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let e = &json[0]["entries"][0];
    assert_eq!(
        (e["format"].as_str(), e["version"].as_str()),
        (Some("txtr"), Some("10"))
    );
    let targets: Vec<_> = e["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["type"].as_str().unwrap())
        .collect();
    assert_eq!(targets, ["png", "dds"]);
}

#[tokio::test]
async fn discover_names_reads_the_asset_names() {
    use respak_respawn_testing::{build_tex_pak, TexSpec};
    let env = rpak_env();
    let mut t = TexSpec::new(0x42, 10, 4, 4, 31, 1, vec![9u8; 64]);
    t.name = Some("texture/ui/icon");
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let name_of = |out: &str| {
        let json: serde_json::Value = serde_json::from_str(out).unwrap();
        json[0]["entries"][0]["name"].as_str().map(str::to_owned)
    };
    let run = invoke(&env, &["discover", "c.rpak", "--depth", "1"]).await;
    assert_eq!(name_of(&run.out), None);
    let run = invoke(&env, &["discover", "c.rpak", "--depth", "1", "--names"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    assert_eq!(name_of(&run.out).as_deref(), Some("texture/ui/icon"));
}

#[tokio::test]
async fn an_rpak_bc1_texture_converts_to_png_and_honours_mips() {
    use respak_respawn_testing::{build_tex_pak, TexSpec};
    // 8x8 BC1, 2 permanent mips (smallest first in the file): mip 0 is solid colour 0xF800 (red).
    let block = |c0: u16| {
        let mut b = c0.to_le_bytes().to_vec();
        b.extend([0, 0, 0, 0, 0, 0]);
        b
    };
    let mut data: Vec<u8> = block(0x001F).repeat(1); // mip 1: 1 block (blue), padded to 16
    data.resize(16, 0);
    data.extend(block(0xF800).repeat(4)); // mip 0: 4 blocks (red)
    let t = TexSpec::new(0x42, 9, 8, 8, 0, 2, data);
    let env = rpak_env();
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();

    let png_of = |bytes: Vec<u8>| {
        let mut r = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        (info.width, info.height, buf[..4].to_vec())
    };

    let run = invoke(
        &env,
        &[
            "convert",
            "c.rpak/0x0000000000000042.txtr",
            "--to",
            r#"{"type":"png"}"#,
        ],
    )
    .await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let (w, h, px) = png_of(
        env.output
            .read("0x0000000000000042.png")
            .await
            .unwrap()
            .to_vec(),
    );
    assert_eq!((w, h), (8, 8));
    assert_eq!(px, [255, 0, 0, 255]);

    let run = invoke(
        &env,
        &[
            "convert",
            "c.rpak/0x0000000000000042.txtr",
            "--to",
            r#"{"type":"png","mips":1}"#,
        ],
    )
    .await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    let (w, h, px) = png_of(
        env.output
            .read("0x0000000000000042.png")
            .await
            .unwrap()
            .to_vec(),
    );
    assert_eq!((w, h), (4, 4));
    assert_eq!(px, [0, 0, 255, 255]);
}

#[tokio::test]
async fn run_reads_a_request_file_with_a_glob() {
    let env = env();
    env.input.write("a/1.png", png_1x1()).await.unwrap();
    env.input.write("a/2.png", png_1x1()).await.unwrap();
    let dir = std::env::temp_dir().join(format!("respak-run-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("request.json");
    std::fs::write(&file, r#"{"version":1,"outputs":[{"from":"a/*.png","to":{"type":"dds"}},{"from":"zzz/*.png","to":{"type":"dds"}}]}"#)
        .unwrap();

    let run = invoke(&env, &["run", file.to_str().unwrap()]).await;
    let _ = std::fs::remove_dir_all(&dir);
    // one output matched nothing, so the exit code is a failure, but the other ran
    assert_eq!(run.code, Some(ExitCode::FAILURE));
    assert!(run.err.contains("zzz/*.png"), "{}", run.err);
    let report: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    assert_eq!(report["outputs"].as_array().unwrap().len(), 3);
    assert!(
        env.output.exists("a/1.dds").await.unwrap() && env.output.exists("a/2.dds").await.unwrap()
    );

    let bad = invoke(&env, &["run", "/definitely/not/here.json"]).await;
    assert!(bad.error.is_some());
}

#[tokio::test]
async fn a_triggered_token_cancels_every_output_and_exits_non_zero() {
    let env = env();
    env.input.write("a.png", png_1x1()).await.unwrap();
    env.input.write("b.png", png_1x1()).await.unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let run = invoke_with(
        &env,
        &cancel,
        &["convert", "*.png", "--to", r#"{"type":"dds"}"#],
    )
    .await;

    assert_eq!(run.code, Some(ExitCode::FAILURE));
    let report: serde_json::Value = serde_json::from_str(&run.out).unwrap();
    let outputs = report["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 2);
    assert!(
        outputs
            .iter()
            .all(|o| o["result"]["Err"]["error"]["kind"] == "Cancelled"),
        "{}",
        run.out
    );
    assert!(!env.output.exists("a.dds").await.unwrap());
}

#[tokio::test]
async fn events_are_printed_to_their_own_stream() {
    let env = env();
    env.input.write("a.png", png_1x1()).await.unwrap();
    env.input.write("b.png", png_1x1()).await.unwrap();
    let cli = Cli::try_parse_from([
        "respak",
        "--root",
        "in",
        "--out",
        "out",
        "convert",
        "*.png",
        "--to",
        r#"{"type":"dds"}"#,
    ])
    .unwrap();
    #[derive(Clone, Default)]
    struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (mut out, mut err, events) = (Vec::new(), Vec::new(), Shared::default());
    let code = respak_cli::run_with_events(
        cli,
        &env.engine,
        &CancellationToken::new(),
        &mut out,
        &mut err,
        events.clone(),
    )
    .await
    .unwrap();

    assert_eq!(code, ExitCode::SUCCESS);
    assert!(err.is_empty());
    let lines = String::from_utf8(events.0.lock().unwrap().clone()).unwrap();
    for expected in ["wrote a.dds", "wrote b.dds", "[2/2] convert"] {
        assert!(
            lines.lines().any(|l| l == expected),
            "{expected} missing in:\n{lines}"
        );
    }
}

#[tokio::test]
async fn copy_copies_files_unchanged_and_combines_with_a_conversion() {
    let env = env();
    let bytes: Vec<u8> = (0..(5u32 << 20)).map(|i| i as u8).collect();
    env.input.write("media/a.bin", bytes.clone()).await.unwrap();
    env.input.write("media/b.bin", vec![7]).await.unwrap();
    env.input.write("icon.png", png_1x1()).await.unwrap();

    let run = invoke(&env, &["copy", "media/*.bin"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    assert_eq!(
        env.output.read("media/a.bin").await.unwrap().to_vec(),
        bytes
    );
    assert_eq!(env.output.read("media/b.bin").await.unwrap().to_vec(), [7]);

    let run = invoke(&env, &["copy", "icon.png", "--path", "copy/icon.png"]).await;
    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    assert_eq!(
        env.output.read("copy/icon.png").await.unwrap().to_vec(),
        png_1x1()
    );
}

#[tokio::test]
async fn convert_with_copy_writes_both_the_conversion_and_the_original() {
    let env = env();
    env.input.write("img/icon.png", png_1x1()).await.unwrap();
    let run = invoke(
        &env,
        &[
            "convert",
            "img/*.png",
            "--to",
            r#"{"type":"dds"}"#,
            "--copy",
        ],
    )
    .await;

    assert_eq!(run.code, Some(ExitCode::SUCCESS), "{}", run.err);
    assert_eq!(
        &env.output.read("img/icon.dds").await.unwrap().to_vec()[..4],
        b"DDS "
    );
    assert_eq!(
        env.output.read("img/icon.png").await.unwrap().to_vec(),
        png_1x1()
    );
}
