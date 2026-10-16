//! Patched paks on synthetic files: the edit stream rebuilds the pages the patch does not store.

mod common;

use common::*;
use opendal::{services::Memory, Operator};
use respak_core::{CancellationToken, Cx, DiscoverOptions, Engine, ErrorKind, Mediator, Registry};
use respak_respawn::RespawnPlugin;

const BASE_PAGE: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const STORED: &[u8] = b"STORED-PAGE-ONE-BYTES";

fn raw(guid: u64, head: (u32, u32), size: u32) -> RawAsset {
    RawAsset { guid, kind: *b"txtr", version: 8, head, data: None, header_size: size, starpak: -1, opt_starpak: -1 }
}

/// The base pak: one page, `BASE_PAGE`.
fn base() -> Vec<u8> {
    build_raw(8, &[BASE_PAGE.to_vec()], &[], &[raw(1, (0, 0), 4)])
}

/// The bytes before the page in the body of the base pak.
fn base_tables() -> u64 {
    (base().len() - 0x80 - BASE_PAGE.len()) as u64
}

/// The final first page: every command once.
fn patched_page() -> Vec<u8> {
    b"0123456789xyzQQ!DEF@@IJKLMN#PQRSTUV".to_vec()
}

/// A script that builds `patched_page` from the base page.
fn script() -> EditScript {
    let mut s = EditScript::new();
    s.skip(base_tables()) // the base pak's tables
        .copy(10) // 0123456789
        .insert(b"xyz")
        .replace(b"QQ") // skips AB
        .patched_copy(4, b"!") // skips C, copies DEF
        .patched_copy(6, b"@@") // skips GH, copies IJKLMN
        .patched_copy(5, b"#"); // skips O, copies PQRSTUV
    s
}

fn top_with(stream: Vec<u8>, patch_pages: u32, indices: Vec<u16>) -> Vec<u8> {
    let pages = [patched_page(), STORED.to_vec()];
    let assets = [raw(0xA, (0, 0), 10), raw(0xB, (1, 0), 6)];
    build_patch_file(8, &pages, &[], &assets, &PatchSpec { patch_pages, indices, stream })
}

fn top() -> Vec<u8> {
    top_with(script().finish(), 1, vec![0])
}

fn expected_mem() -> Vec<u8> {
    [patched_page(), STORED.to_vec()].concat()
}

fn cx(input: &Operator, plugin: RespawnPlugin) -> Cx {
    let mut r = Registry::new();
    r.add(&plugin);
    Cx::new(Mediator::new(r), input.clone(), input.clone())
}

async fn volume(input: &Operator, plugin: RespawnPlugin, path: &str) -> respak_core::Result<Operator> {
    let cx = cx(input, plugin);
    let pak = cx.mediator.open(path, &cx).await?;
    let pak = cx.mediator.mount(pak, &cx).await?;
    Ok(pak.as_container().unwrap().volume())
}

fn memory() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

#[tokio::test]
async fn the_patched_pages_are_rebuilt_from_the_files_below() {
    let input = memory();
    input.write("common.rpak", base()).await.unwrap();
    input.write("common(01).rpak", top()).await.unwrap();
    let v = volume(&input, RespawnPlugin::new(), "common(01).rpak").await.unwrap();

    let want = expected_mem();
    assert_eq!(v.stat(".mem").await.unwrap().content_length(), want.len() as u64);
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), want);
    // Every range, across the end of the first page and inside the stored one.
    for start in 0..want.len() {
        for end in start..=want.len() {
            let got = v.read_with(".mem").range(start as u64..end as u64).await.unwrap().to_vec();
            assert_eq!(got, &want[start..end], "{start}..{end}");
        }
    }
    // Asset headers: one in the rebuilt page, one in the stored page.
    assert_eq!(v.read("0x000000000000000A.txtr").await.unwrap().to_vec(), b"0123456789");
    assert_eq!(v.read("0x000000000000000B.txtr").await.unwrap().to_vec(), b"STORED");
}

#[tokio::test]
async fn listing_does_not_need_the_files_below_but_reading_pages_does() {
    let input = memory();
    input.write("common(01).rpak", top()).await.unwrap();
    let v = volume(&input, RespawnPlugin::new(), "common(01).rpak").await.unwrap();
    let names: Vec<_> = v.list("").await.unwrap().iter().map(|e| e.name().to_owned()).collect();
    assert_eq!(names.len(), 2);
    // The stored page needs only the patch file itself; the rebuilt one needs the base.
    let err = v.read("0x000000000000000B.txtr").await.unwrap_err();
    assert!(err.to_string().contains("missing dependency"), "{err}");
    let err = v.read("0x000000000000000A.txtr").await.unwrap_err();
    assert!(err.to_string().contains("missing dependency") && err.to_string().contains("file: common.rpak"), "{err}");
}

#[tokio::test]
async fn lower_files_are_named_after_the_top_file() {
    let input = memory();
    // A chain of two: the file at index 2 is `(02)`, the base follows it.
    let mid = build_raw(8, &[BASE_PAGE.to_vec()], &[], &[raw(1, (0, 0), 4)]);
    input.write("sub/common(02).rpak", mid).await.unwrap();
    let mut s = EditScript::new();
    s.skip(base_tables()).copy(35).copy(1);
    let top = top_with(s.finish(), 1, vec![2]);
    // The script copies the page it builds from the file at index 2.
    input.write("sub/common(03).rpak", top).await.unwrap();
    let v = volume(&input, RespawnPlugin::new(), "sub/common(03).rpak").await.unwrap();
    // 35 bytes are asked of a 36-byte page, so the first page is its start.
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), [&BASE_PAGE[..35], STORED].concat());
}

