//! `wrap` on synthetic paks: stored, streamed and compressed files, names, versions, errors.

mod common;

use common::*;
use respak_core::capability::read_all;
use respak_core::{Cx, ErrorKind, Registry, Value};
use respak_respawn::RespawnPlugin;

const PATH: &str = "paks/c.rpak";
const STREAMED: &str = "paks/streamed.starpak";
const OPT: &str = "paks/opt.starpak";
const FULL: &str = "scripts/vscripts/foo.gnut.client";
/// Length of `scripts/vscripts/foo.gnut`.
const REAL: u16 = 25;

fn asset(guid: u64) -> String {
    format!("{PATH}/0x{guid:016X}.wrap")
}

fn cx(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&RespawnPlugin::new());
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

fn builder() -> PakBuilder {
    let mut b = PakBuilder::new();
    b.starpak_files(
        &[r"paks\Win64\streamed.starpak"],
        &["paks/Win64/opt.starpak"],
    );
    b
}

async fn put(env: &Env, pak: PakBuilder) {
    env.input.write(PATH, pak.build(8)).await.unwrap();
}

async fn wrap(env: &Env, guid: u64) -> respak_core::Result<Value> {
    let cx = cx(env);
    let asset = cx.mediator.open(&asset(guid), &cx).await?;
    cx.mediator.identify(asset, &cx).await
}

async fn bytes(env: &Env, value: &Value) -> respak_core::Result<Vec<u8>> {
    let cx = cx(env);
    Ok(read_all(value.as_blob().unwrap(), &cx).await?.to_vec())
}

#[tokio::test]
async fn stored_data_is_the_file_and_the_name_skips_the_first_folder() {
    let env = env();
    let mut b = builder();
    b.wrap_v7(1, FULL, REAL, 8, 0, 9, 9, Some(b"print(1)\n"));
    put(&env, b).await;
    let v = wrap(&env, 1).await.unwrap();
    assert_eq!(bytes(&env, &v).await.unwrap(), b"print(1)\n");
    let meta = v.as_metadata().unwrap();
    assert_eq!(meta.name(), Some("vscripts/foo.gnut.client"));
    assert_eq!(meta.kind(), Some("wrap"));
    assert_eq!(meta.guid(), Some(1));
    assert_eq!(
        v.as_blob().unwrap().len(),
        None,
        "a text file's length is not known up front"
    );
}

#[tokio::test]
async fn text_files_lose_one_trailing_nul_and_others_are_kept() {
    let env = env();
    let mut b = builder();
    b.wrap_v7(1, "a/b.txt", 7, 2, 0, 4, 4, Some(b"hi\n\0"));
    b.wrap_v7(2, "a/b.dat", 7, 2, 0, 3, 3, Some(b"hi\0"));
    put(&env, b).await;
    let text = wrap(&env, 1).await.unwrap();
    assert_eq!(bytes(&env, &text).await.unwrap(), b"hi\n");
    let other = wrap(&env, 2).await.unwrap();
    assert_eq!(bytes(&env, &other).await.unwrap(), b"hi\0");
    assert_eq!(other.as_blob().unwrap().len(), Some(3));
}

#[tokio::test]
async fn a_range_read_is_cut_at_the_end() {
    let env = env();
    let mut b = builder();
    b.wrap_v7(1, "a/b.dat", 7, 2, 0, 5, 5, Some(b"abcde"));
    put(&env, b).await;
    let v = wrap(&env, 1).await.unwrap();
    let cx = cx(&env);
    let blob = v.as_blob().unwrap();
    assert_eq!(&blob.read(1..3, &cx).await.unwrap()[..], b"bc");
    assert_eq!(&blob.read(3..99, &cx).await.unwrap()[..], b"de");
    assert!(blob.read(9..12, &cx).await.unwrap().is_empty());
}

#[tokio::test]
async fn permanent_uncompressed_data_starts_two_bytes_late() {
    let env = env();
    let mut b = builder();
    // flags COMPRESSED | PERMANENT with cmp > dcmp: not compressed, 2 bytes of leftover first
    b.wrap_v7(1, "a/b.dat", 7, 2, 0x5, 7, 5, Some(b"\xFF\xFFabcde"));
    put(&env, b).await;
    let v = wrap(&env, 1).await.unwrap();
    assert_eq!(bytes(&env, &v).await.unwrap(), b"abcde");
}

#[tokio::test]
async fn streamed_data_skips_the_prefix_and_falls_back_to_the_optional_starpak() {
    let env = env();
    let mut b = builder();
    b.wrap_v7(1, "a/b.dat", 7, 2, 0x10, 7, 5, None);
    b.stream_last(Some(0x1000), None);
    b.wrap_v7(2, "a/c.dat", 7, 2, 0x10, 5, 5, None);
    b.stream_last(None, Some(0x1000));
    put(&env, b).await;
    env.input
        .write(STREAMED, build_starpak(&[(0x1000, b"\0\0abcde".to_vec())]))
        .await
        .unwrap();
    env.input
        .write(OPT, build_starpak(&[(0x1000, b"vwxyz".to_vec())]))
        .await
        .unwrap();
    let v = wrap(&env, 1).await.unwrap();
    assert_eq!(bytes(&env, &v).await.unwrap(), b"abcde");
    let v = wrap(&env, 2).await.unwrap();
    assert_eq!(bytes(&env, &v).await.unwrap(), b"vwxyz");
}

#[tokio::test]
async fn streamed_data_without_a_starpak_reference_is_a_missing_dependency() {
    let env = env();
    let mut b = builder();
    b.wrap_v7(1, "a/b.dat", 7, 2, 0x10, 5, 5, None);
    put(&env, b).await;
    let v = wrap(&env, 1).await.unwrap();
    let err = bytes(&env, &v).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::MissingDependency { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn version_1_has_data_and_no_name() {
    let env = env();
    let mut b = builder();
    b.wrap_v1(1, b"hello");
    put(&env, b).await;
    let v = wrap(&env, 1).await.unwrap();
    assert_eq!(bytes(&env, &v).await.unwrap(), b"hello");
    assert_eq!(v.as_metadata().unwrap().name(), None);
    assert_eq!(v.format().version().map(|v| v.major), Some(1));
}

#[tokio::test]
async fn other_versions_are_unsupported() {
    let env = env();
    let mut b = builder();
    b.asset(1, b"wrap", 3, vec![0; 0x28], &[], None);
    put(&env, b).await;
    let err = wrap(&env, 1).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn bad_paths_and_data_are_corrupt() {
    let env = env();
    let mut b = builder();
    // pathSize past the end of the path
    b.wrap_v7(1, "a/b.dat", 99, 2, 0, 1, 1, Some(b"x"));
    // data runs past the end of the pak
    b.wrap_v7(2, "a/b.dat", 7, 2, 0, 1 << 20, 1 << 20, Some(b"x"));
    // flagged compressed, but the bytes are not an Oodle stream
    b.wrap_v7(3, "a/b.dat", 7, 2, 0x1, 8, 16, Some(b"notoodle"));
    put(&env, b).await;
    let err = wrap(&env, 1).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err:?}");
    for guid in [2, 3] {
        let v = wrap(&env, guid).await.unwrap();
        let err = bytes(&env, &v).await.unwrap_err();
        assert!(
            matches!(
                err.kind(),
                ErrorKind::Corrupt { .. } | ErrorKind::Custom { .. }
            ),
            "{guid}: {err:?}"
        );
    }
}
