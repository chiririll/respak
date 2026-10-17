//! Fixtures for tests of plugins and handlers: a [`Cx`] without an [`Engine`](crate::Engine),
//! and formats `alpha` / `beta` recognised by magic bytes.

#![allow(missing_docs)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::{services::Memory, Operator};

use crate::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use crate::event::Events;
use crate::mediator::{Cx, Mediator, Target};
use crate::middleware::Middleware;
use crate::detect::{magic, Detect, Detected, Probe};
use crate::plugin::{Plugin, Registry};
use crate::value::{BlobData, Data, Value, ValueKey};
use crate::{
    CancellationToken, ConvertHandler, ConvertRequest, Engine, ErrorKind, FormatId, OutputResult,
    ReadHandler, Result, Version,
};

/// A context over `registry` reading from `input` and writing to `output`, with defaults for the rest.
pub fn cx(registry: Registry, input: Operator, output: Operator) -> Cx {
    CxBuilder::new(registry, input, output).build()
}

/// A mediator over `registry` alone, for planning and detection tests.
pub fn mediator(registry: Registry) -> Mediator {
    Mediator::new(registry)
}

/// Builds a [`Cx`] with the settings an `Engine` would give it.
pub struct CxBuilder {
    mediator: Mediator,
    input: Operator,
    output: Operator,
    cache: Option<Operator>,
    cancel: Option<CancellationToken>,
    limits: Option<(usize, usize)>,
}

impl CxBuilder {
    /// A builder over a checked registry.
    pub fn new(registry: Registry, input: Operator, output: Operator) -> Self {
        Self {
            mediator: Mediator::new(registry),
            input,
            output,
            cache: None,
            cancel: None,
            limits: None,
        }
    }

    /// Uses `events` as the event channel.
    pub fn events(mut self, events: Events) -> Self {
        self.mediator = self.mediator.with_events(events);
        self
    }

    /// Adds host middleware, run after the built-ins.
    pub fn middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        self.mediator = self.mediator.with_middleware(middleware);
        self
    }

    /// Uses `op` as the plugin cache.
    pub fn cache(mut self, op: Operator) -> Self {
        self.cache = Some(op);
        self
    }

    /// Uses `cancel` as the call's token.
    pub fn cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Bounds `Cx::blocking` and `Cx::io` to the given numbers of concurrent tasks.
    pub fn limits(mut self, blocking: usize, io: usize) -> Self {
        self.limits = Some((blocking, io));
        self
    }

    /// The context.
    pub fn build(self) -> Cx {
        let mut cx = Cx::new(self.mediator, self.input, self.output);
        if let Some(op) = self.cache {
            cx = cx.with_cache(op);
        }
        if let Some(cancel) = self.cancel {
            cx = cx.with_cancel(cancel);
        }
        if let Some((blocking, io)) = self.limits {
            cx = cx.with_limits(blocking, io);
        }
        cx
    }
}

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
    let engine = crate::Engine::builder()
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

/// A context over `registry` with in-memory input and output, and its mediator.
pub fn cx_for(registry: Registry) -> (Mediator, Cx) {
    let cx = cx(registry, mem(), mem());
    (cx.mediator.clone(), cx)
}

/// A plugin that only has an id and a cache version, to test cache namespacing.
pub struct Versioned(pub &'static str, pub u32);

impl Plugin for Versioned {
    fn id(&self) -> &str {
        self.0
    }
    fn cache_version(&self) -> u32 {
        self.1
    }
    fn register(&self, _: &mut Registry) {}
}