#[tokio::test]
async fn bad_edit_streams_are_corrupt() {
    let input = memory();
    input.write("common.rpak", base()).await.unwrap();
    let mut past_files = EditScript::new();
    past_files.skip(base_tables()).skip(BASE_PAGE.len() as u64).copy(10);
    let mut short = script().finish();
    short.truncate(short.len() - 12);
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("no commands", EditScript::new().finish()),
        ("past the files", past_files.finish()),
        ("cut", short),
        ("tables cut", vec![0x02; 5]),
        ("empty", Vec::new()),
    ];
    let mut cases = cases;
    // Command 7 is reached only when the script has not filled the pages yet.
    let mut early = EditScript::new();
    early.skip(base_tables()).raw_command(7);
    cases.push(("unknown command", early.finish()));
    for (name, stream) in cases {
        input.write("common(01).rpak", top_with(stream, 1, vec![0])).await.unwrap();
        let v = volume(&input, RespawnPlugin::new(), "common(01).rpak").await.unwrap();
        let err = v.read(".mem").await.unwrap_err();
        assert!(err.to_string().contains("corrupt data"), "{name}: {err}");
    }
}

#[tokio::test]
async fn a_patch_block_that_claims_too_many_pages_is_corrupt() {
    let input = memory();
    input.write("common(01).rpak", top_with(script().finish(), 5, vec![0])).await.unwrap();
    let err = volume(&input, RespawnPlugin::new(), "common(01).rpak").await.map(|_| ()).expect_err("must fail");
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

/// A `patch_master.rpak` with one entry.
fn master(entries: &[(&str, u8)]) -> Vec<u8> {
    // header (0x18), the name pointers, the versions, the strings
    let n = entries.len();
    let mut page = Vec::new();
    let names_at = 0x18u32;
    let versions_at = names_at + 8 * n as u32;
    let strings_at = (versions_at + n as u32 + 7) & !7;
    page.extend_from_slice(&0i32.to_le_bytes());
    page.extend_from_slice(&(n as i32).to_le_bytes());
    for at in [names_at, versions_at] {
        page.extend_from_slice(&0u32.to_le_bytes());
        page.extend_from_slice(&at.to_le_bytes());
    }
    let mut at = strings_at;
    let mut strings = Vec::new();
    for (name, _) in entries {
        page.extend_from_slice(&0u32.to_le_bytes());
        page.extend_from_slice(&at.to_le_bytes());
        strings.extend_from_slice(name.as_bytes());
        strings.push(0);
        at += name.len() as u32 + 1;
    }
    page.extend(entries.iter().map(|(_, v)| *v));
    page.resize(strings_at as usize, 0);
    page.extend(strings);
    let asset = RawAsset { kind: *b"Ptch", ..raw(0x6FC6, (0, 0), 0x18) };
    build_raw(8, &[page], &[], &[asset])
}

#[tokio::test]
async fn patch_master_redirects_a_base_pak_to_its_newest_patch() {
    let input = memory();
    input.write("p/common.rpak", base()).await.unwrap();
    input.write("p/common(01).rpak", top()).await.unwrap();
    input.write("p/patch_master.rpak", master(&[("other.rpak", 3), ("common.rpak", 1)])).await.unwrap();

    // Opening the base file gives the patched pages.
    let v = volume(&input, RespawnPlugin::new(), "p/common.rpak").await.unwrap();
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), expected_mem());
    // The patch file opens as itself, and the master is not asked about it.
    let v = volume(&input, RespawnPlugin::new(), "p/common(01).rpak").await.unwrap();
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), expected_mem());
}

#[tokio::test]
async fn patch_master_entries_that_do_not_apply_leave_the_file_alone() {
    let input = memory();
    input.write("common.rpak", base()).await.unwrap();
    // The master names no patch for it, or a patch number of 0, or a patch that is not there.
    for entries in [vec![("other.rpak", 1)], vec![("common.rpak", 0)], vec![("common.rpak", 2)]] {
        input.write("patch_master.rpak", master(&entries)).await.unwrap();
        let v = volume(&input, RespawnPlugin::new(), "common.rpak").await.unwrap();
        assert_eq!(v.read(".mem").await.unwrap().to_vec(), BASE_PAGE, "{entries:?}");
    }
    // A master that is not a pak is ignored.
    input.write("patch_master.rpak", b"junk".to_vec()).await.unwrap();
    let v = volume(&input, RespawnPlugin::new(), "common.rpak").await.unwrap();
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), BASE_PAGE);
}

#[tokio::test]
async fn ignoring_patches_opens_the_base_and_refuses_a_patch_file() {
    let input = memory();
    input.write("common.rpak", base()).await.unwrap();
    input.write("common(01).rpak", top()).await.unwrap();
    input.write("patch_master.rpak", master(&[("common.rpak", 1)])).await.unwrap();

    let off = || RespawnPlugin::new().ignore_patches(true);
    let v = volume(&input, off(), "common.rpak").await.unwrap();
    assert_eq!(v.read(".mem").await.unwrap().to_vec(), BASE_PAGE);

    let err = volume(&input, off(), "common(01).rpak").await.map(|_| ()).expect_err("must fail");
    match err.kind() {
        ErrorKind::UnsupportedVersion { format, version } => {
            assert_eq!(format, "rpak");
            assert!(version.contains("patch"), "{version}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_patched_pak_is_discovered_like_any_other() {
    let input = memory();
    input.write("common.rpak", base()).await.unwrap();
    input.write("common(01).rpak", top()).await.unwrap();
    let engine = Engine::builder().input(input.clone()).output(memory()).plugin(RespawnPlugin::new()).build().unwrap();
    let d = engine.discover("common(01).rpak", &DiscoverOptions { depth: 1, ..Default::default() }, &CancellationToken::new()).await.unwrap();
    assert_eq!(d.entries.len(), 2);
}
