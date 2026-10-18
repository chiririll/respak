//! `subt` on synthetic paks: lines, colour tags, versions, bounds, the language, text targets.

mod common;

use common::*;
use respak_core::capability::{read_all, Cell};
use respak_core::{Cx, ErrorKind, FormatId, Registry, Target, Value};
use respak_respawn::{string_to_guid, RespawnPlugin, SubtAsset};

const ENGLISH: &str = "paks/subtitles_english.rpak";

fn guid_english() -> u64 {
    string_to_guid("subtitles/subtitles_english.rpak")
}

fn cx(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&RespawnPlugin::new());
    r.add(&respak_formats::FormatsPlugin);
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

async fn put(env: &Env, path: &str, pak: PakBuilder) {
    env.input.write(path, pak.build(8)).await.unwrap();
}

async fn subt(env: &Env, path: &str, guid: u64) -> respak_core::Result<Value> {
    let cx = cx(env);
    let asset = cx
        .mediator
        .open(&format!("{path}/0x{guid:016X}.subt"), &cx)
        .await?;
    cx.mediator.identify(asset, &cx).await
}

fn rows(v: &Value) -> Vec<Vec<Cell>> {
    v.as_table().unwrap().rows().to_vec()
}

fn text(s: &str) -> Cell {
    Cell::String(s.into())
}

fn language(v: &Value) -> Option<String> {
    v.downcast_ref::<SubtAsset>()
        .unwrap()
        .language()
        .map(str::to_owned)
}

#[tokio::test]
async fn lines_keep_table_order_and_empty_slots_are_skipped() {
    let env = env();
    let mut b = PakBuilder::new();
    b.subt(
        guid_english(),
        1,
        &[
            (0xAAAA_0001, "<clr:255,0,10>Hi"),
            (0, ""),
            (0xAAAA_0002, "Plain"),
        ],
    );
    put(&env, ENGLISH, b).await;
    let v = subt(&env, ENGLISH, guid_english()).await.unwrap();
    let t = v.as_table().unwrap();
    let names: Vec<_> = t.columns().iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["hash", "color", "subtitle"]);
    assert_eq!(
        rows(&v),
        vec![
            vec![
                text("0xaaaa0001"),
                Cell::Vector([255.0, 0.0, 10.0]),
                text("Hi")
            ],
            vec![
                text("0xaaaa0002"),
                Cell::Vector([255.0, 255.0, 255.0]),
                text("Plain")
            ],
        ]
    );
    let meta = v.as_metadata().unwrap();
    assert_eq!(meta.kind(), Some("subt"));
    assert_eq!(meta.guid(), Some(guid_english()));
}

#[tokio::test]
async fn version_0_and_1_read_the_same_and_others_are_unsupported() {
    let env = env();
    for (guid, version) in [(1u64, 0u32), (2, 1), (3, 2)] {
        let mut b = PakBuilder::new();
        b.subt(guid, version, &[(7, "x")]);
        put(&env, &format!("paks/v{version}.rpak"), b).await;
    }
    let a = subt(&env, "paks/v0.rpak", 1).await.unwrap();
    let b = subt(&env, "paks/v1.rpak", 2).await.unwrap();
    assert_eq!(rows(&a), rows(&b));
    let err = subt(&env, "paks/v2.rpak", 3).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }));
}

#[tokio::test]
async fn an_empty_table_is_empty() {
    let env = env();
    let mut b = PakBuilder::new();
    b.subt_raw(1, 1, 0, 0, None, None);
    put(&env, "paks/e.rpak", b).await;
    assert!(rows(&subt(&env, "paks/e.rpak", 1).await.unwrap()).is_empty());
}

#[tokio::test]
async fn bad_pointers_and_offsets_are_corrupt() {
    let env = env();
    let entry = |hash: u32, offset: i32| [hash.to_le_bytes(), offset.to_le_bytes()].concat();
    #[allow(clippy::type_complexity)]
    let cases: [(u64, i32, Option<Vec<u8>>, Option<&[u8]>); 5] = [
        (1, 1, None, Some(b"a\0")),                    // null entries, count > 0
        (2, 1, Some(entry(5, 0)), None),               // null strings, a line
        (3, 1, Some(entry(5, 1000)), Some(b"a\0")),    // offset past the memory
        (4, 1, Some(entry(5, -1)), Some(b"a\0")),      // negative offset
        (5, 1 << 28, Some(entry(5, 0)), Some(b"a\0")), // more slots than memory
    ];
    for (guid, count, entries, strings) in cases {
        let mut b = PakBuilder::new();
        b.subt_raw(guid, 1, 1, count, entries.as_deref(), strings);
        let path = format!("paks/bad{guid}.rpak");
        put(&env, &path, b).await;
        let err = subt(&env, &path, guid).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::Corrupt { .. }),
            "case {guid}: {err}"
        );
    }
}

