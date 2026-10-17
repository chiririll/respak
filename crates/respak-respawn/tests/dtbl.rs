//! `dtbl` on synthetic paks: layouts, cell types, stripped strings, errors, CSV / JSON output.

mod common;

use common::*;
use respak_core::capability::{read_all, Cell, ColumnType};
use respak_core::{Cx, ErrorKind, FormatId, Registry, Target, Value};
use respak_respawn::{RespawnPlugin, DATA_EXCLUDED};

/// A pak date inside the V1.1 window (2022).
const V1_1_TIME: u64 = 0x1d8_0000_0000_0000;
const GUID: u64 = 0xD7B1;
const PATH: &str = "c.rpak/0x000000000000D7B1.dtbl";

fn cx(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&RespawnPlugin::new());
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

async fn write(env: &Env, pak: PakBuilder, created: u64) {
    let mut bytes = pak.build(8);
    set_created_time(&mut bytes, created);
    env.input.write("c.rpak", bytes).await.unwrap();
}

async fn table(env: &Env) -> respak_core::Result<Value> {
    let cx = cx(env);
    let asset = cx.mediator.open(PATH, &cx).await?;
    cx.mediator.identify(asset, &cx).await
}

async fn convert(env: &Env, format: &'static str) -> String {
    let cx = cx(env);
    let value = table(env).await.unwrap();
    let target = Target {
        format: FormatId::new(format),
        options: serde_json::Value::Null,
        via: Vec::new(),
    };
    let out = cx.mediator.convert(value, target, &cx).await.unwrap();
    String::from_utf8(
        read_all(out.as_blob().unwrap(), &cx)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn all_types() -> Vec<(&'static str, i32)> {
    vec![
        ("b", 0),
        ("i", 1),
        ("f", 2),
        ("v", 3),
        ("s", 4),
        ("a", 5),
        ("n", 6),
    ]
}

fn sample_rows() -> Vec<Vec<DtblCell<'static>>> {
    vec![
        vec![
            DtblCell::Bool(true),
            DtblCell::Int(-3),
            DtblCell::Float(1.5),
            DtblCell::Vector([1.0, 2.0, 3.25]),
            DtblCell::Str("hello"),
            DtblCell::Str("mdl/a.rmdl"),
            DtblCell::Str("\x0fstripped"),
        ],
        vec![
            DtblCell::Bool(false),
            DtblCell::Int(7),
            DtblCell::Float(0.1),
            DtblCell::Vector([0.0, -1.0, 0.5]),
            DtblCell::Str(""),
            DtblCell::Null,
            DtblCell::Str("x"),
        ],
    ]
}

#[tokio::test]
async fn every_layout_reads_the_same_table() {
    for (version, wide, created, want) in [
        (0, false, 0, "dtbl@0"),
        (1, false, 0, "dtbl@1"),
        (1, true, V1_1_TIME, "dtbl@1.1"),
    ] {
        let env = common::env();
        let mut pak = PakBuilder::new();
        pak.dtbl(GUID, version, wide, &all_types(), &sample_rows());
        write(&env, pak, created).await;

        let v = table(&env).await.unwrap();
        assert_eq!(v.format().to_string(), want);
        let t = v.as_table().unwrap();
        let types: Vec<ColumnType> = t.columns().iter().map(|c| c.ty).collect();
        assert_eq!(t.columns()[0].name, "b");
        assert_eq!(types[6], ColumnType::AssetNoPrecache);
        assert_eq!(
            t.rows()[0],
            vec![
                Cell::Bool(true),
                Cell::Int(-3),
                Cell::Float(1.5),
                Cell::Vector([1.0, 2.0, 3.25]),
                Cell::String("hello".into()),
                Cell::String("mdl/a.rmdl".into()),
                Cell::String(DATA_EXCLUDED.into()),
            ],
            "{want}"
        );
        assert_eq!(t.rows()[1][4], Cell::String(String::new()));
        assert_eq!(t.rows()[1][5], Cell::String(String::new()));
        assert_eq!(v.as_metadata().unwrap().guid(), Some(GUID));
    }
}

#[tokio::test]
async fn csv_is_rsx_text() {
    let env = common::env();
    let mut pak = PakBuilder::new();
    pak.dtbl(GUID, 1, false, &all_types(), &sample_rows());
    write(&env, pak, 0).await;
    let want = "\"b\",\"i\",\"f\",\"v\",\"s\",\"a\",\"n\"\n\
        true,-3,1.5,\"<1,2,3.25>\",\"hello\",\"mdl/a.rmdl\",\"!!DATA EXCLUDED!!\"\n\
        false,7,0.1,\"<0,-1,0.5>\",\"\",\"\",\"x\"\n\
        \"bool\",\"int\",\"float\",\"vector\",\"string\",\"asset\",\"asset_noprecache\"";
    assert_eq!(convert(&env, "dtbl.csv").await, want);
}

#[tokio::test]
async fn json_lists_columns_and_rows() {
    let env = common::env();
    let mut pak = PakBuilder::new();
    pak.dtbl(
        GUID,
        0,
        false,
        &[("name", 4), ("speed", 2)],
        &[vec![DtblCell::Str("r99"), DtblCell::Float(0.1)]],
    );
    write(&env, pak, 0).await;
    let json: serde_json::Value = serde_json::from_str(&convert(&env, "dtbl.json").await).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "columns": [{"name": "name", "type": "string"}, {"name": "speed", "type": "float"}],
            "rows": [["r99", 0.1]],
        })
    );
}

