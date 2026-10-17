//! `txtr` on synthetic paks: header versions, relocation, mip layout, errors.

mod common;

use bytes::Bytes;
use common::*;
use respak_core::capability::PixelFormat;
use respak_core::{Cx, ErrorKind, Registry, Value};
use respak_respawn::RespawnPlugin;

const BC1: u16 = 0;
const BC7: u16 = 12;
const RGBA8: u16 = 31;

fn cx(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&RespawnPlugin::new());
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

/// Opens `path` and reads it as a texture.
async fn texture(env: &Env, path: &str) -> respak_core::Result<Value> {
    let cx = cx(env);
    let asset = cx.mediator.open(path, &cx).await?;
    cx.mediator.identify(asset, &cx).await
}

/// Data bytes for a texture whose permanent mips are stored smallest first; every mip is
/// padded to 16 bytes and filled with its shift + 1 (+ 100 per array layer).
fn data(w: u32, h: u32, fmt: PixelFormat, first_shift: u32, total: u32, array: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for shift in (first_shift..total).rev() {
        let len = fmt.mip_len(w >> shift, h >> shift).div_ceil(16).max(1) * 16;
        for layer in 0..array {
            out.extend(std::iter::repeat_n(
                (shift + 1) as u8 + 100 * layer as u8,
                len as usize,
            ));
        }
    }
    out
}

#[tokio::test]
async fn every_header_version_is_read() {
    for (pak_v, tex_v) in [(8u16, 8u32), (8, 9), (8, 10), (7, 9), (6, 8)] {
        let env = env();
        let mut t = TexSpec::new(
            0xABCD,
            tex_v,
            16,
            8,
            BC1,
            3,
            data(16, 8, PixelFormat::Bc1, 0, 3, 1),
        );
        t.name = Some("ui/icon_col");
        env.input
            .write("c.rpak", build_tex_pak(pak_v, &[t]))
            .await
            .unwrap();

        let v = texture(&env, "c.rpak/0x000000000000ABCD.txtr")
            .await
            .unwrap();
        assert_eq!(
            v.format().to_string(),
            format!("txtr@{tex_v}"),
            "pak v{pak_v}"
        );
        let b = v.as_bitmap().unwrap().info();
        assert_eq!(
            (b.width, b.height, b.mip_count, b.format),
            (16, 8, 3, PixelFormat::Bc1)
        );
        assert_eq!(v.as_metadata().unwrap().name(), Some("texture/ui/icon_col"));
        assert_eq!(v.as_metadata().unwrap().guid(), Some(0xABCD));
    }
}

#[tokio::test]
async fn permanent_mips_are_ordered_largest_first_and_read_from_the_right_place() {
    let env = env();
    let t = TexSpec::new(
        1,
        9,
        16,
        16,
        BC7,
        3,
        data(16, 16, PixelFormat::Bc7, 0, 3, 1),
    );
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let v = texture(&env, "c.rpak/0x0000000000000001.txtr")
        .await
        .unwrap();
    let bmp = v.as_bitmap().unwrap();
    // shift 0 (16x16 = 16 blocks = 256 bytes) holds 1, shift 1 (8x8) holds 2, shift 2 (4x4) holds 3
    assert_eq!(
        bmp.read_mip(0, 0).await.unwrap(),
        Bytes::from(vec![1u8; 256])
    );
    assert_eq!(
        bmp.read_mip(1, 0).await.unwrap(),
        Bytes::from(vec![2u8; 64])
    );
    assert_eq!(
        bmp.read_mip(2, 0).await.unwrap(),
        Bytes::from(vec![3u8; 16])
    );
    assert!(bmp.read_mip(3, 0).await.is_err());
    assert!(bmp.read_mip(0, 1).await.is_err());
}

