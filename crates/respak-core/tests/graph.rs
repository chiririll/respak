//! Detection helpers, path planning, `formats`, `discover` and multi-hop conversion.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use common::*;
use respak_core::capability::{Bitmap, Blob};
use respak_core::value::{BlobData, FileData};
use respak_core::{
    u16le, u32le, within, ConvertHandler, Cx, DiscoverOptions, ErrorKind, FormatId, Mediator, Registry, Result,
    Target, Value, ValueKey,
};

/// Appends its tag to the bytes of the previous step, so the output shows the route taken.
struct Step(&'static str, &'static str);

#[async_trait]
impl ConvertHandler for Step {
    async fn convert(&self, source: Value, _target: &Target, _cx: &Cx) -> Result<Value> {
        let mut bytes = source.downcast_ref::<BlobData>().map(|b| b.bytes().to_vec()).unwrap_or_default();
        if !bytes.is_empty() {
            bytes.push(b'>');
        }
        bytes.extend_from_slice(self.1.as_bytes());
        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new(self.0), &serde_json::Value::Null);
        Ok(Arc::new(BlobData::with_format(FormatId::new(self.0), Bytes::from(bytes), key)))
    }
}

fn mediator(register: impl FnOnce(&mut Registry)) -> Mediator {
    let mut r = Registry::new();
    register(&mut r);
    r.check().unwrap();
    Mediator::new(r)
}

fn route(m: &Mediator, to: &str, via: &[&str]) -> Result<String> {
    let via: Vec<String> = via.iter().map(|s| s.to_string()).collect();
    m.plan(&FormatId::new("src"), to, &via).map(|p| p.to_string())
}

// ---- helpers ----------------------------------------------------------------------------

#[test]
fn little_endian_readers_and_within() {
    let h = [1, 2, 3, 4, 5];
    assert_eq!(u16le(&h, 0), Some(0x0201));
    assert_eq!(u32le(&h, 1), Some(0x0504_0302));
    assert_eq!(u32le(&h, 2), None, "runs past the end");
    assert_eq!(u16le(&h, 4), None);
    assert_eq!(u16le(&h, usize::MAX), None);
    assert!(within(Some(5), 1..=5));
    assert!(!within(Some(6), 1..=5));
    assert!(!within(None::<u32>, 0..=u32::MAX));
}

// ---- planning ---------------------------------------------------------------------------

#[test]
fn the_cheapest_path_wins() {
    let m = mediator(|r| {
        r.format("src").provides::<dyn Bitmap>().to("mid", Step("mid", "m")).cost(1);
        r.format("mid").to("dst", Step("dst", "d")).cost(1);
        r.format("dst").from_cap::<dyn Bitmap>(Step("dst", "direct-cap")).cost(5);
    });
    assert_eq!(route(&m, "dst", &[]).unwrap(), "src => mid => dst");
}

#[test]
fn equal_cost_prefers_direct_then_fewer_hops_then_registration_order() {
    // direct before capability, although the capability edge was registered first
    let m = mediator(|r| {
        r.format("dst").from_cap::<dyn Bitmap>(Step("dst", "cap")).cost(2);
        r.format("src").provides::<dyn Bitmap>().to("dst", Step("dst", "direct")).cost(2);
    });
    assert_eq!(route(&m, "dst", &[]).unwrap(), "src => dst");

    // fewer hops: one edge of cost 2 beats two edges of cost 1
    let m = mediator(|r| {
        r.format("src").to("mid", Step("mid", "a")).to("dst", Step("dst", "b")).cost(2);
        r.format("mid").to("dst", Step("dst", "c"));
        r.format("dst");
    });
    assert_eq!(route(&m, "dst", &[]).unwrap(), "src => dst");

    // registration order: two identical edges, the first wins
    let m = mediator(|r| {
        r.format("src").to("dst", Step("dst", "first")).to("dst", Step("dst", "second"));
        r.format("dst");
    });
    let path = m.plan(&FormatId::new("src"), "dst", &[]).unwrap();
    assert_eq!(path.steps.len(), 1);
}

#[test]
fn via_forces_a_longer_path_and_unsatisfiable_via_is_no_path() {
    let m = mediator(|r| {
        r.format("src").to("dst", Step("dst", "x")).to("mid", Step("mid", "y"));
        r.format("mid").to("dst", Step("dst", "z"));
        r.format("dst");
    });
    assert_eq!(route(&m, "dst", &[]).unwrap(), "src => dst");
    assert_eq!(route(&m, "dst", &["mid"]).unwrap(), "src => mid => dst");
    assert_eq!(route(&m, "dst", &["src"]).unwrap(), "src => dst", "the ends count");
    let err = route(&m, "dst", &["nowhere"]).unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NoPath { .. }));
    let err = route(&m, "dst", &["dst", "mid"]).unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NoPath { .. }), "order matters");
}

