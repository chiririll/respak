//! `uimg` on synthetic paks: both versions, names, the atlas bitmap, `uimg.json` and sprite cutting.

mod common;

use bytes::Bytes;
use common::*;
use respak_core::capability::{read_all, PixelFormat};
use respak_core::value::BundleData;
use respak_core::{Cx, ErrorKind, Event, FormatId, Mediator, Registry, Target, Value};
use respak_respawn::RespawnPlugin;
use serde_json::{json, Value as Json};

const RGBA8: u16 = 31;
const ATLAS: u64 = 0xA71A5;
const UIMG: u64 = 0x1111;
const PATH: &str = "c.rpak/0x0000000000001111.uimg";

fn cx(env: &Env) -> Cx {
    let mut r = Registry::new();
    r.add(&respak_formats::FormatsPlugin);
    r.add(&RespawnPlugin::new());
    Cx::new(Mediator::new(r), env.input.clone(), env.input.clone())
}

/// An 8x4 atlas whose pixel (x, y) is `[x, y, 7, 255]`.
fn atlas(guid: u64) -> TexSpec {
    let data = (0..4u8)
        .flat_map(|y| (0..8u8).flat_map(move |x| [x, y, 7, 255]))
        .collect();
    TexSpec::new(guid, 9, 8, 4, RGBA8, 1, data)
}

fn sprite(path: Option<&'static str>, hash: u32, px: [u32; 4]) -> SpriteSpec {
    let [x, y, w, h] = px.map(|v| v as f32);
    SpriteSpec {
        path,
        hash,
        uv: [x / 8.0, y / 4.0, w / 8.0, h / 4.0],
        layout: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
        render: (w as u16, h as u16),
    }
}

fn spec(version: u32, names: bool, sprites: Vec<SpriteSpec>) -> UimgSpec {
    UimgSpec {
        guid: UIMG,
        version,
        width: 8,
        height: 4,
        atlas_guid: ATLAS,
        sprites,
        names,
    }
}

async fn write(env: &Env, textures: &[TexSpec], atlases: &[UimgSpec]) {
    env.input
        .write("c.rpak", build_uimg_pak(8, textures, atlases))
        .await
        .unwrap();
}

async fn read(cx: &Cx) -> respak_core::Result<Value> {
    let asset = cx.mediator.open(PATH, cx).await?;
    cx.mediator.identify(asset, cx).await
}

/// Converts to `uimg.json` and returns the bundle's files by name, with the warnings raised.
async fn json_bundle(env: &Env, options: Json) -> (Vec<(String, Bytes)>, Vec<String>) {
    let cx = cx(env);
    let mut events = cx.mediator.subscribe();
    let value = read(&cx).await.unwrap();
    let target = Target {
        format: FormatId::new("uimg.json"),
        options,
        via: Vec::new(),
    };
    let out = cx.mediator.convert(value, target, &cx).await.unwrap();
    let mut warnings = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let Event::Warning { message } = e {
            warnings.push(message);
        }
    }
    let bundle = out.downcast_ref::<BundleData>().unwrap();
    let mut files = Vec::new();
    for (n, item) in bundle.names.iter().zip(&bundle.items) {
        files.push((
            n.clone(),
            read_all(item.as_blob().unwrap(), &cx).await.unwrap(),
        ));
    }
    (files, warnings)
}

fn file<'a>(files: &'a [(String, Bytes)], name: &str) -> &'a Bytes {
    &files
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| {
            panic!(
                "no {name} in {:?}",
                files.iter().map(|f| &f.0).collect::<Vec<_>>()
            )
        })
        .1
}

#[tokio::test]
async fn both_versions_are_read_with_their_hash_stride() {
    for version in [10, 11] {
        let env = env();
        let sprites = vec![
            sprite(Some("rui/a"), 0xAAAA, [2, 1, 3, 2]),
            sprite(Some("rui/b"), 0xBBBB, [0, 0, 8, 4]),
        ];
        write(&env, &[atlas(ATLAS)], &[spec(version, true, sprites)]).await;
        let cx = cx(&env);
        let v = read(&cx).await.unwrap();
        assert_eq!(v.format().to_string(), format!("uimg@{version}"));
        let m = v.as_metadata().unwrap();
        assert_eq!(
            (m.kind(), m.guid(), m.name()),
            (Some("uimg"), Some(UIMG), None)
        );
        let deps = m.dependencies();
        assert_eq!((deps.len(), deps[0].guid), (1, ATLAS));
        let info = v.as_bitmap().unwrap().info();
        assert_eq!(
            (info.width, info.height, info.format),
            (8, 4, PixelFormat::Rgba8)
        );
    }
}