#[tokio::test]
async fn a_table_without_rows_is_read() {
    let env = common::env();
    let mut pak = PakBuilder::new();
    pak.dtbl(GUID, 1, false, &[("only", 1)], &[]);
    write(&env, pak, 0).await;
    assert_eq!(convert(&env, "dtbl.csv").await, "\"only\"\n\"int\"");
}

#[tokio::test]
async fn unknown_versions_are_unsupported() {
    let env = common::env();
    let mut pak = PakBuilder::new();
    pak.asset(GUID, b"dtbl", 2, vec![0; 0x28], &[], None);
    write(&env, pak, 0).await;
    let e = table(&env).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn bad_headers_and_pointers_are_corrupt() {
    // A V0 header size under asset version 1.
    let env = common::env();
    let mut pak = PakBuilder::new();
    pak.asset(GUID, b"dtbl", 1, vec![0; 0x20], &[], None);
    write(&env, pak, 0).await;
    assert!(matches!(
        table(&env).await.unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));

    // More rows than the pak holds.
    let env = common::env();
    let mut pak = PakBuilder::new();
    let rows = pak.data(&[0; 16]);
    let cols = pak.data(&[0; 16]);
    let mut h = vec![0u8; 0x20];
    h[0..4].copy_from_slice(&0i32.to_le_bytes());
    h[4..8].copy_from_slice(&1_000_000i32.to_le_bytes());
    h[0x18..0x1C].copy_from_slice(&64i32.to_le_bytes());
    pak.asset(GUID, b"dtbl", 0, h, &[(0x08, cols), (0x10, rows)], None);
    write(&env, pak, 0).await;
    assert!(matches!(
        table(&env).await.unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));

    // A column past the row stride, and one of an unknown type.
    for (ty, offset) in [(1i32, 2i32), (9, 0)] {
        let env = common::env();
        let mut pak = PakBuilder::new();
        let mut record = [0u8; 16];
        record[8..12].copy_from_slice(&ty.to_le_bytes());
        record[12..16].copy_from_slice(&offset.to_le_bytes());
        let cols = pak.data(&record);
        let rows = pak.data(&[0; 4]);
        let mut h = vec![0u8; 0x20];
        h[0..4].copy_from_slice(&1i32.to_le_bytes());
        h[4..8].copy_from_slice(&1i32.to_le_bytes());
        h[0x18..0x1C].copy_from_slice(&4i32.to_le_bytes());
        pak.asset(GUID, b"dtbl", 0, h, &[(0x08, cols), (0x10, rows)], None);
        write(&env, pak, 0).await;
        assert!(
            matches!(
                table(&env).await.unwrap_err().kind(),
                ErrorKind::Corrupt { .. }
            ),
            "type {ty}"
        );
    }
}
