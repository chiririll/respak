//! Events, middleware, cancellation and bounded work in `Cx` (slice 12).


use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use respak_core::testing::*;
use respak_core::broadcast::error::{RecvError, TryRecvError};
use respak_core::testing::CxBuilder;
use respak_core::middleware::{Convert, Middleware, Next};
use respak_core::{
    Cx, ErrorKind, Event, Events, FormatId, Registry, Result, Target, Value,
};

fn register(r: &mut Registry) {
    r.format("src")
        .from("file", MagicRead::new("src", b"SSSS", 10))
        .extensions(["ss"])
        .to(
            "dst",
            Enc {
                name: "dst",
                tag: "d",
            },
        );
    r.format("dst");
}

fn registry(register: fn(&mut Registry)) -> Registry {
    let mut r = Registry::new();
    r.add(&TestPlugin { id: "t", register });
    r.check().unwrap();
    r
}

async fn input() -> opendal::Operator {
    let input = mem();
    input.write("x.ss", "SSSS").await.unwrap();
    input
}

async fn cx_with(registry: Registry) -> Cx {
    respak_core::testing::cx(registry, input().await, mem())
}

fn to(format: &'static str) -> Target {
    Target {
        format: FormatId::new(format),
        options: serde_json::Value::Null,
        via: Vec::new(),
    }
}

fn drain(rx: &mut respak_core::broadcast::Receiver<Event>) -> Vec<Event> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.push(e);
    }
    out
}

fn warning(message: &str) -> Event {
    Event::Warning {
        message: message.into(),
    }
}

#[tokio::test]
async fn subscribers_get_events_in_order_and_publishing_alone_is_fine() {
    let events = Events::new(8);
    events.publish(warning("nobody listens"));
    let mut rx = events.subscribe();
    for m in ["a", "b", "c"] {
        events.publish(warning(m));
    }
    assert_eq!(
        drain(&mut rx),
        vec![warning("a"), warning("b"), warning("c")]
    );
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn a_lagging_subscriber_gets_lagged_not_a_panic() {
    let events = Events::new(2);
    let mut rx = events.subscribe();
    for m in ["0", "1", "2", "3", "4"] {
        events.publish(warning(m));
    }
    assert!(matches!(rx.recv().await, Err(RecvError::Lagged(3))));
    assert_eq!(rx.recv().await.unwrap(), warning("3"));
    assert_eq!(rx.recv().await.unwrap(), warning("4"));
}

struct Spy {
    log: Arc<Mutex<Vec<String>>>,
    rx: Mutex<respak_core::broadcast::Receiver<Event>>,
    short_circuit: bool,
}

#[async_trait]
impl Middleware for Spy {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        let started = matches!(
            self.rx.lock().unwrap().try_recv(),
            Ok(Event::RequestStarted { .. })
        );
        self.log.lock().unwrap().push(format!(
            "{} -> {} started_already={started}",
            req.source.format(),
            req.to.format
        ));
        if self.short_circuit {
            return Ok(req.source.clone());
        }
        next.run(req, cx).await
    }
}

#[tokio::test]
async fn host_middleware_runs_after_the_built_ins_and_can_short_circuit() {
    for short in [false, true] {
        let events = Events::new(64);
        let log = Arc::new(Mutex::new(Vec::new()));
        let spy = Arc::new(Spy {
            log: log.clone(),
            rx: Mutex::new(events.subscribe()),
            short_circuit: short,
        });
        let cx = CxBuilder::new(registry(register), input().await, mem())
            .events(events)
            .middleware(vec![spy])
            .build();
        let mediator = cx.mediator.clone();
        let source = mediator.open("x.ss", &cx).await.unwrap();
        let result = mediator.convert(source, to("dst"), &cx).await.unwrap();
        assert_eq!(
            log.lock().unwrap().as_slice(),
            ["file -> dst started_already=true"]
        );
        assert_eq!(result.format().name(), if short { "file" } else { "dst" });
    }
}