#[tokio::test]
async fn an_unsupported_version_is_refused() {
    let env = env();
    write(
        &env,
        &[atlas(ATLAS)],
        &[spec(12, true, vec![sprite(Some("a"), 1, [0, 0, 1, 1])])],
    )
    .await;
    let cx = cx(&env);
    let e = read(&cx).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn the_bitmap_is_the_atlas_texture() {
    let env = env();
    write(
        &env,
        &[atlas(ATLAS)],
        &[spec(11, true, vec![sprite(Some("a"), 1, [0, 0, 1, 1])])],
    )
    .await;
    let cx = cx(&env);
    let v = read(&cx).await.unwrap();
    let px = v.as_bitmap().unwrap().read_mip(0, 0).await.unwrap();
    assert_eq!(px.len(), 8 * 4 * 4);
    assert_eq!(&px[(3 * 8 + 5) * 4..][..4], [5, 3, 7, 255]);
}

#[tokio::test]
async fn a_missing_atlas_fails_the_bitmap_but_not_the_json() {
    let env = env();
    write(
        &env,
        &[],
        &[spec(10, true, vec![sprite(Some("a"), 1, [0, 0, 1, 1])])],
    )
    .await;
    let cx = cx(&env);
    let v = read(&cx).await.unwrap();
    let e = v.as_bitmap().unwrap().read_mip(0, 0).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );

    let (files, warnings) = json_bundle(&env, json!({ "sprites": true })).await;
    let doc: Json = serde_json::from_slice(file(&files, "uimg.json")).unwrap();
    assert_eq!(
        doc["atlas"],
        json!({ "guid": "0x00000000000A71A5", "file": null })
    );
    assert_eq!(doc["sprites"][0]["file"], Json::Null);
    assert_eq!(files.len(), 1);
    assert!(warnings.iter().any(|w| w.contains("atlas")), "{warnings:?}");
}

#[tokio::test]
async fn the_json_lists_sprites_in_stored_order() {
    let env = env();
    let sprites = vec![
        sprite(Some("rui/wide"), 0xAAAA, [0, 0, 8, 4]),
        sprite(Some("rui/small"), 0xBBBB, [2, 1, 3, 2]),
        sprite(None, 0xCCCC, [1, 1, 1, 1]),
    ];
    write(&env, &[atlas(ATLAS)], &[spec(11, true, sprites)]).await;
    let (files, warnings) = json_bundle(&env, json!({})).await;
    assert!(
        warnings.iter().any(|w| w.contains("not readable")),
        "{warnings:?}"
    );
    assert!(
        files
            .iter()
            .all(|(n, _)| n == "uimg.json" || n == "atlas.png"),
        "{:?}",
        files.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    let doc: Json = serde_json::from_slice(file(&files, "uimg.json")).unwrap();
    assert_eq!(
        (doc["version"].clone(), doc["guid"].clone()),
        (json!(11), json!("0x0000000000001111"))
    );
    assert_eq!(
        doc["atlas"],
        json!({ "guid": "0x00000000000A71A5", "file": "atlas.png" })
    );
    let s = doc["sprites"].as_array().unwrap();
    assert_eq!(s.len(), 3);
    assert_eq!(s[1]["index"], 1);
    assert_eq!(s[1]["path"], "rui/small");
    assert_eq!(s[1]["pathHash"], "0x0000BBBB");
    assert_eq!(
        (
            s[1]["x"].clone(),
            s[1]["y"].clone(),
            s[1]["width"].clone(),
            s[1]["height"].clone()
        ),
        (json!(2), json!(1), json!(3), json!(2))
    );
    assert_eq!(s[1]["renderSize"], json!({ "width": 3, "height": 2 }));
    assert_eq!(s[1]["layout"]["endAnchorX"], 1.0);
    // A sprite whose name is empty is called after its hash.
    assert_eq!(s[2]["path"], "0x0000CCCC");
    // Sprites are not cut unless asked.
    assert!(s.iter().all(|s| s["file"].is_null()));
}

#[tokio::test]
async fn without_names_the_sprites_are_called_after_their_hash() {
    let env = env();
    write(
        &env,
        &[atlas(ATLAS)],
        &[spec(
            10,
            false,
            vec![sprite(Some("ignored"), 0xBEEF, [0, 0, 1, 1])],
        )],
    )
    .await;
    let (files, _) = json_bundle(&env, json!({})).await;
    let doc: Json = serde_json::from_slice(file(&files, "uimg.json")).unwrap();
    assert_eq!(doc["sprites"][0]["path"], "0x0000BEEF");
}

#[tokio::test]
async fn sprites_are_cut_from_the_atlas() {
    let env = env();
    let sprites = vec![
        sprite(Some("rui/a"), 1, [2, 1, 3, 2]),
        sprite(Some("../../escape"), 2, [0, 0, 1, 1]),
        sprite(Some("rui/empty"), 3, [0, 0, 0, 0]),
        sprite(Some("rui/a"), 4, [7, 3, 1, 1]),
    ];
    write(&env, &[atlas(ATLAS)], &[spec(11, true, sprites)]).await;
    let (files, warnings) =
        json_bundle(&env, json!({ "sprites": true, "image": { "type": "dds" } })).await;
    let doc: Json = serde_json::from_slice(file(&files, "uimg.json")).unwrap();
    let s = doc["sprites"].as_array().unwrap();
    assert_eq!(s[0]["file"], "sprites/rui/a.dds", "{warnings:?}");
    // `..` cannot leave the folder.
    assert_eq!(s[1]["file"], "sprites/escape.dds");
    // An empty sprite is skipped with a warning, a repeated path gets its index.
    assert_eq!(s[2]["file"], Json::Null);
    assert_eq!(s[3]["file"], "sprites/rui/a_3.dds");
    assert!(
        warnings.iter().any(|w| w.contains("rui/empty")),
        "{warnings:?}"
    );

    // The pixels of the first sprite are the last bytes of its image: 3x2 pixels at (2, 1).
    let dds = file(&files, "sprites/rui/a.dds");
    let want: Vec<u8> = [(2u8, 1u8), (3, 1), (4, 1), (2, 2), (3, 2), (4, 2)]
        .iter()
        .flat_map(|&(x, y)| [x, y, 7, 255])
        .collect();
    assert_eq!(&dds[dds.len() - want.len()..], &want[..]);
}
