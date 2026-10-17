//! Bink 2 files on hand-built data: detection, discovery, unsupported and damaged input.
//! No game files: the real decoder is checked by hand (task 15 demo).

use opendal::{services::Memory, Operator};
use respak_core::{CancellationToken, Cx, Engine, ErrorKind, Registry};
use respak_formats::FormatsPlugin;
use respak_rad::RadPlugin;

/// The 44-byte fixed header of a Bink file, then `extra` bytes.
fn header(
    magic: &[u8; 4],
    frames: u32,
    width: u32,
    height: u32,
    fps: (u32, u32),
    tracks: u32,
    extra: usize,
) -> Vec<u8> {
    let mut h = magic.to_vec();
    h.extend(1000u32.to_le_bytes()); // file size - 8
    h.extend(frames.to_le_bytes());
    h.extend(100u32.to_le_bytes()); // largest frame
    h.extend(frames.to_le_bytes());
    h.extend(width.to_le_bytes());
    h.extend(height.to_le_bytes());
    h.extend(fps.0.to_le_bytes());
    h.extend(fps.1.to_le_bytes());
    h.extend(0u32.to_le_bytes()); // flags
    h.extend(tracks.to_le_bytes());
    h.resize(44 + extra, 0);
    h
}

fn kb2i() -> Vec<u8> {
    header(b"KB2i", 210, 1920, 1080, (30, 1), 1, 100)
}

struct Env {
    engine: Engine,
    input: Operator,
}

fn env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output)
        .plugin(FormatsPlugin)
        .plugin(RadPlugin)
        .build()
        .unwrap();
    Env { engine, input }
}

async fn discover(env: &Env, path: &str) -> respak_core::DiscoveryEntry {
    let d = env
        .engine
        .discover(path, &Default::default(), &CancellationToken::new())
        .await
        .unwrap();
    d.entries.into_iter().next().unwrap()
}

#[tokio::test]
async fn a_bink2_header_is_detected_with_its_revision() {
    let env = env();
    env.input.write("media/a.bik", kb2i()).await.unwrap();
    env.input.write("media/b.bk2", kb2i()).await.unwrap();
    env.input.write("media/c.dat", kb2i()).await.unwrap();

    let a = discover(&env, "media/a.bik").await;
    assert_eq!(
        (a.format.as_deref(), a.version.as_deref()),
        (Some("bik"), Some("2.8"))
    );
    // Magic 10 + plausible fields 2 + extension 1.
    assert_eq!(a.score, 13);
    assert_eq!(discover(&env, "media/b.bk2").await.score, 13);
    // The extension only adds a point: the content decides.
    let c = discover(&env, "media/c.dat").await;
    assert_eq!((c.format.as_deref(), c.score), (Some("bik"), 12));
}

#[tokio::test]
async fn other_revisions_keep_their_letter() {
    let env = env();
    env.input
        .write("j.bik", header(b"KB2j", 10, 640, 360, (30, 1), 0, 0))
        .await
        .unwrap();
    assert_eq!(
        discover(&env, "j.bik").await.version.as_deref(),
        Some("2.9")
    );
}

#[tokio::test]
async fn implausible_headers_are_not_bink() {
    let env = env();
    let bad = [
        (
            "zero_frames.bik",
            header(b"KB2i", 0, 1920, 1080, (30, 1), 1, 100),
        ),
        (
            "zero_fps.bik",
            header(b"KB2i", 10, 1920, 1080, (0, 1), 1, 100),
        ),
        (
            "zero_den.bik",
            header(b"KB2i", 10, 1920, 1080, (30, 0), 1, 100),
        ),
        (
            "zero_size.bik",
            header(b"KB2i", 10, 0, 1080, (30, 1), 1, 100),
        ),
        (
            "huge.bik",
            header(b"KB2i", 10, 1 << 20, 1080, (30, 1), 1, 100),
        ),
        ("short.bik", kb2i()[..20].to_vec()),
        (
            "noise.bik",
            (0..200u32).map(|i| (i * 37 % 251) as u8).collect(),
        ),
        (
            "not_a_letter.bik",
            header(b"KB2\x01", 10, 640, 360, (30, 1), 0, 0),
        ),
    ];
    for (name, bytes) in bad {
        env.input.write(name, bytes).await.unwrap();
        assert_eq!(discover(&env, name).await.format, None, "{name}");
    }
}

#[tokio::test]
async fn discover_lists_frames_and_wav_as_targets() {
    let env = env();
    env.input.write("a.bik", kb2i()).await.unwrap();
    let targets: Vec<_> = discover(&env, "a.bik")
        .await
        .targets
        .into_iter()
        .map(|t| t.format)
        .collect();
    assert!(targets.contains(&"png.frames".to_owned()), "{targets:?}");
    assert!(targets.contains(&"wav".to_owned()), "{targets:?}");
}

#[tokio::test]
async fn bink_1_is_unsupported_not_unknown() {
    let env = env();
    env.input
        .write("old.bik", header(b"BIKi", 10, 640, 360, (30, 1), 0, 100))
        .await
        .unwrap();
    let e = discover(&env, "old.bik").await;
    assert_eq!(
        (e.format.as_deref(), e.version.as_deref()),
        (Some("bik"), Some("1.8"))
    );

    let mut r = Registry::new();
    r.add(&FormatsPlugin);
    r.add(&RadPlugin);
    let cx = respak_core::testing::cx(r, env.input.clone(), env.input.clone());
    let file = cx.mediator.open("old.bik", &cx).await.unwrap();
    let err = cx.mediator.identify(file, &cx).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { format, .. } if format == "bik"),
        "{err:?}"
    );
}

fn cx_on(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&FormatsPlugin);
    r.add(&RadPlugin);
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

async fn identify(env: &Env, path: &str) -> respak_core::Result<respak_core::Value> {
    let cx = cx_on(env);
    let file = cx.mediator.open(path, &cx).await?;
    cx.mediator.identify(file, &cx).await
}

fn assert_corrupt(result: respak_core::Result<respak_core::Value>, what: &str) {
    match result {
        Err(e) if matches!(e.kind(), ErrorKind::Corrupt { .. }) => {}
        Err(e) => panic!("{what}: expected Corrupt, got {e:?}"),
        Ok(_) => panic!("{what}: expected Corrupt, got a value"),
    }
}

#[tokio::test]
async fn garbage_after_a_valid_header_is_corrupt_and_never_panics() {
    let env = env();
    // The index fits (2 frames) but the bytes after the header are noise.
    let mut noisy = header(b"KB2i", 2, 640, 360, (30, 1), 0, 0);
    noisy.extend((0..200u32).map(|i| (i * 131 % 253) as u8));
    env.input.write("noise.bik", noisy).await.unwrap();
    assert_corrupt(identify(&env, "noise.bik").await, "noise");

    // All zeros after the header.
    env.input
        .write("zeros.bik", header(b"KB2i", 2, 640, 360, (30, 1), 1, 300))
        .await
        .unwrap();
    assert_corrupt(identify(&env, "zeros.bik").await, "zeros");
}

#[tokio::test]
async fn a_frame_count_larger_than_the_file_is_corrupt_without_a_huge_allocation() {
    let env = env();
    for frames in [211, 1 << 20, u32::MAX] {
        env.input
            .write(
                "big.bik",
                header(b"KB2i", frames, 1920, 1080, (30, 1), 1, 100),
            )
            .await
            .unwrap();
        assert_corrupt(identify(&env, "big.bik").await, &format!("{frames} frames"));
    }
}
