//! Block-compressed DDS files to PNG, and the `png` target options.

use ddsfile::{
    AlphaMode, D3D10ResourceDimension, D3DFormat, Dds, DxgiFormat, NewD3dParams, NewDxgiParams,
};
use opendal::{services::Memory, Operator};
use respak_core::{ConvertRequest, Engine, ErrorKind, OutputResult};
use respak_formats::FormatsPlugin;

const RED_BC1: [u8; 8] = [0x00, 0xF8, 0, 0, 0, 0, 0, 0];
const BLUE_BC1: [u8; 8] = [0x1F, 0x00, 0, 0, 0, 0, 0, 0];

fn env() -> (Engine, Operator, Operator) {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(FormatsPlugin)
        .build()
        .unwrap();
    (engine, input, output)
}

fn dx10(format: DxgiFormat, w: u32, h: u32, mips: u32, data: Vec<u8>) -> Vec<u8> {
    let mut d = Dds::new_dxgi(NewDxgiParams {
        height: h,
        width: w,
        depth: None,
        format,
        mipmap_levels: (mips > 1).then_some(mips),
        array_layers: None,
        caps2: None,
        is_cubemap: false,
        resource_dimension: D3D10ResourceDimension::Texture2D,
        alpha_mode: AlphaMode::Unknown,
    })
    .unwrap();
    assert_eq!(d.data.len(), data.len());
    d.data = data;
    let mut out = Vec::new();
    d.write(&mut out).unwrap();
    out
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

async fn to_png(
    env: &(Engine, Operator, Operator),
    from: &str,
    to: serde_json::Value,
) -> Result<Vec<u8>, ErrorKind> {
    let request: ConvertRequest = serde_json::from_value(
        serde_json::json!({ "version": 1, "outputs": [{ "from": from, "to": to, "path": "out.png" }] }),
    )
    .unwrap();
    match env
        .0
        .convert(&request, &respak_core::CancellationToken::new())
        .await
        .unwrap()
        .outputs
        .remove(0)
        .result
    {
        OutputResult::Ok { files, .. } => Ok(env.2.read(&files[0]).await.unwrap().to_vec()),
        OutputResult::Err { error } => Err(match error.kind() {
            ErrorKind::InvalidOptions { message } => ErrorKind::InvalidOptions {
                message: message.clone(),
            },
            other => ErrorKind::Custom {
                source: None,
                message: other.to_string(),
            },
        }),
    }
}

#[tokio::test]
async fn a_legacy_dxt1_file_becomes_a_png() {
    let env = env();
    let mut d = Dds::new_d3d(NewD3dParams {
        height: 4,
        width: 4,
        depth: None,
        format: D3DFormat::DXT1,
        mipmap_levels: None,
        caps2: None,
    })
    .unwrap();
    d.data = RED_BC1.to_vec();
    let mut bytes = Vec::new();
    d.write(&mut bytes).unwrap();
    env.1.write("red.dds", bytes).await.unwrap();

    let (w, h, px) = decode(
        &to_png(&env, "red.dds", serde_json::json!({ "type": "png" }))
            .await
            .unwrap(),
    );
    assert_eq!((w, h), (4, 4));
    assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
}

#[tokio::test]
async fn the_mips_option_picks_a_level() {
    let env = env();
    // 8x8 BC1 with 2 mips: mip 0 = four red blocks, mip 1 = one blue block (4x4)
    let data = [RED_BC1, RED_BC1, RED_BC1, RED_BC1, BLUE_BC1].concat();
    env.1
        .write("t.dds", dx10(DxgiFormat::BC1_UNorm, 8, 8, 2, data))
        .await
        .unwrap();

    for (to, size, colour) in [
        (serde_json::json!({ "type": "png" }), 8, [255, 0, 0, 255]),
        (
            serde_json::json!({ "type": "png", "mips": "first" }),
            8,
            [255, 0, 0, 255],
        ),
        (
            serde_json::json!({ "type": "png", "mips": 0 }),
            8,
            [255, 0, 0, 255],
        ),
        (
            serde_json::json!({ "type": "png", "mips": 1 }),
            4,
            [0, 0, 255, 255],
        ),
    ] {
        let (w, h, px) = decode(&to_png(&env, "t.dds", to.clone()).await.unwrap());
        assert_eq!((w, h), (size, size), "{to}");
        assert!(px.chunks_exact(4).all(|p| p == colour), "{to}");
    }
}

#[tokio::test]
async fn bad_options_are_invalid_options() {
    let env = env();
    env.1
        .write(
            "t.dds",
            dx10(DxgiFormat::BC1_UNorm, 4, 4, 1, RED_BC1.to_vec()),
        )
        .await
        .unwrap();
    for to in [
        serde_json::json!({ "type": "png", "mips": 5 }),
        serde_json::json!({ "type": "png", "mips": "last" }),
        serde_json::json!({ "type": "png", "mips": -1 }),
        serde_json::json!({ "type": "png", "colour": "red" }),
    ] {
        let err = to_png(&env, "t.dds", to.clone()).await.unwrap_err();
        assert!(
            matches!(err, ErrorKind::InvalidOptions { .. }),
            "{to}: {err:?}"
        );
    }
}

#[tokio::test]
async fn the_dds_target_rejects_unknown_options() {
    let env = env();
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, 1, 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&[1, 2, 3, 4])
        .unwrap();
    env.1.write("a.png", bytes).await.unwrap();
    let err = to_png(
        &env,
        "a.png",
        serde_json::json!({ "type": "dds", "mips": 1 }),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ErrorKind::InvalidOptions { .. }), "{err:?}");
}