#[test]
fn a_format_that_is_already_the_target_is_an_empty_path() {
    let m = mediator(|r| {
        r.format("src");
    });
    let path = m.plan(&FormatId::new("src"), "src", &[]).unwrap();
    assert!(path.steps.is_empty());
    assert_eq!(path.to_string(), "(already there)");
}

#[test]
fn targets_follow_multi_hop_paths_and_exclude_the_source() {
    let m = mediator(|r| {
        r.format("src").provides::<dyn Bitmap>().to("mid", Step("mid", "m"));
        r.format("mid").label("Middle").to("far", Step("far", "f"));
        r.format("far");
        r.format("cap").from_cap::<dyn Bitmap>(Step("cap", "c"));
        r.format("island");
    });
    let names: Vec<_> = m.targets_from("src").into_iter().map(|t| (t.format, t.label)).collect();
    assert_eq!(
        names,
        [("mid".into(), "Middle".into()), ("far".into(), "far".into()), ("cap".into(), "cap".into())]
    );
    assert_eq!(m.targets_from("far"), []);
}

#[test]
fn a_direct_edge_to_an_undefined_format_fails_the_check() {
    let mut r = Registry::new();
    r.format("src").to("ghost", Step("ghost", "g"));
    assert!(r.check().is_err());
}

#[test]
fn formats_lists_edges_with_costs_and_labels() {
    let m = mediator(|r| {
        r.format("src").label("Source").extensions(["s"]).provides::<dyn Bitmap>().to("dst", Step("dst", "d")).cost(4);
        r.format("dst").from_cap::<dyn Bitmap>(Step("dst", "c")).cost(2).from("file", MagicRead::new("dst", b"D", 1));
    });
    let json = serde_json::to_value(m.formats()).unwrap();
    let src = &json["formats"][0];
    assert_eq!((src["name"].as_str(), src["label"].as_str()), (Some("src"), Some("Source")));
    assert_eq!(src["extensions"], serde_json::json!(["s"]));
    assert_eq!(src["provides"], serde_json::json!(["Bitmap"]));
    let dst = &json["formats"][1];
    assert_eq!(dst["label"], "dst");
    assert_eq!(dst["read_from"], serde_json::json!(["file"]));
    assert_eq!(
        dst["produced_by"],
        serde_json::json!([
            { "kind": "format", "from": "src", "cost": 4 },
            { "kind": "capability", "from": "Bitmap", "cost": 2 },
        ])
    );
}

// ---- conversion along planned paths ------------------------------------------------------

fn register_route(r: &mut Registry) {
    r.format("src")
        .from("file", MagicRead::new("src", b"SSSS", 10))
        .extensions(["ss"])
        .provides::<dyn Bitmap>()
        .to("mid", Step("mid", "m"))
        .to("dst", Step("dst", "direct"))
        .cost(5);
    r.format("mid").to("dst", Step("dst", "d"));
    r.format("dst").extensions(["dd"]);
}

#[tokio::test]
async fn convert_follows_the_cheapest_path_and_honours_via() {
    let env = env_with(register_route);
    env.input.write("x.ss", "SSSS").await.unwrap();

    let report = env.engine.convert(&one("x.ss", "dst"), &respak_core::CancellationToken::new()).await.unwrap();
    assert!(matches!(report.outputs[0].result, respak_core::OutputResult::Ok { .. }));
    assert_eq!(env.output.read("x.dd").await.unwrap().to_vec(), b"m>d");

    let req = request(r#"{"version":1,"outputs":[{"from":"x.ss","to":{"type":"dst","via":[]},"path":"y"}]}"#);
    env.engine.convert(&req, &respak_core::CancellationToken::new()).await.unwrap();
    assert_eq!(env.output.read("y").await.unwrap().to_vec(), b"m>d");
}

#[tokio::test]
async fn via_pins_the_route_in_a_request() {
    let env = env_with(|r| {
        r.format("src")
            .from("file", MagicRead::new("src", b"SSSS", 10))
            .extensions(["ss"])
            .to("dst", Step("dst", "direct"))
            .to("mid", Step("mid", "m"))
            .cost(1);
        r.format("mid").to("dst", Step("dst", "d"));
        r.format("dst").extensions(["dd"]);
    });
    env.input.write("x.ss", "SSSS").await.unwrap();
    let json = r#"{"version":1,"outputs":[{"from":"x.ss","to":{"type":"dst","via":["mid"]},"path":"p"},
                                          {"from":"x.ss","to":{"type":"dst","via":["nope"]},"path":"q"}]}"#;
    let req = request(json);
    assert_eq!(serde_json::to_value(&req).unwrap()["outputs"][0]["to"]["via"], serde_json::json!(["mid"]));
    let report = env.engine.convert(&req, &respak_core::CancellationToken::new()).await.unwrap();
    assert_eq!(env.output.read("p").await.unwrap().to_vec(), b"m>d");
    assert!(matches!(error_kind(&report.outputs[1].result), ErrorKind::NoPath { .. }));
}

