//! The in-memory result cache (slice 29).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::testing::*;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, ErrorKind, FormatId, Registry, Result, Target, Value,
    ValueKey,
};

fn to_dst(options: serde_json::Value) -> Target {
    Target {
        format: FormatId::new("dst"),
        options,
        via: Vec::new(),
    }
}

/// `src -> dst`: counts runs per source path, produces `size` bytes, and converts each path in
/// `links` for its source first. It can fail or cancel on its first run.
#[derive(Clone, Default)]
struct Counting {
    runs: Arc<Mutex<HashMap<String, usize>>>,
    size: usize,
    links: Vec<(&'static str, &'static str)>,
    fail_first: bool,
    cancel_first: bool,
    delay: Duration,
}

impl Counting {
    fn runs(&self, path: &str) -> usize {
        self.runs.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

#[async_trait]
impl ConvertHandler for Counting {
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let path = source.key().to_string();
        let first = {
            let mut runs = self.runs.lock().unwrap();
            let n = runs.entry(path.clone()).or_default();
            *n += 1;
            *n == 1
        };
        tokio::time::sleep(self.delay).await;
        if first && self.fail_first {
            return Err(respak_core::Error::msg("first run fails"));
        }
        if first && self.cancel_first {
            return Err(respak_core::Error::new(ErrorKind::Cancelled));
        }
        for (_, link) in self.links.iter().filter(|(from, _)| *from == path) {
            let other = cx.mediator.open(link, cx).await?;
            cx.mediator
                .convert(other, to_dst(serde_json::Value::Null), cx)
                .await?;
        }
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("dst"),
            &target.options,
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("dst"),
            Bytes::from(vec![0u8; self.size]),
            key,
        )))
    }
}

struct CountingPlugin(Counting);

impl respak_core::Plugin for CountingPlugin {
    fn id(&self) -> &str {
        "counting"
    }
    fn register(&self, r: &mut Registry) {
        r.format("src")
            .from("file", MagicRead::new("src", b"SSSS", 10))
            .extensions(["ss"])
            .to("dst", self.0.clone());
        r.format("dst");
    }
}

fn registry(handler: &Counting) -> Registry {
    let mut r = Registry::new();
    r.add(&CountingPlugin(handler.clone()));
    r.check().unwrap();
    r
}

/// The handler and a context with the cache set to `cache` bytes (`None`: off).
async fn setup(handler: Counting, cache: Option<u64>, paths: &[&str]) -> (Counting, Cx) {
    let mut builder = CxBuilder::new(registry(&handler), mem(), mem());
    if let Some(bytes) = cache {
        builder = builder.memory_cache(bytes);
    }
    let cx = builder.build();
    for p in paths {
        cx.input.write(p, "SSSS").await.unwrap();
    }
    (handler, cx)
}

fn sized(size: usize) -> Counting {
    Counting {
        size,
        ..Counting::default()
    }
}

async fn convert(cx: &Cx, path: &str) -> Result<Value> {
    let source = cx.mediator.open(path, cx).await?;
    cx.mediator
        .convert(source, to_dst(serde_json::Value::Null), cx)
        .await
}

#[tokio::test]
async fn sequential_requests_run_once_and_share_the_value() {
    let (h, cx) = setup(sized(100), Some(1 << 20), &["a.ss"]).await;
    let first = convert(&cx, "a.ss").await.unwrap();
    let second = convert(&cx, "a.ss").await.unwrap();
    assert_eq!(h.runs("a.ss"), 1);
    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn without_the_cache_sequential_requests_run_again() {
    let (h, cx) = setup(sized(100), None, &["a.ss"]).await;
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "a.ss").await.unwrap();
    assert_eq!(h.runs("a.ss"), 2);
}

#[tokio::test]
async fn different_options_are_different_entries() {
    let (h, cx) = setup(sized(100), Some(1 << 20), &["a.ss"]).await;
    let a = cx.mediator.open("a.ss", &cx).await.unwrap();
    for x in [1, 2, 1] {
        cx.mediator
            .convert(a.clone(), to_dst(serde_json::json!({ "x": x })), &cx)
            .await
            .unwrap();
    }
    assert_eq!(h.runs("a.ss"), 2);
}

#[tokio::test]
async fn a_failed_run_is_not_cached() {
    let handler = Counting {
        fail_first: true,
        ..sized(100)
    };
    let (h, cx) = setup(handler, Some(1 << 20), &["a.ss"]).await;
    assert!(convert(&cx, "a.ss").await.is_err());
    assert!(convert(&cx, "a.ss").await.is_ok());
    assert!(convert(&cx, "a.ss").await.is_ok());
    assert_eq!(h.runs("a.ss"), 2);
}

#[tokio::test]
async fn a_cancelled_run_is_not_cached() {
    let handler = Counting {
        cancel_first: true,
        ..sized(100)
    };
    let (h, cx) = setup(handler, Some(1 << 20), &["a.ss"]).await;
    let err = convert(&cx, "a.ss").await.unwrap_err();
    assert!(err.is_cancelled(), "{err}");
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "a.ss").await.unwrap();
    assert_eq!(h.runs("a.ss"), 2);
}

#[tokio::test]
async fn the_least_recently_used_value_goes_first() {
    // Room for two of the three 100-byte values.
    let (h, cx) = setup(sized(100), Some(250), &["a.ss", "b.ss", "c.ss"]).await;
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "b.ss").await.unwrap();
    convert(&cx, "a.ss").await.unwrap(); // a is now newer than b
    convert(&cx, "c.ss").await.unwrap(); // evicts b
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "b.ss").await.unwrap();
    assert_eq!((h.runs("a.ss"), h.runs("b.ss"), h.runs("c.ss")), (1, 2, 1));
}

#[tokio::test]
async fn a_value_without_a_size_is_not_cached() {
    let (h, cx) = setup(sized(0), Some(1 << 20), &["a.ss"]).await;
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "a.ss").await.unwrap();
    assert_eq!(h.runs("a.ss"), 2);
}

#[tokio::test]
async fn concurrent_requests_share_a_run_and_a_later_one_hits_the_cache() {
    let handler = Counting {
        delay: Duration::from_millis(50),
        ..sized(100)
    };
    let (h, cx) = setup(handler, Some(1 << 20), &["a.ss"]).await;
    let results = futures::future::join_all((0..5).map(|_| convert(&cx, "a.ss"))).await;
    assert!(results.iter().all(|r| r.is_ok()));
    convert(&cx, "a.ss").await.unwrap();
    assert_eq!(h.runs("a.ss"), 1);
}

#[tokio::test]
async fn a_dependency_shared_by_two_requests_that_do_not_overlap_converts_once() {
    let handler = Counting {
        links: vec![("a.ss", "t.ss"), ("b.ss", "t.ss")],
        ..sized(100)
    };
    let (h, cx) = setup(handler, Some(1 << 20), &["a.ss", "b.ss", "t.ss"]).await;
    convert(&cx, "a.ss").await.unwrap();
    convert(&cx, "b.ss").await.unwrap();
    assert_eq!(h.runs("t.ss"), 1);
}

#[tokio::test]
async fn the_cycle_guard_still_works_with_the_cache_on() {
    let handler = Counting {
        links: vec![("a.ss", "b.ss"), ("b.ss", "a.ss")],
        ..sized(100)
    };
    let (_, cx) = setup(handler, Some(1 << 20), &["a.ss", "b.ss"]).await;
    let err = convert(&cx, "a.ss").await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::DependencyCycle { .. }),
        "{err}"
    );
}