#[tokio::test]
async fn streamed_mips_without_a_starpak_reference_are_a_missing_dependency() {
    let env = env();
    // 64x64, 6 mips: 1 optional, 2 streamed, 3 permanent (shifts 3, 4, 5 = 8x8, 4x4, 2x2)
    let mut t = TexSpec::new(
        2,
        10,
        64,
        64,
        RGBA8,
        3,
        data(64, 64, PixelFormat::Rgba8, 3, 6, 1),
    );
    t.optional = 1;
    t.streamed = 2;
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let err = texture(&env, "c.rpak/0x0000000000000002.txtr")
        .await
        .unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::MissingDependency { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn array_slices_are_read_back_to_back() {
    let env = env();
    let mut t = TexSpec::new(
        3,
        9,
        8,
        8,
        RGBA8,
        2,
        data(8, 8, PixelFormat::Rgba8, 0, 2, 3),
    );
    t.array = 3;
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let v = texture(&env, "c.rpak/0x0000000000000003.txtr")
        .await
        .unwrap();
    let bmp = v.as_bitmap().unwrap();
    assert_eq!(bmp.info().array_len, 3);
    assert_eq!(
        bmp.read_mip(0, 2).await.unwrap(),
        Bytes::from(vec![1u8 + 200; 8 * 8 * 4])
    );
    assert_eq!(
        bmp.read_mip(1, 1).await.unwrap(),
        Bytes::from(vec![2u8 + 100; 4 * 4 * 4])
    );
}

#[tokio::test]
async fn a_missing_name_gives_no_name() {
    let env = env();
    let t = TexSpec::new(4, 8, 4, 4, BC1, 1, data(4, 4, PixelFormat::Bc1, 0, 1, 1));
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let v = texture(&env, "c.rpak/0x0000000000000004.txtr")
        .await
        .unwrap();
    assert_eq!(v.as_metadata().unwrap().name(), None);
}

#[tokio::test]
async fn unsupported_and_broken_textures_say_so() {
    let env = env();
    let good = data(4, 4, PixelFormat::Rgba8, 0, 1, 1);
    let mk = |guid: u64, f: fn(&mut TexSpec)| {
        let mut t = TexSpec::new(guid, 9, 4, 4, RGBA8, 1, good.clone());
        f(&mut t);
        t
    };
    let specs = vec![
        mk(1, |t| t.swizzle = 8),
        mk(2, |t| t.format = 62), // ASTC
        mk(3, |t| t.format = 200),
        mk(4, |t| t.permanent = 0),
        mk(5, |t| t.width = 0),
        mk(6, |t| t.permanent = 40),
        mk(7, |t| t.data = vec![0; 4]), // mips run past the pages
    ];
    env.input
        .write("c.rpak", build_tex_pak(8, &specs))
        .await
        .unwrap();
    for (guid, want_unsupported) in [
        (1, true),
        (2, true),
        (3, true),
        (4, false),
        (5, false),
        (6, false),
        (7, false),
    ] {
        let err = texture(&env, &format!("c.rpak/0x{guid:016X}.txtr"))
            .await
            .unwrap_err();
        match (err.kind(), want_unsupported) {
            (ErrorKind::UnsupportedVersion { format, .. }, true) => assert_eq!(format, "txtr"),
            (ErrorKind::Corrupt { .. }, false) => {}
            (other, _) => panic!("texture {guid}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn an_unknown_asset_version_is_unsupported() {
    let env = env();
    let mut t = TexSpec::new(
        1,
        9,
        4,
        4,
        RGBA8,
        1,
        data(4, 4, PixelFormat::Rgba8, 0, 1, 1),
    );
    t.version = 11;
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let err = texture(&env, "c.rpak/0x0000000000000001.txtr")
        .await
        .unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn headers_are_served_with_pointers_turned_into_mem_offsets() {
    let env = env();
    let mut t = TexSpec::new(5, 9, 4, 4, BC1, 1, data(4, 4, PixelFormat::Bc1, 0, 1, 1));
    t.name = Some("a/b");
    env.input
        .write("c.rpak", build_tex_pak(8, &[t]))
        .await
        .unwrap();
    let cx = cx(&env);
    let pak = cx.mediator.open("c.rpak", &cx).await.unwrap();
    let pak = cx.mediator.mount(pak, &cx).await.unwrap();
    let volume = pak.as_container().unwrap().volume();

    let header = volume
        .read("0x0000000000000005.txtr")
        .await
        .unwrap()
        .to_vec();
    // headers are page 0 (56 bytes), so page 1 starts at 56 in `.mem`; the name sits at its start
    assert_eq!(u64::from_le_bytes(header[..8].try_into().unwrap()), 56);
    let mem = volume.read(".mem").await.unwrap().to_vec();
    assert_eq!(&mem[56..59], b"a/b");
    assert_eq!(
        volume.stat(".mem").await.unwrap().content_length(),
        mem.len() as u64
    );
    assert_eq!(
        volume
            .read_with(".mem")
            .range(56..59)
            .await
            .unwrap()
            .to_vec(),
        b"a/b"
    );
}

#[tokio::test]
async fn a_pointer_to_a_missing_page_is_corrupt() {
    let env = env();
    let mut t = TexSpec::new(6, 9, 4, 4, BC1, 1, data(4, 4, PixelFormat::Bc1, 0, 1, 1));
    t.name = Some("x");
    let mut file = build_tex_pak(8, &[t]);
    // The header is the first thing in the page data; its name slot is {page 1, offset}. The
    // pages are 56 bytes (headers) and 32 (name + data, padded) = 88 bytes at the end.
    let at = file.len() - 88;
    file[at..at + 4].copy_from_slice(&9u32.to_le_bytes());
    env.input.write("c.rpak", file).await.unwrap();
    let err = texture(&env, "c.rpak/0x0000000000000006.txtr")
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}