// ---- detect / discover ------------------------------------------------------------------

fn register_scored(r: &mut Registry) {
    let mut v = MagicRead::new("versioned", b"AAAA", 12);
    v.version = Some(respak_core::Version::with_minor(3, 1));
    r.format("low").from("file", MagicRead::new("low", b"AAAA", 4)).provides::<dyn Bitmap>();
    r.format("versioned").from("file", v).extensions(["vv"]).provides::<dyn Bitmap>();
    r.format("twin").from("file", MagicRead::new("twin", b"AAAA", 12));
    r.format("target").extensions(["tt"]).from_cap::<dyn Bitmap>(Step("target", "t"));
}

#[tokio::test]
async fn detect_lists_all_hits_winner_first_without_reading() {
    let (mediator, cx) = cx_for({
        let mut r = Registry::new();
        r.add(&TestPlugin { id: "t", register: register_scored });
        r
    });
    cx.input.write("f.bin", "AAAA").await.unwrap();
    cx.input.write("junk.bin", "ZZZZ").await.unwrap();
    let file = |p: &str| -> Value { Arc::new(FileData::new(cx.input.clone(), p, p, None)) };

    let hits = mediator.detect(&file("f.bin"), &cx).await.unwrap();
    let shown: Vec<_> = hits.iter().map(|h| (h.format.to_string(), h.score)).collect();
    assert_eq!(shown, [("versioned@3.1".into(), 12), ("twin".into(), 12), ("low".into(), 4)]);
    assert!(mediator.detect(&file("junk.bin"), &cx).await.unwrap().is_empty());
}

#[tokio::test]
async fn discover_reports_winner_alternatives_and_targets() {
    let env = env_with(register_scored);
    env.input.write("f.bin", "AAAA").await.unwrap();
    env.input.write("junk.bin", "ZZZZ").await.unwrap();

    let d = env.engine.discover("f.bin", &DiscoverOptions::default(), &respak_core::CancellationToken::new()).await.unwrap();
    assert_eq!(d.path, "f.bin");
    let e = &d.entries[0];
    assert_eq!((e.path.as_str(), e.format.as_deref(), e.version.as_deref(), e.score), ("f.bin", Some("versioned"), Some("3.1"), 12));
    let alts: Vec<_> = e.alternatives.iter().map(|a| (a.format.as_str(), a.score)).collect();
    assert_eq!(alts, [("twin", 12), ("low", 4)]);
    assert_eq!(e.targets.iter().map(|t| t.format.as_str()).collect::<Vec<_>>(), ["target"]);
    assert_eq!(e.targets[0].defaults, serde_json::json!({}));

    let d = env.engine.discover("junk.bin", &DiscoverOptions::default(), &respak_core::CancellationToken::new()).await.unwrap();
    let e = &d.entries[0];
    assert!(e.format.is_none() && e.targets.is_empty() && e.alternatives.is_empty());
    assert_eq!(serde_json::to_value(e).unwrap()["format"], serde_json::Value::Null);
}

#[tokio::test]
async fn discover_errors_for_a_missing_path_and_ignores_depth_on_plain_files() {
    let env = env_with(register_scored);
    let err = env.engine.discover("nope.bin", &DiscoverOptions::default(), &respak_core::CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NotFound { .. }), "{err}");
    env.input.write("f.bin", "AAAA").await.unwrap();
    // A file that is not a container is described as itself, whatever the depth.
    let d = env.engine.discover("f.bin", &DiscoverOptions { depth: 3, ..Default::default() }, &respak_core::CancellationToken::new()).await.unwrap();
    assert_eq!(d.entries.len(), 1);
}

#[allow(dead_code)]
fn _types(_: &dyn Blob) {}

#[tokio::test]
async fn the_first_registered_edge_runs_when_everything_else_is_equal() {
    let env = env_with(|r| {
        r.format("src")
            .from("file", MagicRead::new("src", b"SSSS", 10))
            .extensions(["ss"])
            .to("dst", Step("dst", "first"))
            .to("dst", Step("dst", "second"));
        r.format("dst").extensions(["dd"]);
    });
    env.input.write("x.ss", "SSSS").await.unwrap();
    env.engine.convert(&one("x.ss", "dst"), &respak_core::CancellationToken::new()).await.unwrap();
    assert_eq!(env.output.read("x.dd").await.unwrap().to_vec(), b"first");
}
