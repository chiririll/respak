//! The DDS encoder on bitmaps other plugins would provide: BC, mips, arrays.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ddsfile::{Dds, DxgiFormat};
use opendal::{services::Memory, Operator};
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::{
    magic, ConvertRequest, Cx, Data, Detect, Detected, Engine, ErrorKind, FormatId, OutputResult,
    Plugin, Probe, ReadHandler, Registry, Result, Value, ValueKey,
};
use respak_formats::FormatsPlugin;

/// A format `fake` whose files (`FAKE...`) decode to a fixed bitmap.
#[derive(Debug)]
struct FakeImage {
    key: ValueKey,
    format: FormatId,
    buf: BitmapBuf,
}

impl Data for FakeImage {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(&self.buf)
    }
}

#[derive(Clone)]
struct FakeRead(BitmapBuf);

#[async_trait]
impl Detect for FakeRead {
    async fn score(&self, input: &Probe<'_>, _: &Cx) -> Result<Option<Detected>> {
        Ok(magic(input.header, 0, b"FAKE").then_some(Detected {
            score: 10,
            version: None,
        }))
    }
    fn header_len(&self) -> usize {
        4
    }
}

#[async_trait]
impl ReadHandler for FakeRead {
    async fn read(&self, source: Value, _: &Detected, _: &Cx) -> Result<Value> {
        Ok(Arc::new(FakeImage {
            key: source.key().clone(),
            format: FormatId::new("fake"),
            buf: self.0.clone(),
        }))
    }
}

struct FakePlugin(BitmapBuf);

impl Plugin for FakePlugin {
    fn id(&self) -> &str {
        "fake"
    }
    fn register(&self, r: &mut Registry) {
        r.format("fake")
            .from("file", FakeRead(self.0.clone()))
            .provides::<dyn Bitmap>();
    }
}

fn info(format: PixelFormat, w: u32, h: u32, mips: u32, layers: u32) -> BitmapInfo {
    BitmapInfo {
        width: w,
        height: h,
        depth: 1,
        array_len: layers,
        faces: 1,
        mip_count: mips,
        format,
    }
}

/// Mip `level` of `layer` is filled with the byte `layer * 16 + level + 1`.
fn bitmap(i: BitmapInfo) -> BitmapBuf {
    let mips = (0..i.layer_count())
        .map(|layer| {
            (0..i.mip_count)
                .map(|level| {
                    let len = i.format.mip_len(i.width >> level, i.height >> level) as usize;
                    Bytes::from(vec![(layer * 16 + level + 1) as u8; len])
                })
                .collect()
        })
        .collect();
    BitmapBuf::new(i, mips).unwrap()
}

