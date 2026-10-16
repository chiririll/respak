//! Shared fixtures: formats `alpha` / `beta` recognised by magic bytes.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::{services::Memory, Operator};
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::value::BlobData;
use respak_core::{
    magic, ConvertHandler, ConvertRequest, Cx, Data, Detect, Detected, Engine, ErrorKind, FormatId,
    Mediator, OutputResult, Plugin, Probe, ReadHandler, Registry, Result, Target, Value, ValueKey,
    Version,
};

// ---- fixtures -------------------------------------------------------------------------------

/// A decoded image of some test format; provides `Bitmap` but is not writable.
#[derive(Debug)]
pub struct Img {
    pub format: FormatId,
    pub key: ValueKey,
    pub buf: BitmapBuf,
}

impl Img {
    pub fn new(format: FormatId, key: ValueKey) -> Self {
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
pub struct MagicRead {
    pub name: &'static str,
    pub magic: &'static [u8],
    pub score: u32,
    pub version: Option<Version>,
    pub header_len: usize,
    pub seen: Arc<Mutex<Vec<usize>>>,
}

impl MagicRead {
    pub fn new(name: &'static str, magic: &'static [u8], score: u32) -> Self {
        Self {
            name,
            magic,
            score,
            version: None,
            header_len: magic.len(),
            seen: Default::default(),
        }
    }
}

#[async_trait]
impl Detect for MagicRead {
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        self.seen.lock().unwrap().push(input.header.len());
        Ok(magic(input.header, 0, self.magic).then_some(Detected {
            score: self.score,
            version: self.version,
        }))
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
pub struct Enc {
    pub name: &'static str,
    pub tag: &'static str,
}

#[async_trait]
impl ConvertHandler for Enc {
    async fn convert(&self, source: Value, _target: &Target, _cx: &Cx) -> Result<Value> {
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new(self.name),
            &serde_json::Value::Null,
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new(self.name),
            Bytes::from(self.tag),
            key,
        )))
    }
}

pub struct TestPlugin {
    pub id: &'static str,
    pub register: fn(&mut Registry),
}

impl Plugin for TestPlugin {
    fn id(&self) -> &str {
        self.id
    }
    fn register(&self, r: &mut Registry) {
        (self.register)(r)
    }
}

pub fn register_ab(r: &mut Registry) {
    r.format("alpha")
        .from("file", MagicRead::new("alpha", b"AAAA", 10))
        .extensions(["aa"])
        .provides::<dyn Bitmap>();
    r.format("beta")
        .from("file", MagicRead::new("beta", b"BBBB", 10))
        .extensions(["bb"])
        .provides::<dyn Bitmap>()
        .from_cap::<dyn Bitmap>(Enc {
            name: "beta",
            tag: "beta-bitmap",
        });
    r.format("gamma").extensions(["gg"]);
}

pub fn mem() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

pub struct Env {
    pub engine: Engine,
    pub input: Operator,
    pub output: Operator,
}

pub fn env_with(register: fn(&mut Registry)) -> Env {
    let (input, output) = (mem(), mem());
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(TestPlugin {
            id: "test",
            register,
        })
        .build()
        .unwrap();
    Env {
        engine,
        input,
        output,
    }
}

pub fn request(json: &str) -> ConvertRequest {
    serde_json::from_str(json).unwrap()
}

pub fn one(from: &str, to: &str) -> ConvertRequest {
    request(&format!(
        r#"{{"version":1,"outputs":[{{"from":"{from}","to":{{"type":"{to}"}}}}]}}"#
    ))
}

pub fn error_kind(result: &OutputResult) -> &ErrorKind {
    match result {
        OutputResult::Err { error } => error.kind(),
        OutputResult::Ok { files, .. } => panic!("expected an error, got files {files:?}"),
    }
}

pub fn cx_for(registry: Registry) -> (Mediator, Cx) {
    let mediator = Mediator::new(registry);
    let cx = Cx::new(mediator.clone(), mem(), mem());
    (mediator, cx)
}
