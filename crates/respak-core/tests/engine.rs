//! Core behaviour with a synthetic plugin: formats `alpha` / `beta` recognised by magic bytes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::{services::Memory, Operator};
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, Blob, PixelFormat};
use respak_core::value::BlobData;
use respak_core::{
    magic, ConvertHandler, ConvertRequest, Cx, Data, Detect, Detected, Engine, ErrorKind, FormatId, Mediator,
    OutputResult, Plugin, Probe, ReadHandler, Registry, Result, Target, Value, ValueKey, Version,
};

// ---- fixtures -------------------------------------------------------------------------------

/// A decoded image of some test format; provides `Bitmap` but is not writable.
#[derive(Debug)]
struct Img {
    format: FormatId,
    key: ValueKey,
    buf: BitmapBuf,
}

impl Img {
    fn new(format: FormatId, key: ValueKey) -> Self {
        let info = BitmapInfo {
            width: 1,
            height: 1,
            depth: 1,
            array_len: 1,
            faces: 1,
            mip_count: 1,
            format: PixelFormat::Rgba8,
        };
        let buf = BitmapBuf::new(info, vec![vec![Bytes::from_static(&[1, 2, 3, 4])]]).unwrap();
        Self { format, key, buf }
    }
}

impl Data for Img {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(&self.buf)
    }
}

/// Reads a `file` that starts with `magic` as format `name`.
struct MagicRead {
    name: &'static str,
    magic: &'static [u8],
    score: u32,
    version: Option<Version>,
    header_len: usize,
    seen: Arc<Mutex<Vec<usize>>>,
}

impl MagicRead {
    fn new(name: &'static str, magic: &'static [u8], score: u32) -> Self {
        Self { name, magic, score, version: None, header_len: magic.len(), seen: Default::default() }
    }
}

#[async_trait]
impl Detect for MagicRead {
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        self.seen.lock().unwrap().push(input.header.len());
        Ok(magic(input.header, 0, self.magic).then_some(Detected { score: self.score, version: self.version }))
    }
    fn header_len(&self) -> usize {
        self.header_len
    }
}

#[async_trait]
impl ReadHandler for MagicRead {
    async fn read(&self, source: Value, detected: &Detected, _cx: &Cx) -> Result<Value> {
        let mut format = FormatId::new(self.name);
        if let Some(v) = detected.version {
            format = format.with_version(v);
        }
        Ok(Arc::new(Img::new(format, source.key().clone())))
    }
}

/// Encodes to a blob of format `name` whose bytes say which edge produced it.
struct Enc {
    name: &'static str,
    tag: &'static str,
}

#[async_trait]
impl ConvertHandler for Enc {
    async fn convert(&self, source: Value, _target: &Target, _cx: &Cx) -> Result<Value> {
        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new(self.name), &serde_json::Value::Null);
        Ok(Arc::new(BlobData::with_format(FormatId::new(self.name), Bytes::from(self.tag), key)))
    }
}

struct TestPlugin {
    id: &'static str,
    register: fn(&mut Registry),
}

impl Plugin for TestPlugin {
    fn id(&self) -> &str {
        self.id
    }
    fn register(&self, r: &mut Registry) {
        (self.register)(r)
    }
}

fn register_ab(r: &mut Registry) {
    r.format("alpha").from("file", MagicRead::new("alpha", b"AAAA", 10)).extensions(["aa"]).provides::<dyn Bitmap>();
    r.format("beta")
        .from("file", MagicRead::new("beta", b"BBBB", 10))
        .extensions(["bb"])
        .provides::<dyn Bitmap>()
        .from_cap::<dyn Bitmap>(Enc { name: "beta", tag: "beta-bitmap" });
    r.format("gamma").extensions(["gg"]);
}

fn mem() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

struct Env {
    engine: Engine,
    input: Operator,
    output: Operator,
}

fn env_with(register: fn(&mut Registry)) -> Env {
    let (input, output) = (mem(), mem());
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(TestPlugin { id: "test", register })
        .build()
        .unwrap();
    Env { engine, input, output }
}

fn request(json: &str) -> ConvertRequest {
    serde_json::from_str(json).unwrap()
}