async fn to_dds(b: BitmapBuf) -> std::result::Result<Vec<u8>, ErrorKind> {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    input.write("x.fake", "FAKE").await.unwrap();
    let engine = Engine::builder()
        .input(input)
        .output(output.clone())
        .plugin(FormatsPlugin)
        .plugin(FakePlugin(b))
        .build()
        .unwrap();
    let request: ConvertRequest =
        serde_json::from_str(r#"{"version":1,"outputs":[{"from":"x.fake","to":{"type":"dds"}}]}"#)
            .unwrap();
    match engine
        .convert(&request, &respak_core::CancellationToken::new())
        .await
        .unwrap()
        .outputs
        .remove(0)
        .result
    {
        OutputResult::Ok { files, .. } => Ok(output.read(&files[0]).await.unwrap().to_vec()),
        OutputResult::Err { error } => Err(match error.kind() {
            ErrorKind::UnsupportedVersion { .. } => ErrorKind::UnknownFormat,
            ErrorKind::Corrupt { context } => ErrorKind::Corrupt {
                context: context.clone(),
            },
            other => ErrorKind::Custom {
                source: None,
                message: other.to_string(),
            },
        }),
    }
}

#[test]
fn mip_lengths_round_blocks_up() {
    assert_eq!(PixelFormat::Bc1.mip_len(8, 8), 32);
    assert_eq!(PixelFormat::Bc7.mip_len(1, 1), 16);
    assert_eq!(PixelFormat::Bc7.mip_len(5, 4), 32);
    assert_eq!(PixelFormat::Rgba8.mip_len(3, 5), 60);
    assert_eq!(PixelFormat::Rgba16F.mip_len(2, 2), 32);
}

#[tokio::test]
async fn bc_mips_are_copied_as_they_are_with_a_dx10_header() {
    for (format, dxgi) in [
        (PixelFormat::Bc1, DxgiFormat::BC1_UNorm),
        (PixelFormat::Bc3Srgb, DxgiFormat::BC3_UNorm_sRGB),
        (PixelFormat::Bc5Snorm, DxgiFormat::BC5_SNorm),
        (PixelFormat::Bc6H, DxgiFormat::BC6H_UF16),
        (PixelFormat::Bc7, DxgiFormat::BC7_UNorm),
    ] {
        let buf = bitmap(info(format, 8, 8, 4, 1));
        let bytes = to_dds(buf.clone()).await.unwrap();
        let dds = Dds::read(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(
            dds.header10.as_ref().map(|h| h.dxgi_format),
            Some(dxgi),
            "{format:?}"
        );
        assert_eq!(
            (
                dds.get_width(),
                dds.get_height(),
                dds.get_num_mipmap_levels()
            ),
            (8, 8, 4)
        );
        let mut expected = Vec::new();
        for level in 0..4 {
            expected.extend_from_slice(&buf.read_mip(level, 0).await.unwrap());
        }
        assert_eq!(dds.data, expected, "{format:?}");
    }
}

#[tokio::test]
async fn arrays_put_layers_outside_and_mips_inside() {
    let buf = bitmap(info(PixelFormat::Rgba8, 4, 4, 3, 2));
    let bytes = to_dds(buf.clone()).await.unwrap();
    let dds = Dds::read(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(dds.get_num_array_layers(), 2);
    let mut expected = Vec::new();
    for layer in 0..2 {
        for level in 0..3 {
            expected.extend_from_slice(&buf.read_mip(level, layer).await.unwrap());
        }
    }
    assert_eq!(dds.data, expected);
}

#[tokio::test]
async fn srgb_and_float_formats_keep_their_dxgi_format() {
    for (format, dxgi) in [
        (PixelFormat::Rgba8Srgb, DxgiFormat::R8G8B8A8_UNorm_sRGB),
        (PixelFormat::Rgba16F, DxgiFormat::R16G16B16A16_Float),
    ] {
        let bytes = to_dds(bitmap(info(format, 2, 2, 1, 1))).await.unwrap();
        let dds = Dds::read(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(dds.header10.unwrap().dxgi_format, dxgi);
    }
}

#[tokio::test]
async fn a_single_rgba8_layer_still_gets_a_legacy_header() {
    let bytes = to_dds(bitmap(info(PixelFormat::Rgba8, 2, 2, 1, 1)))
        .await
        .unwrap();
    let dds = Dds::read(std::io::Cursor::new(&bytes)).unwrap();
    assert!(dds.header10.is_none());
}

#[tokio::test]
async fn cube_maps_are_unsupported_and_wrong_mip_sizes_are_corrupt() {
    let mut cube = info(PixelFormat::Rgba8, 2, 2, 1, 1);
    cube.faces = 6;
    assert!(matches!(
        to_dds(bitmap(cube)).await.unwrap_err(),
        ErrorKind::UnknownFormat
    ));

    let i = info(PixelFormat::Bc7, 8, 8, 1, 1);
    let wrong = BitmapBuf::new(i, vec![vec![Bytes::from(vec![0u8; 5])]]).unwrap();
    assert!(matches!(
        to_dds(wrong).await.unwrap_err(),
        ErrorKind::Corrupt { .. }
    ));
}
