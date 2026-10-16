//! Streamed texture mips and starpaks on synthetic files.

mod common;

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use common::*;
use opendal::Operator;
use respak_core::capability::PixelFormat;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{Cx, ErrorKind, Mediator, Registry, Value};
use respak_respawn::RespawnPlugin;

const RGBA8: u16 = 31;
const PAK: &str = "paks/c.rpak";
const STREAMED: &str = "paks/streamed.starpak";
const OPT: &str = "paks/opt.starpak";

fn cx_on(op: &Operator) -> Cx {
    let mut r = Registry::new();
    r.add(&RespawnPlugin::new());
    Cx::new(Mediator::new(r), op.clone(), op.clone())
}

async fn texture_on(op: &Operator, path: &str) -> respak_core::Result<Value> {
    let cx = cx_on(op);
    let asset = cx.mediator.open(path, &cx).await?;
    cx.mediator.identify(asset, &cx).await
}

fn fill(len: usize, v: u8) -> Vec<u8> {
    vec![v; len]
}

/// 64x64 RGBA8, 6 mips: 1 optional (shift 0), 2 streamed (shifts 1, 2), 3 permanent (3, 4, 5).
/// Mip `s` is filled with `10 * (s + 1)` (permanent ones with `s + 1`, as `data` makes them).
fn spec(version: u32) -> TexSpec {
    let perm = {
        let mut out = Vec::new();
        for shift in (3..6u32).rev() {
            let len = PixelFormat::Rgba8
                .mip_len(64 >> shift, 64 >> shift)
                .div_ceil(16)
                .max(1)
                * 16;
            out.extend(fill(len as usize, shift as u8 + 1));
        }
        out
    };
    let mut t = TexSpec::new(7, version, 64, 64, RGBA8, 3, perm);
    t.optional = 1;
    t.streamed = 2;
    t.stream_at = Some(0x1000);
    t.opt_at = Some(0x1000);
    t
}

/// The mandatory starpak entry for `spec`: shifts 1 and 2, in the order of `version`.
fn streamed_entry(version: u32) -> Vec<u8> {
    let (s1, s2) = (fill(32 * 32 * 4, 20), fill(16 * 16 * 4, 30));
    if version == 8 {
        [s2, s1].concat()
    } else {
        [s1, s2].concat()
    }
}

async fn put(env: &Env, tex: &TexSpec, streamed: bool, opt: bool) {
    env.input
        .write(PAK, build_tex_pak(8, std::slice::from_ref(tex)))
        .await
        .unwrap();
    if streamed {
        let entry = streamed_entry(tex.version);
        env.input
            .write(STREAMED, build_starpak(&[(0x1000, entry)]))
            .await
            .unwrap();
    }
    if opt {
        env.input
            .write(OPT, build_starpak(&[(0x1000, fill(64 * 64 * 4, 10))]))
            .await
            .unwrap();
    }
}

const GUID: &str = "paks/c.rpak/0x0000000000000007.txtr";

#[tokio::test]
async fn streamed_and_optional_mips_make_a_full_chain() {
    for version in [8u32, 9, 10] {
        let env = env();
        put(&env, &spec(version), true, true).await;
        let v = texture_on(&env.input, GUID).await.unwrap();
        let bmp = v.as_bitmap().unwrap();
        let info = bmp.info();
        assert_eq!(
            (info.width, info.height, info.mip_count),
            (64, 64, 6),
            "v{version}"
        );
        // every mip 0..=2 comes from a starpak, 3..=5 from the pak
        for (level, bytes, fill) in [
            (0u32, 64 * 64 * 4, 10u8),
            (1, 32 * 32 * 4, 20),
            (2, 16 * 16 * 4, 30),
            (3, 8 * 8 * 4, 4),
        ] {
            assert_eq!(
                bmp.read_mip(level, 0).await.unwrap(),
                Bytes::from(vec![fill; bytes]),
                "v{version} level {level}"
            );
        }
    }
}

#[tokio::test]
async fn a_missing_optional_starpak_falls_back_to_lower_mips() {
    let env = env();
    put(&env, &spec(10), true, false).await;
    let v = texture_on(&env.input, GUID).await.unwrap();
    let bmp = v.as_bitmap().unwrap();
    let info = bmp.info();
    assert_eq!((info.width, info.height, info.mip_count), (32, 32, 5));
    assert_eq!(
        bmp.read_mip(0, 0).await.unwrap(),
        Bytes::from(fill(32 * 32 * 4, 20))
    );
    assert_eq!(
        bmp.read_mip(4, 0).await.unwrap(),
        Bytes::from(fill(2 * 2 * 4, 6))
    );
}