fn one(from: &str, to: &str) -> ConvertRequest {
    request(&format!(r#"{{"version":1,"outputs":[{{"from":"{from}","to":{{"type":"{to}"}}}}]}}"#))
}

fn error_kind(result: &OutputResult) -> &ErrorKind {
    match result {
        OutputResult::Err { error } => error.kind(),
        OutputResult::Ok { files } => panic!("expected an error, got files {files:?}"),
    }
}

fn cx_for(registry: Registry) -> (Mediator, Cx) {
    let mediator = Mediator::new(registry);
    let cx = Cx { mediator: mediator.clone(), input: mem(), output: mem() };
    (mediator, cx)
}

// ---- tests ----------------------------------------------------------------------------------

#[test]
fn duplicate_format_fails_build() {
    let result = Engine::builder()
        .input(mem())
        .output(mem())
        .plugin(TestPlugin { id: "one", register: register_ab })
        .plugin(TestPlugin { id: "two", register: |r| {
            r.format("alpha");
        } })
        .build();
    let err = result.err().expect("duplicate format must fail");
    assert!(matches!(err.kind(), ErrorKind::Custom { .. }), "{err}");
    assert!(err.to_string().contains("alpha"));
}

#[test]
fn magic_matches_at_offset_and_short_header_is_false() {
    assert!(magic(b"xxDDS ", 2, b"DDS "));
    assert!(!magic(b"xxDDS ", 0, b"DDS "));
    assert!(!magic(b"DD", 0, b"DDS "));
    assert!(!magic(b"DDS ", usize::MAX, b"DDS "));
}

#[tokio::test]
async fn extension_candidates_are_scored_before_the_rest() {
    // `beta` would score higher, but the `.dat` extension only makes `alpha` a candidate.
    let mut r = Registry::new();
    r.format("alpha").from("file", MagicRead::new("alpha", b"AAAA", 10)).extensions(["dat"]);
    r.format("beta").from("file", MagicRead::new("beta", b"AAAA", 20)).extensions(["bb"]);
    let (mediator, cx) = cx_for(r);
    cx.input.write("x.dat", "AAAA").await.unwrap();
    cx.input.write("x.zzz", "AAAA").await.unwrap();
    for (path, expected) in [("x.dat", "alpha"), ("x.zzz", "beta")] {
        let file: Value = Arc::new(respak_core::value::FileData::new(cx.input.clone(), path, path, None));
        let value = mediator.identify(file, &cx).await.unwrap();
        assert_eq!(value.format().name(), expected, "{path}");
    }
}

#[tokio::test]
async fn highest_score_wins_and_carries_its_version() {
    let mut low = MagicRead::new("low", b"AAAA", 3);
    low.version = Some(Version::new(1));
    let mut high = MagicRead::new("high", b"AAAA", 12);
    high.version = Some(Version::with_minor(7, 2));
    let mut r = Registry::new();
    r.format("low").from("file", low);
    r.format("high").from("file", high);
    let (mediator, cx) = cx_for(r);
    cx.input.write("f.bin", "AAAA").await.unwrap();
    let file: Value = Arc::new(respak_core::value::FileData::new(cx.input.clone(), "f.bin", "f.bin", None));
    let value = mediator.identify(file, &cx).await.unwrap();
    assert_eq!(value.format().to_string(), "high@7.2");
}

#[tokio::test]
async fn equal_top_scores_are_ambiguous_and_no_match_is_unknown() {
    let env = env_with(|r| {
        r.format("one").from("file", MagicRead::new("one", b"AAAA", 10));
        r.format("two").from("file", MagicRead::new("two", b"AAAA", 10));
    });
    env.input.write("tie.bin", "AAAA").await.unwrap();
    env.input.write("junk.bin", "zzzz").await.unwrap();
    let report = env.engine.convert(&request(
        r#"{"version":1,"outputs":[{"from":"tie.bin","to":{"type":"one"}},{"from":"junk.bin","to":{"type":"one"}}]}"#,
    ))
    .await
    .unwrap();
    match error_kind(&report.outputs[0].result) {
        ErrorKind::AmbiguousFormat { candidates } => assert_eq!(candidates, &["one", "two"]),
        other => panic!("{other:?}"),
    }
    assert!(matches!(error_kind(&report.outputs[1].result), ErrorKind::UnknownFormat));
}

#[tokio::test]
async fn header_prefix_is_sized_to_the_largest_header_len() {
    let short = MagicRead::new("short", b"AA", 10);
    let mut long = MagicRead::new("long", b"BBBB", 10);
    long.header_len = 8;
    let (seen_short, seen_long) = (short.seen.clone(), long.seen.clone());
    let mut r = Registry::new();
    r.format("short").from("file", short);
    r.format("long").from("file", long);
    let (mediator, cx) = cx_for(r);
    cx.input.write("f.bin", "AAAA-and-more-bytes").await.unwrap();
    let file: Value = Arc::new(respak_core::value::FileData::new(cx.input.clone(), "f.bin", "f.bin", None));
    mediator.identify(file, &cx).await.unwrap();
    assert_eq!(*seen_short.lock().unwrap(), [8]);
    assert_eq!(*seen_long.lock().unwrap(), [8]);
}

#[tokio::test]
async fn converts_through_a_capability_edge_and_defaults_the_output_path() {
    let env = env_with(register_ab);
    env.input.write("dir.v2/pic.aa", "AAAA").await.unwrap();
    let report = env.engine.convert(&one("dir.v2/pic.aa", "beta")).await.unwrap();
    match &report.outputs[0].result {
        OutputResult::Ok { files } => assert_eq!(files, &["dir.v2/pic.bb"]),
        OutputResult::Err { error } => panic!("{error}"),
    }
    assert_eq!(env.output.read("dir.v2/pic.bb").await.unwrap().to_vec(), b"beta-bitmap");
}

#[tokio::test]
async fn explicit_path_overrides_the_default() {
    let env = env_with(register_ab);
    env.input.write("pic.aa", "AAAA").await.unwrap();
    let req = request(r#"{"version":1,"outputs":[{"from":"pic.aa","to":{"type":"beta"},"path":"custom/out.dat"}]}"#);
    env.engine.convert(&req).await.unwrap();
    assert!(env.output.exists("custom/out.dat").await.unwrap());
    assert!(!env.output.exists("pic.bb").await.unwrap());
}

#[tokio::test]
async fn cheaper_capability_edge_wins() {
    let env = env_with(|r| {
        r.format("alpha")
            .from("file", MagicRead::new("alpha", b"AAAA", 10))
            .extensions(["aa"])
            .provides::<dyn Bitmap>()
            .provides::<dyn Blob>();
        r.format("beta")
            .extensions(["bb"])
            .from_cap::<dyn Bitmap>(Enc { name: "beta", tag: "bitmap-edge" })
            .cost(3)
            .from_cap::<dyn Blob>(Enc { name: "beta", tag: "blob-edge" })
            .cost(2);
    });
    env.input.write("pic.aa", "AAAA").await.unwrap();
    env.engine.convert(&one("pic.aa", "beta")).await.unwrap();
    assert_eq!(env.output.read("pic.bb").await.unwrap().to_vec(), b"blob-edge");
}

#[tokio::test]
async fn no_edge_to_the_target_is_no_path() {
    let env = env_with(register_ab);
    env.input.write("pic.aa", "AAAA").await.unwrap();
    let report = env.engine.convert(&one("pic.aa", "gamma")).await.unwrap();
    assert!(matches!(error_kind(&report.outputs[0].result), ErrorKind::NoPath { .. }));
}

#[tokio::test]
async fn a_value_already_in_the_target_format_is_returned_as_is() {
    let mut r = Registry::new();
    r.add(&TestPlugin { id: "t", register: register_ab });
    let (mediator, cx) = cx_for(r);
    let value: Value = Arc::new(Img::new(FormatId::new("beta"), ValueKey::file("x.bb")));
    let target = Target { format: FormatId::new("beta"), options: serde_json::Value::Null };
    let out = mediator.convert(value.clone(), target, &cx).await.unwrap();
    assert!(Arc::ptr_eq(&value, &out));
}

#[tokio::test]
async fn a_result_that_is_not_a_blob_is_an_error_and_writes_nothing() {
    let env = env_with(register_ab);
    env.input.write("pic.aa", "AAAA").await.unwrap();
    // alpha -> alpha is the identified `Img`, which is not writable.
    let report = env.engine.convert(&one("pic.aa", "alpha")).await.unwrap();
    assert!(matches!(error_kind(&report.outputs[0].result), ErrorKind::Custom { .. }));
    assert!(!env.output.exists("pic.aa").await.unwrap());
}

#[tokio::test]
async fn unknown_request_version_is_invalid() {
    let env = env_with(register_ab);
    let err = env.engine.convert(&request(r#"{"version":2,"outputs":[]}"#)).await.err().unwrap();
    assert!(matches!(err.kind(), ErrorKind::InvalidRequest { .. }));
}

#[tokio::test]
async fn a_failing_output_does_not_stop_the_others() {
    let env = env_with(register_ab);
    env.input.write("pic.aa", "AAAA").await.unwrap();
    let report = env
        .engine
        .convert(&request(
            r#"{"version":1,"outputs":[{"from":"missing.aa","to":{"type":"beta"}},{"from":"pic.aa","to":{"type":"beta"}}]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(report.outputs.len(), 2);
    assert_eq!(report.outputs[0].from, "missing.aa");
    assert!(matches!(error_kind(&report.outputs[0].result), ErrorKind::NotFound { .. }));
    assert!(matches!(report.outputs[1].result, OutputResult::Ok { .. }));
    assert!(env.output.exists("pic.bb").await.unwrap());
}

#[test]
fn request_json_round_trips() {
    let json = r#"{"version":1,"outputs":[{"from":"a.aa","to":{"type":"beta@2","level":3},"path":"o"}]}"#;
    let req = request(json);
    assert_eq!(req.outputs[0].to.format.to_string(), "beta@2");
    assert_eq!(req.outputs[0].to.options["level"], 3);
    assert_eq!(serde_json::to_string(&req).unwrap(), json);
}

#[tokio::test]
async fn report_serializes_with_ok_and_err() {
    let env = env_with(register_ab);
    env.input.write("pic.aa", "AAAA").await.unwrap();
    let report = env.engine.convert(&one("pic.aa", "beta")).await.unwrap();
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["outputs"][0]["result"]["Ok"]["files"][0], "pic.bb");
}
