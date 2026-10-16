//! PNG <-> DDS through the whole engine, on synthetic files in memory.

use ddsfile::{D3DFormat, Dds, NewD3dParams};
use opendal::{services::Memory, Operator};
use respak_core::{ConvertRequest, Engine, ErrorKind, OutputResult};
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

impl Env {
    /// Converts `from` to `to`; returns the written bytes or the error kind's name.
    async fn convert(&self, from: &str, to: &str) -> Result<Vec<u8>, ErrorKind> {
        let json = format!(
            r#"{{"version":1,"outputs":[{{"from":"{from}","to":{{"type":"{to}"}},"path":"out.{to}"}}]}}"#
        );
        let request: ConvertRequest = serde_json::from_str(&json).unwrap();
        let report = self
            .engine
            .convert(&request, &respak_core::CancellationToken::new())
            .await
            .unwrap();
        match report.outputs.into_iter().next().unwrap().result {
            OutputResult::Ok { files, .. } => {
                Ok(self.output.read(&files[0]).await.unwrap().to_vec())
            }
            OutputResult::Err { error } => Err(clone_kind(error.kind())),
        }
    }
}

/// `ErrorKind` is not `Clone`; keep what the tests look at.
fn clone_kind(kind: &ErrorKind) -> ErrorKind {
    match kind {
        ErrorKind::Corrupt { context } => ErrorKind::Corrupt {
            context: context.clone(),
        },
        ErrorKind::UnsupportedVersion { format, version } => ErrorKind::UnsupportedVersion {
            format: format.clone(),
            version: version.clone(),
        },
        ErrorKind::UnknownFormat => ErrorKind::UnknownFormat,
        other => ErrorKind::Custom {
            source: None,
            message: other.to_string(),
        },
    }
}

fn gradient(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [
                (x * 37 + 3) as u8,
                (y * 91 + 5) as u8,
                (x * y + 7) as u8,
                (255 - (x + y) * 13 % 256) as u8,
            ]
        })
        .collect()
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn dds(format: D3DFormat, w: u32, h: u32, data: Vec<u8>) -> Vec<u8> {
    let mut dds = Dds::new_d3d(NewD3dParams {
        height: h,
        width: w,
        depth: None,
        format,
        mipmap_levels: None,
        caps2: None,
    })
    .unwrap();
    dds.data = data;
    let mut out = Vec::new();
    dds.write(&mut out).unwrap();
    out
}

#[tokio::test]
async fn png_to_dds_to_png_keeps_the_pixels() {
    let env = env();
    let pixels = gradient(16, 8);
    env.input
        .write("icon.png", encode_png(16, 8, &pixels))
        .await
        .unwrap();

    let dds_bytes = env.convert("icon.png", "dds").await.unwrap();
    env.input.write("icon.dds", dds_bytes).await.unwrap();
    let png_bytes = env.convert("icon.dds", "png").await.unwrap();

    assert_eq!(decode_png(&png_bytes), (16, 8, pixels));
}

#[tokio::test]
async fn dds_to_png_to_dds_keeps_the_bytes() {
    let env = env();
    let original = dds(D3DFormat::A8B8G8R8, 5, 3, gradient(5, 3));
    env.input.write("tex.dds", original.clone()).await.unwrap();

    let png_bytes = env.convert("tex.dds", "png").await.unwrap();
    env.input.write("tex.png", png_bytes).await.unwrap();
    assert_eq!(env.convert("tex.png", "dds").await.unwrap(), original);
}

#[tokio::test]
async fn odd_sizes_round_trip() {
    for (w, h) in [(1, 1), (3, 5), (7, 2)] {
        let env = env();
        let pixels = gradient(w, h);
        env.input
            .write("a.png", encode_png(w, h, &pixels))
            .await
            .unwrap();
        let dds_bytes = env.convert("a.png", "dds").await.unwrap();
        env.input.write("a.dds", dds_bytes).await.unwrap();
        let png_bytes = env.convert("a.dds", "png").await.unwrap();
        assert_eq!(decode_png(&png_bytes), (w, h, pixels), "{w}x{h}");
    }
}

#[tokio::test]
async fn bgra_dds_gives_rgba_png_with_the_right_channel_order() {
    let env = env();
    env.input
        .write(
            "bgra.dds",
            dds(D3DFormat::A8R8G8B8, 1, 1, vec![10, 20, 30, 40]),
        )
        .await
        .unwrap();
    let png_bytes = env.convert("bgra.dds", "png").await.unwrap();
    assert_eq!(decode_png(&png_bytes), (1, 1, vec![30, 20, 10, 40]));
}

#[tokio::test]
async fn magic_beats_the_extension() {
    let env = env();
    env.input
        .write("fake.png", dds(D3DFormat::A8B8G8R8, 2, 2, gradient(2, 2)))
        .await
        .unwrap();
    let png_bytes = env.convert("fake.png", "png").await.unwrap();
    assert_eq!(decode_png(&png_bytes), (2, 2, gradient(2, 2)));
}

#[tokio::test]
async fn broken_input_is_corrupt() {
    let env = env();
    let good_png = encode_png(8, 8, &gradient(8, 8));
    let good_dds = dds(D3DFormat::A8B8G8R8, 4, 4, gradient(4, 4));
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("truncated.png", good_png[..good_png.len() / 2].to_vec()),
        ("garbage.png", [&good_png[..8], &[0xAA; 40][..]].concat()),
        ("short.dds", good_dds[..40].to_vec()),
        ("cut.dds", good_dds[..good_dds.len() - 8].to_vec()),
    ];
    for (name, bytes) in cases {
        env.input.write(name, bytes).await.unwrap();
        let target = if name.ends_with("png") { "dds" } else { "png" };
        let err = env.convert(name, target).await.unwrap_err();
        assert!(matches!(err, ErrorKind::Corrupt { .. }), "{name}: {err:?}");
    }
}

#[tokio::test]
async fn unrelated_bytes_are_an_unknown_format() {
    let env = env();
    env.input.write("notes.png", "hello").await.unwrap();
    assert!(matches!(
        env.convert("notes.png", "dds").await.unwrap_err(),
        ErrorKind::UnknownFormat
    ));
}