#[tokio::test]
async fn skipped_optional_mips_are_a_warning_event() {
    let env = env();
    put(&env, &spec(10), true, false).await;
    let cx = cx_on(&env.input);
    let mut rx = cx.mediator.subscribe();
    let asset = cx.mediator.open(GUID, &cx).await.unwrap();
    cx.mediator.identify(asset, &cx).await.unwrap();
    let mut warnings = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if let respak_core::Event::Warning { message } = e {
            warnings.push(message);
        }
    }
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("1 optional mip(s) are skipped"),
        "{warnings:?}"
    );
}

#[tokio::test]
async fn a_missing_mandatory_starpak_is_a_missing_dependency() {
    let env = env();
    put(&env, &spec(10), false, true).await;
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    match err.kind() {
        ErrorKind::MissingDependency { item, file } => {
            assert!(item.contains("0x0000000000000007"), "{item}");
            assert_eq!(file, STREAMED);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_entry_missing_from_the_table_of_contents_is_a_missing_dependency() {
    let env = env();
    put(&env, &spec(10), false, true).await;
    env.input
        .write(STREAMED, build_starpak(&[(0x5000, fill(5120, 1))]))
        .await
        .unwrap();
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::MissingDependency { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn mips_that_do_not_fit_their_entry_are_corrupt() {
    let env = env();
    put(&env, &spec(10), false, true).await;
    env.input
        .write(STREAMED, build_starpak(&[(0x1000, fill(100, 1))]))
        .await
        .unwrap();
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_broken_table_of_contents_is_corrupt() {
    let env = env();
    put(&env, &spec(10), false, true).await;
    env.input
        .write(
            STREAMED,
            [fill(40, 0), 1_000_000u64.to_le_bytes().to_vec()].concat(),
        )
        .await
        .unwrap();
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
    env.input.write(STREAMED, vec![1, 2, 3]).await.unwrap();
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_starpak_index_past_the_list_is_corrupt() {
    let env = env();
    let mut t = spec(10);
    t.stream_at = Some(0x1000 | 5);
    put(&env, &t, true, true).await;
    let err = texture_on(&env.input, GUID).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

/// Version 10 `spec` whose streamed mip 2 (16x16, 1024 bytes) is in a 4096-byte block with
/// `codec`; mip 1 stays plain.
async fn put_with_block(env: &Env, codec: u16, block: Vec<u8>) {
    let mut t = spec(10);
    t.codecs = codec << 4;
    let mut entry = fill(32 * 32 * 4, 20);
    let mut block = block;
    block.resize(4096, 0);
    entry.extend(block);
    env.input
        .write(PAK, build_tex_pak(8, std::slice::from_ref(&t)))
        .await
        .unwrap();
    env.input
        .write(STREAMED, build_starpak(&[(0x1000, entry)]))
        .await
        .unwrap();
    env.input
        .write(OPT, build_starpak(&[(0x1000, fill(64 * 64 * 4, 10))]))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_oodle_mip_in_a_padded_block_decodes_to_its_length() {
    let env = env();
    put_with_block(&env, 3, oodle_stored(&fill(16 * 16 * 4, 30), 0)).await;
    let v = texture_on(&env.input, GUID).await.unwrap();
    let bmp = v.as_bitmap().unwrap();
    assert_eq!(
        bmp.read_mip(2, 0).await.unwrap(),
        Bytes::from(fill(16 * 16 * 4, 30))
    );
    // the plain neighbours are untouched
    assert_eq!(
        bmp.read_mip(1, 0).await.unwrap(),
        Bytes::from(fill(32 * 32 * 4, 20))
    );
}

#[tokio::test]
async fn a_broken_oodle_mip_is_corrupt_and_other_codecs_say_so() {
    let env = env();
    let stream = oodle_stored(&fill(16 * 16 * 4, 30), 0);
    // a block that claims more compressed bytes than the padded block holds
    put_with_block(&env, 3, vec![0x8C, 0x06, 0x03, 0xFF, 0xFE]).await;
    let v = texture_on(&env.input, GUID).await.unwrap();
    let err = v.as_bitmap().unwrap().read_mip(2, 0).await.unwrap_err();
    assert!(err.to_string().contains("oodle"), "{err}");
    // not a stream at all
    put_with_block(&env, 3, vec![0xFF; 100]).await;
    let v = texture_on(&env.input, GUID).await.unwrap();
    assert!(v.as_bitmap().unwrap().read_mip(2, 0).await.is_err());
    // Snowflake
    put_with_block(&env, 2, stream).await;
    let v = texture_on(&env.input, GUID).await.unwrap();
    let err = v.as_bitmap().unwrap().read_mip(2, 0).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn a_starpak_is_a_container_of_its_entries() {
    let env = env();
    env.input
        .write(
            "s.starpak",
            build_starpak(&[(0x1000, fill(100, 7)), (0x3000, fill(50, 8))]),
        )
        .await
        .unwrap();
    let cx = cx_on(&env.input);
    let pak = cx.mediator.open("s.starpak", &cx).await.unwrap();
    let pak = cx.mediator.mount(pak, &cx).await.unwrap();
    assert_eq!(pak.format().to_string(), "starpak");
    let volume = pak.as_container().unwrap().volume();
    let mut names: Vec<_> = volume
        .list("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name().to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["0x0000000000001000", "0x0000000000003000"]);
    assert_eq!(
        volume.read("0x0000000000003000").await.unwrap().to_vec(),
        fill(50, 8)
    );
    assert_eq!(
        volume
            .stat("0x0000000000001000")
            .await
            .unwrap()
            .content_length(),
        100
    );
    assert!(volume
        .read_with("0x0000000000001000")
        .range(90..110)
        .await
        .is_err());
}

// ---- only the table of contents and the requested ranges are read -----------------------------

const GIB: u64 = 1 << 30;

/// A host with a real rpak and a starpak of 2 GiB + a few KiB that exists only as a recipe:
/// zeros, the mandatory entry at 2 GiB, and the table of contents. Counts the bytes read.
struct Host {
    pak: Vec<u8>,
    entry: Vec<u8>,
    starpak_len: u64,
    toc: Vec<u8>,
    read: Arc<AtomicU64>,
}

#[async_trait::async_trait]
impl VolumeSource for Host {
    async fn stat(&self, path: &str) -> respak_core::Result<VolumeStat> {
        match path {
            "" => Ok(VolumeStat {
                is_dir: true,
                len: 0,
            }),
            PAK => Ok(VolumeStat {
                is_dir: false,
                len: self.pak.len() as u64,
            }),
            STREAMED => Ok(VolumeStat {
                is_dir: false,
                len: self.starpak_len,
            }),
            _ => Err(respak_core::Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })),
        }
    }

    async fn read(&self, path: &str, range: Range<u64>) -> respak_core::Result<Bytes> {
        if path == PAK {
            return Ok(Bytes::copy_from_slice(
                &self.pak[range.start as usize..range.end as usize],
            ));
        }
        self.read
            .fetch_add(range.end - range.start, Ordering::SeqCst);
        let mut out = vec![0u8; (range.end - range.start) as usize];
        let regions = [
            (2 * GIB, &self.entry),
            (self.starpak_len - self.toc.len() as u64, &self.toc),
        ];
        for (at, bytes) in regions {
            let (s, e) = (range.start.max(at), range.end.min(at + bytes.len() as u64));
            if s < e {
                out[(s - range.start) as usize..(e - range.start) as usize]
                    .copy_from_slice(&bytes[(s - at) as usize..(e - at) as usize]);
            }
        }
        Ok(out.into())
    }

    async fn list(&self, _: &str) -> respak_core::Result<Vec<VolumeEntry>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn a_huge_starpak_costs_its_table_of_contents_and_the_requested_ranges() {
    let mut t = spec(10);
    t.stream_at = Some(2 * GIB);
    t.opt_at = None;
    t.optional = 0;
    t.streamed = 2;
    // 5 mips now: 2 streamed (shifts 0, 1: 64x64 and 32x32), 3 permanent (shifts 2.. -> need data)
    t.permanent = 3;
    t.data = {
        let mut out = Vec::new();
        for shift in (2..5u32).rev() {
            let len = PixelFormat::Rgba8
                .mip_len(64 >> shift, 64 >> shift)
                .div_ceil(16)
                .max(1)
                * 16;
            out.extend(fill(len as usize, shift as u8 + 1));
        }
        out
    };
    let entry = [fill(64 * 64 * 4, 11), fill(32 * 32 * 4, 22)].concat();
    let toc = [
        (2 * GIB).to_le_bytes().to_vec(),
        (entry.len() as u64).to_le_bytes().to_vec(),
        1u64.to_le_bytes().to_vec(),
    ]
    .concat();
    let read = Arc::new(AtomicU64::new(0));
    let host = Host {
        pak: build_tex_pak(8, &[t]),
        starpak_len: 2 * GIB + 32 * 1024 + toc.len() as u64,
        entry,
        toc,
        read: read.clone(),
    };
    let op = volume_operator(host, "host");

    let v = texture_on(&op, GUID).await.unwrap();
    let bmp = v.as_bitmap().unwrap();
    assert_eq!(bmp.info().mip_count, 5);
    let after_open = read.load(Ordering::SeqCst);
    assert!(
        after_open <= 8 + 16,
        "opening read {after_open} bytes of the starpak"
    );

    let mip = bmp.read_mip(1, 0).await.unwrap();
    assert_eq!(mip, Bytes::from(fill(32 * 32 * 4, 22)));
    let total = read.load(Ordering::SeqCst);
    assert_eq!(
        total - after_open,
        32 * 32 * 4,
        "only the requested mip is read"
    );
    assert!(total < 16 * 1024, "{total} bytes read of a 2 GiB starpak");
}
