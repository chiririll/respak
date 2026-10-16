//! `discover` and `formats` on synthetic PNG and DDS files.

use ddsfile::{D3DFormat, Dds, NewD3dParams};
use opendal::{services::Memory, Operator};
use respak_core::{ConvertRequest, DiscoverOptions, Engine, OutputResult};
use respak_formats::FormatsPlugin;

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

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 2, 2);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&[7; 16])
        .unwrap();
    out
}

fn dds() -> Vec<u8> {
    let mut d = Dds::new_d3d(NewD3dParams {
        height: 2,
        width: 2,
        depth: None,
        format: D3DFormat::A8B8G8R8,
        mipmap_levels: None,
        caps2: None,
    })
    .unwrap();
    d.data = vec![9; 16];
    let mut out = Vec::new();
    d.write(&mut out).unwrap();
    out
}

fn target_names(e: &respak_core::DiscoveryEntry) -> Vec<&str> {
    e.targets.iter().map(|t| t.format.as_str()).collect()
}

#[tokio::test]
async fn discover_a_png_and_a_dds() {
    let (engine, input, _) = env();
    input.write("icon.png", png()).await.unwrap();
    input.write("icon.dds", dds()).await.unwrap();

    let d = engine
        .discover(
            "icon.png",
            &DiscoverOptions::default(),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    let e = &d.entries[0];
    assert_eq!((e.format.as_deref(), e.score), (Some("png"), 13));
    assert!(e.alternatives.is_empty());
    assert_eq!(target_names(e), ["dds"]);

    let d = engine
        .discover(
            "icon.dds",
            &DiscoverOptions::default(),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    let e = &d.entries[0];
    assert_eq!((e.format.as_deref(), e.score), (Some("dds"), 13));
    assert_eq!(target_names(e), ["png"]);
}

#[tokio::test]
async fn a_dds_named_png_is_discovered_as_dds() {
    let (engine, input, _) = env();
    input.write("fake.png", dds()).await.unwrap();
    let e = &engine
        .discover(
            "fake.png",
            &DiscoverOptions::default(),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap()
        .entries[0];
    // No extension bonus: the `.png` candidate did not match, so every edge was scored.
    assert_eq!((e.format.as_deref(), e.score), (Some("dds"), 12));
    assert!(e.alternatives.is_empty());
}

#[tokio::test]
async fn every_listed_target_converts() {
    let (engine, input, output) = env();
    input.write("icon.png", png()).await.unwrap();
    let e = engine
        .discover(
            "icon.png",
            &DiscoverOptions::default(),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap()
        .entries
        .remove(0);
    assert!(!e.targets.is_empty());
    for t in &e.targets {
        let json = serde_json::json!({
            "version": 1,
            "outputs": [{ "from": e.path, "to": { "type": t.format, "via": [] }, "path": format!("out.{}", t.format) }]
        });
        let request: ConvertRequest = serde_json::from_value(json).unwrap();
        let report = engine
            .convert(&request, &respak_core::CancellationToken::new())
            .await
            .unwrap();
        assert!(
            matches!(report.outputs[0].result, OutputResult::Ok { .. }),
            "{}",
            t.format
        );
        assert!(output.exists(&format!("out.{}", t.format)).await.unwrap());
    }
}

#[tokio::test]
async fn formats_describes_png_and_dds() {
    let (engine, ..) = env();
    let json = serde_json::to_value(engine.formats()).unwrap();
    let find = |n: &str| {
        json["formats"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == n)
            .unwrap()
            .clone()
    };
    let (png, dds) = (find("png"), find("dds"));
    assert_eq!(png["extensions"], serde_json::json!(["png"]));
    assert_eq!(png["provides"], serde_json::json!(["Bitmap"]));
    assert_eq!(
        png["produced_by"],
        serde_json::json!([{ "kind": "capability", "from": "Bitmap", "cost": 2 }])
    );
    assert_eq!(
        dds["produced_by"],
        serde_json::json!([{ "kind": "capability", "from": "Bitmap", "cost": 1 }])
    );
    assert_eq!(dds["plugin"], "formats");
}
