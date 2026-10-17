//! Dedup and cycle-guard middleware (slice 13).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::testing::*;
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, ErrorKind, FormatId, Registry, Result, Target, Value, ValueKey,
};

fn to_dst() -> Target {
    Target {
        format: FormatId::new("dst"),
        options: serde_json::Value::Null,
        via: Vec::new(),
    }
}

/// `src -> dst`: counts runs, waits a bit, then converts `link` (a path on the input) if set.
struct Slow {
    runs: Arc<AtomicUsize>,
    /// Source path -> the path it asks for.
    links: Vec<(&'static str, &'static str)>,
}

#[async_trait]
impl ConvertHandler for Slow {
    async fn convert(&self, source: Value, _target: &Target, cx: &Cx) -> Result<Value> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let path = source.key().to_string();
        if let Some((_, link)) = self.links.iter().find(|(from, _)| *from == path) {
            let other = cx.mediator.open(link, cx).await?;
            cx.mediator.convert(other, to_dst(), cx).await?;
        }
        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("dst"),
            &serde_json::Value::Null,
        );
        Ok(Arc::new(BlobData::with_format(
            FormatId::new("dst"),
            Bytes::from("d"),
            key,
        )))
    }
}

fn setup(
    links: Vec<(&'static str, &'static str)>,
) -> (respak_core::Mediator, Cx, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let mut r = Registry::new();
    r.add(&SlowPlugin {
        links,
        runs: runs.clone(),
    });
    r.check().unwrap();
    let (mediator, cx) = cx_for(r);
    (mediator, cx, runs)
}

async fn put(cx: &Cx, paths: &[&str]) {
    for p in paths {
        cx.input.write(p, "SSSS").await.unwrap();
    }
}

#[tokio::test]
async fn concurrent_identical_requests_share_one_run() {
    let (m, cx, runs) = setup(vec![]);
    put(&cx, &["a.ss"]).await;
    let a = m.open("a.ss", &cx).await.unwrap();
    let results =
        futures::future::join_all((0..5).map(|_| m.convert(a.clone(), to_dst(), &cx))).await;
    assert!(results.iter().all(|r| r.is_ok()));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sequential_requests_run_again() {
    let (m, cx, runs) = setup(vec![]);
    put(&cx, &["a.ss"]).await;
    let a = m.open("a.ss", &cx).await.unwrap();
    m.convert(a.clone(), to_dst(), &cx).await.unwrap();
    m.convert(a, to_dst(), &cx).await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn different_sources_are_not_merged() {
    let (m, cx, runs) = setup(vec![]);
    put(&cx, &["a.ss", "b.ss"]).await;
    let (a, b) = (
        m.open("a.ss", &cx).await.unwrap(),
        m.open("b.ss", &cx).await.unwrap(),
    );
    let (ra, rb) = tokio::join!(m.convert(a, to_dst(), &cx), m.convert(b, to_dst(), &cx));
    assert!(ra.is_ok() && rb.is_ok());
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_request_for_itself_is_a_cycle() {
    let (m, cx, _) = setup(vec![("a.ss", "a.ss")]);
    put(&cx, &["a.ss"]).await;
    let a = m.open("a.ss", &cx).await.unwrap();
    let err = m.convert(a, to_dst(), &cx).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::DependencyCycle { chain } if chain.len() >= 2),
        "{err}"
    );
}

#[tokio::test]
async fn a_longer_loop_is_a_cycle() {
    let (m, cx, _) = setup(vec![("a.ss", "b.ss"), ("b.ss", "c.ss"), ("c.ss", "a.ss")]);
    put(&cx, &["a.ss", "b.ss", "c.ss"]).await;
    let a = m.open("a.ss", &cx).await.unwrap();
    let err = m.convert(a, to_dst(), &cx).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::DependencyCycle { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn concurrent_chains_that_wait_on_each_other_fail_instead_of_hanging() {
    let (m, cx, _) = setup(vec![("a.ss", "b.ss"), ("b.ss", "a.ss")]);
    put(&cx, &["a.ss", "b.ss"]).await;
    let (a, b) = (
        m.open("a.ss", &cx).await.unwrap(),
        m.open("b.ss", &cx).await.unwrap(),
    );
    let both = async { tokio::join!(m.convert(a, to_dst(), &cx), m.convert(b, to_dst(), &cx)) };
    let (ra, rb) = tokio::time::timeout(Duration::from_secs(5), both)
        .await
        .expect("deadlocked");
    for r in [ra, rb] {
        let err = r.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::DependencyCycle { .. }),
            "{err}"
        );
    }
}

struct SlowPlugin {
    runs: Arc<AtomicUsize>,
    links: Vec<(&'static str, &'static str)>,
}

impl respak_core::Plugin for SlowPlugin {
    fn id(&self) -> &str {
        "slow"
    }
    fn register(&self, r: &mut Registry) {
        let handler = Slow {
            runs: self.runs.clone(),
            links: self.links.clone(),
        };
        r.format("src")
            .from("file", MagicRead::new("src", b"SSSS", 10))
            .extensions(["ss"])
            .to("dst", handler);
        r.format("dst");
    }
}

#[tokio::test]
async fn the_report_lists_the_values_a_converter_asked_for() {
    let (input, output) = (mem(), mem());
    let plugin = SlowPlugin {
        runs: Arc::default(),
        links: vec![("a.ss", "b.ss")],
    };
    let engine = respak_core::Engine::builder()
        .input(input.clone())
        .output(output)
        .plugin(plugin)
        .build()
        .unwrap();
    input.write("a.ss", "SSSS").await.unwrap();
    input.write("b.ss", "SSSS").await.unwrap();

    let report = engine
        .convert(&one("a.ss", "dst"), &respak_core::CancellationToken::new())
        .await
        .unwrap();
    match &report.outputs[0].result {
        respak_core::OutputResult::Ok { dependencies, .. } => {
            let seen: Vec<_> = dependencies
                .iter()
                .map(|d| (d.key.as_str(), d.format.as_str()))
                .collect();
            assert_eq!(seen, [("b.ss", "dst")]);
        }
        other => panic!("{other:?}"),
    }
}