#[tokio::test]
async fn a_triggered_token_cancels_before_any_handler_runs() {
    let cx = cx_with(registry(register)).await;
    let mediator = cx.mediator.clone();
    let mut rx = mediator.subscribe();
    let source = mediator.open("x.ss", &cx).await.unwrap();
    cx.cancel.cancel();
    let err = mediator.convert(source, to("dst"), &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    assert!(
        drain(&mut rx).is_empty(),
        "cancelled requests publish no events"
    );
}

#[tokio::test]
async fn requests_publish_started_then_finished_or_failed() {
    let cx = cx_with(registry(register)).await;
    let mediator = cx.mediator.clone();
    let mut rx = mediator.subscribe();
    let source = mediator.open("x.ss", &cx).await.unwrap();

    let kinds = |events: Vec<Event>| {
        events
            .iter()
            .map(|e| match e {
                Event::RequestStarted { .. } => "started",
                Event::RequestFinished { .. } => "finished",
                Event::RequestFailed { .. } => "failed",
                other => panic!("unexpected {other:?}"),
            })
            .collect::<Vec<_>>()
    };

    mediator
        .convert(source.clone(), to("dst"), &cx)
        .await
        .unwrap();
    assert_eq!(kinds(drain(&mut rx)), ["started", "finished"]);

    assert!(mediator.convert(source, to("nowhere"), &cx).await.is_err());
    assert_eq!(kinds(drain(&mut rx)), ["started", "failed"]);
}

fn enter(current: &AtomicUsize, max: &AtomicUsize) {
    max.fetch_max(current.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
}

#[tokio::test]
async fn blocking_and_io_respect_their_limits() {
    let cx = CxBuilder::new(registry(register), mem(), mem())
        .limits(2, 3)
        .build();

    let (current, max) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    futures::future::join_all((0..12).map(|_| {
        let (cx, current, max) = (cx.clone(), current.clone(), max.clone());
        async move {
            cx.blocking(move || {
                enter(&current, &max);
                std::thread::sleep(std::time::Duration::from_millis(20));
                current.fetch_sub(1, Ordering::SeqCst);
            })
            .await
            .unwrap();
        }
    }))
    .await;
    assert_eq!(max.load(Ordering::SeqCst), 2);

    let (current, max) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    futures::future::join_all((0..12).map(|_| {
        let (cx, current, max) = (cx.clone(), current.clone(), max.clone());
        async move {
            cx.io(async {
                enter(&current, &max);
                for _ in 0..20 {
                    tokio::task::yield_now().await;
                }
                current.fetch_sub(1, Ordering::SeqCst);
            })
            .await
            .unwrap();
        }
    }))
    .await;
    assert_eq!(max.load(Ordering::SeqCst), 3);
}

fn register_tie(r: &mut Registry) {
    r.format("src")
        .from("file", MagicRead::new("src", b"SSSS", 10))
        .extensions(["ss"])
        .to(
            "dst",
            Enc {
                name: "dst",
                tag: "first",
            },
        )
        .to(
            "dst",
            Enc {
                name: "dst",
                tag: "second",
            },
        );
    r.format("dst");
}

#[tokio::test]
async fn the_tie_break_is_a_warning_event() {
    let cx = cx_with(registry(register_tie)).await;
    let mediator = cx.mediator.clone();
    let mut rx = mediator.subscribe();

    mediator.plan(&FormatId::new("src"), "dst", &[]).unwrap();
    let source = mediator.open("x.ss", &cx).await.unwrap();
    mediator.convert(source, to("dst"), &cx).await.unwrap();

    let warnings: Vec<_> = drain(&mut rx)
        .into_iter()
        .filter_map(|e| match e {
            Event::Warning { message } => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .all(|m| m.contains("src") && m.contains("dst")),
        "{warnings:?}"
    );
}

// ---- Engine --------------------------------------------------------------------------------

/// Triggers the call's token once the first conversion has finished.
struct CancelAfterFirst(AtomicUsize);

#[async_trait]
impl Middleware for CancelAfterFirst {
    async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value> {
        let result = next.run(req, cx).await;
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            cx.cancel.cancel();
        }
        result
    }
}

#[tokio::test]
async fn cancelling_mid_run_keeps_finished_outputs_and_cancels_the_rest() {
    let (input, output) = (mem(), mem());
    for name in ["1.aa", "2.aa", "3.aa"] {
        input.write(name, "AAAA").await.unwrap();
    }
    let engine = respak_core::Engine::builder()
        .input(input)
        .output(output.clone())
        .plugin(TestPlugin {
            id: "t",
            register: register_ab,
        })
        .middleware(CancelAfterFirst(AtomicUsize::new(0)))
        .max_concurrency(1)
        .build()
        .unwrap();
    let report = engine
        .convert(&one("*.aa", "beta"), &respak_core::CancellationToken::new())
        .await
        .unwrap();

    assert!(matches!(
        report.outputs[0].result,
        respak_core::OutputResult::Ok { .. }
    ));
    for o in &report.outputs[1..] {
        assert!(
            matches!(error_kind(&o.result), ErrorKind::Cancelled),
            "{} {:?}",
            o.from,
            o.result
        );
    }
    assert!(output.exists("1.bb").await.unwrap());
    assert!(!output.exists("2.bb").await.unwrap());
}

#[tokio::test]
async fn the_engine_publishes_output_written_and_progress() {
    let (input, output) = (mem(), mem());
    for name in ["1.aa", "2.aa"] {
        input.write(name, "AAAA").await.unwrap();
    }
    let engine = respak_core::Engine::builder()
        .input(input)
        .output(output)
        .plugin(TestPlugin {
            id: "t",
            register: register_ab,
        })
        .build()
        .unwrap();
    let mut rx = engine.events();
    engine
        .convert(&one("*.aa", "beta"), &respak_core::CancellationToken::new())
        .await
        .unwrap();

    let events = drain(&mut rx);
    let mut written: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::OutputWritten { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    written.sort();
    assert_eq!(written, ["1.bb", "2.bb"]);
    let progress: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Progress { task, done, total } if task == "convert" => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(progress, [(1, 2), (2, 2)]);
}