#[tokio::test]
async fn every_block_format_decodes_through_a_dds_file() {
    let env = env();
    for (name, format, block) in [
        ("bc1", DxgiFormat::BC1_UNorm, 8usize),
        ("bc2", DxgiFormat::BC2_UNorm, 16),
        ("bc3", DxgiFormat::BC3_UNorm_sRGB, 16),
        ("bc4", DxgiFormat::BC4_UNorm, 8),
        ("bc5", DxgiFormat::BC5_SNorm, 16),
        ("bc6", DxgiFormat::BC6H_UF16, 16),
        ("bc7", DxgiFormat::BC7_UNorm, 16),
    ] {
        env.1
            .write(
                &format!("{name}.dds"),
                dx10(format, 8, 4, 1, vec![0x5A; block * 2]),
            )
            .await
            .unwrap();
        let (w, h, px) = decode(
            &to_png(
                &env,
                &format!("{name}.dds"),
                serde_json::json!({ "type": "png" }),
            )
            .await
            .unwrap(),
        );
        assert_eq!((w, h, px.len()), (8, 4, 8 * 4 * 4), "{name}");
    }
}

#[tokio::test]
async fn mips_all_writes_a_folder_with_one_png_per_mip() {
    let env = env();
    // 8x8 BC1 with 2 mips: 4 blocks (red) and 1 block (blue)
    let red = [0x00, 0xF8, 0, 0, 0, 0, 0, 0];
    let blue = [0x1F, 0x00, 0, 0, 0, 0, 0, 0];
    env.1
        .write(
            "t.dds",
            dx10(
                DxgiFormat::BC1_UNorm,
                8,
                8,
                2,
                [red.repeat(4), blue.to_vec()].concat(),
            ),
        )
        .await
        .unwrap();
    let request: ConvertRequest = serde_json::from_value(
        serde_json::json!({ "version": 1, "outputs": [{ "from": "t.dds", "to": { "type": "png", "mips": "all" } }] }),
    )
    .unwrap();
    let report = env
        .0
        .convert(&request, &respak_core::CancellationToken::new())
        .await
        .unwrap();
    let OutputResult::Ok { files, .. } = &report.outputs[0].result else {
        panic!("{:?}", report.outputs[0].result)
    };
    assert_eq!(files, &["t/mip0.png", "t/mip1.png"]);
    let (w, h, px) = decode(&env.2.read("t/mip0.png").await.unwrap().to_vec());
    assert_eq!((w, h, &px[..4]), (8, 8, &[255, 0, 0, 255][..]));
    let (w, h, px) = decode(&env.2.read("t/mip1.png").await.unwrap().to_vec());
    assert_eq!((w, h, &px[..4]), (4, 4, &[0, 0, 255, 255][..]));
}