#[tokio::test]
async fn an_unterminated_string_is_corrupt() {
    let env = env();
    let mut b = PakBuilder::new();
    let entry = [5u32.to_le_bytes(), 0i32.to_le_bytes()].concat();
    // No NUL anywhere in the block, which ends the pak's memory.
    b.subt_raw(1, 1, 1, 1, Some(&entry), Some(&[b'a'; 16]));
    put(&env, "paks/u.rpak", b).await;
    let err = subt(&env, "paks/u.rpak", 1).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
}

#[tokio::test]
async fn malformed_tags_stay_in_the_text_and_are_white() {
    let env = env();
    let bad = [
        "<clr:1,2>x",
        "<clr:1,2,3 x",
        "<clr:300,0,0>x",
        "<clr:a,b,c>x",
    ];
    let mut b = PakBuilder::new();
    let lines: Vec<(u32, &str)> = bad.iter().zip(1u32..).map(|(s, i)| (i, *s)).collect();
    b.subt(1, 1, &lines);
    put(&env, "paks/m.rpak", b).await;
    let v = subt(&env, "paks/m.rpak", 1).await.unwrap();
    for (row, original) in rows(&v).iter().zip(bad) {
        assert_eq!(row[1], Cell::Vector([255.0; 3]), "{original}");
        assert_eq!(row[2], text(original));
    }
}

#[tokio::test]
async fn text_may_be_non_ascii_and_invalid_utf8_is_replaced() {
    let env = env();
    let mut b = PakBuilder::new();
    b.subt(1, 1, &[(1, "<clr:0,0,0>Привет, 世界")]);
    put(&env, "paks/n.rpak", b).await;
    let v = subt(&env, "paks/n.rpak", 1).await.unwrap();
    assert_eq!(rows(&v)[0][2], text("Привет, 世界"));

    let mut b = PakBuilder::new();
    let entry = [9u32.to_le_bytes(), 0i32.to_le_bytes()].concat();
    b.subt_raw(2, 1, 1, 1, Some(&entry), Some(b"a\xFFb\0"));
    put(&env, "paks/i.rpak", b).await;
    let v = subt(&env, "paks/i.rpak", 2).await.unwrap();
    assert_eq!(rows(&v)[0][2], text("a\u{FFFD}b"));
}

#[tokio::test]
async fn the_language_needs_the_pak_name_to_hash_to_the_guid() {
    let env = env();
    for (path, guid) in [
        (ENGLISH, guid_english()),
        ("paks/subtitles_french.rpak", guid_english()),
        ("paks/other.rpak", 5),
    ] {
        let mut b = PakBuilder::new();
        b.subt(guid, 1, &[(1, "x")]);
        put(&env, path, b).await;
    }
    let v = subt(&env, ENGLISH, guid_english()).await.unwrap();
    assert_eq!(language(&v).as_deref(), Some("english"));
    let v = subt(&env, "paks/subtitles_french.rpak", guid_english())
        .await
        .unwrap();
    assert_eq!(language(&v), None);
    let v = subt(&env, "paks/other.rpak", 5).await.unwrap();
    assert_eq!(language(&v), None);
}

async fn convert(env: &Env, path: &str, guid: u64, to: &'static str) -> String {
    let cx = cx(env);
    let source = subt(env, path, guid).await.unwrap();
    let target = Target {
        format: FormatId::new(to),
        options: serde_json::Value::Null,
        via: Vec::new(),
    };
    let out = cx.mediator.convert(source, target, &cx).await.unwrap();
    String::from_utf8(
        read_all(out.as_blob().unwrap(), &cx)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn txt_has_one_row_per_line_and_escapes_newlines() {
    let env = env();
    let mut b = PakBuilder::new();
    b.subt(1, 1, &[(1, "<clr:1,2,3>one"), (0, ""), (2, "two\nlines")]);
    put(&env, "paks/t.rpak", b).await;
    assert_eq!(
        convert(&env, "paks/t.rpak", 1, "subt.txt").await,
        "one\ntwo\\nlines\n"
    );
}

#[tokio::test]
async fn table_csv_and_json_come_from_the_table() {
    let env = env();
    let mut b = PakBuilder::new();
    b.subt(1, 1, &[(1, "<clr:1,2,3>say \"hi\", ok"), (2, "a\nb")]);
    put(&env, "paks/c.rpak", b).await;
    assert_eq!(
        convert(&env, "paks/c.rpak", 1, "table.csv").await,
        "hash,color,subtitle\n0x00000001,1;2;3,\"say \"\"hi\"\", ok\"\n0x00000002,255;255;255,\"a\nb\"\n"
    );
    let json: serde_json::Value =
        serde_json::from_str(&convert(&env, "paks/c.rpak", 1, "table.json").await).unwrap();
    assert_eq!(json["columns"][1]["type"], "vector");
    assert_eq!(json["rows"][0][0], "0x00000001");
    assert_eq!(json["rows"][0][1], serde_json::json!([1.0, 2.0, 3.0]));
    assert_eq!(json["rows"][1][2], "a\nb");
}
