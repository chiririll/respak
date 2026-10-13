//! Batch requests: globs, lists, `{path, format}`, parallel outputs, composite outputs and the
//! written-paths map.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use common::*;
use respak_core::value::{BlobData, BundleData};
use respak_core::{ConvertHandler, Cx, ErrorKind, FormatId, OutputResult, Registry, Result, Target, Value, ValueKey};

/// alpha -> multi: a bundle of two named blobs (a composite output).
struct Multi;

#[async_trait]
impl ConvertHandler for Multi {
    async fn convert(&self, source: Value, _: &Target, _: &Cx) -> Result<Value> {
        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("multi"), &serde_json::Value::Null);
        let item = |name: &str, bytes: &'static str| {
            // The shared item has the same key whatever the source is.
            let key = ValueKey::file(format!("shared/{name}"));
            (name.to_owned(), Arc::new(BlobData::with_format(FormatId::new("beta"), Bytes::from(bytes), key)) as Value)
        };
        Ok(Arc::new(BundleData::named(vec![item("one.bin", "1"), item("two.bin", "2")], key)))
    }
}

fn register(r: &mut Registry) {
    register_ab(r);
    r.format("multi").from_cap::<dyn respak_core::capability::Bitmap>(Multi);
}

async fn env() -> Env {
    let env = env_with(register);
    for (path, body) in [("a/1.aa", "AAAA1"), ("a/2.aa", "AAAA2"), ("b/3.aa", "AAAA3"), ("b/skip.bb", "BBBB"), ("loose.dat", "AAAA")] {
        env.input.write(path, body).await.unwrap();
    }
    env
}

async fn run(env: &Env, json: &str) -> Vec<(String, std::result::Result<Vec<String>, &'static str>)> {
    let report = env.engine.convert(&request(json), &respak_core::CancellationToken::new()).await.unwrap();
    report
        .outputs
        .into_iter()
        .map(|o| {
            (
                o.from,
                match o.result {
                    OutputResult::Ok { files } => Ok(files),
                    OutputResult::Err { error } => Err(error.kind().name()),
                },
            )
        })
        .collect()
}

#[tokio::test]
async fn a_glob_gives_one_report_entry_per_match_in_order() {
    let env = env().await;
    let r = run(&env, r#"{"version":1,"outputs":[{"from":"**/*.aa","to":{"type":"beta"}}]}"#).await;
    let froms: Vec<_> = r.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(froms, ["a/1.aa", "a/2.aa", "b/3.aa"]);
    assert!(r.iter().all(|(_, res)| res.is_ok()));
    for out in ["a/1.bb", "a/2.bb", "b/3.bb"] {
        assert!(env.output.exists(out).await.unwrap(), "{out}");
    }
}

#[tokio::test]
async fn a_list_mixes_paths_globs_and_forced_formats() {
    let env = env().await;
    let r = run(
        &env,
        r#"{"version":1,"outputs":[{"from":["a/1.aa","b/*.aa",{"path":"loose.dat","format":"alpha"}],"to":{"type":"beta"}}]}"#,
    )
    .await;
    let froms: Vec<_> = r.iter().map(|(f, _)| f.as_str()).collect();
    assert_eq!(froms, ["a/1.aa", "b/3.aa", "loose.dat"]);
    assert!(r.iter().all(|(_, res)| res.is_ok()), "{r:?}");
    assert!(env.output.exists("loose.bb").await.unwrap());
}

#[tokio::test]
async fn a_forced_format_that_has_no_read_edge_is_no_path() {
    let env = env().await;
    let r = run(&env, r#"{"version":1,"outputs":[{"from":{"path":"loose.dat","format":"nope"},"to":{"type":"beta"}}]}"#).await;
    assert!(r[0].1 == Err("NoPath"), "{r:?}");
}

#[tokio::test]
async fn a_glob_that_matches_nothing_fails_alone() {
    let env = env().await;
    let r = run(
        &env,
        r#"{"version":1,"outputs":[{"from":"zzz/*.aa","to":{"type":"beta"}},{"from":"a/1.aa","to":{"type":"beta"}},{"from":"a/[","to":{"type":"beta"}}]}"#,
    )
    .await;
    assert!(r[0].1 == Err("NoMatch"), "{r:?}");
    assert_eq!(r[0].0, "zzz/*.aa");
    assert!(r[1].1.is_ok());
    // `a/[` has glob syntax but is not a valid one
    assert!(r[2].1 == Err("InvalidRequest"), "{r:?}");
}

#[tokio::test]
async fn an_empty_list_is_an_invalid_request() {
    let env = env().await;
    let r = run(&env, r#"{"version":1,"outputs":[{"from":[],"to":{"type":"beta"}}]}"#).await;
    assert!(r[0].1 == Err("InvalidRequest"), "{r:?}");
}

#[tokio::test]
async fn two_different_values_on_one_path_conflict_but_the_same_one_is_written_once() {
    let env = env().await;
    let r = run(&env, r#"{"version":1,"outputs":[{"from":["a/1.aa","a/2.aa"],"to":{"type":"beta"},"path":"out/x.bb"}]}"#).await;
    let ok = r.iter().filter(|(_, res)| res.is_ok()).count();
    let conflicts = r.iter().filter(|(_, res)| *res == Err("OutputConflict")).count();
    assert_eq!((ok, conflicts), (1, 1), "{r:?}");

    // the same source twice: the second write is skipped, both succeed
    let r = run(&env, r#"{"version":1,"outputs":[{"from":["a/1.aa","a/1.aa"],"to":{"type":"beta"}}]}"#).await;
    assert!(r.iter().all(|(_, res)| res.as_ref().is_ok_and(|f| f == &["a/1.bb"])), "{r:?}");
}

#[tokio::test]
async fn a_composite_output_gets_its_own_folder() {
    let env = env().await;
    let r = run(&env, r#"{"version":1,"outputs":[{"from":"a/*.aa","to":{"type":"multi"}}]}"#).await;
    assert_eq!(r[0].1.as_ref().unwrap(), &["a/1/one.bin", "a/1/two.bin"]);
    assert_eq!(env.output.read("a/1/two.bin").await.unwrap().to_vec(), b"2");
    assert!(env.output.exists("a/2/one.bin").await.unwrap());
}

#[tokio::test]
async fn path_names_the_folder_of_a_composite_and_shared_items_are_written_once() {
    let env = env().await;
    // Both sources produce the same items (same keys) into the same folder: no conflict.
    let r = run(&env, r#"{"version":1,"outputs":[{"from":"a/*.aa","to":{"type":"multi"},"path":"models/m/"}]}"#).await;
    assert!(r.iter().all(|(_, res)| res.is_ok()), "{r:?}");
    assert!(env.output.exists("models/m/one.bin").await.unwrap());
}

#[tokio::test]
async fn max_concurrency_keeps_results_in_request_order() {
    let (input, output) = (mem(), mem());
    let engine = respak_core::Engine::builder()
        .input(input.clone())
        .output(output)
        .plugin(TestPlugin { id: "t", register })
        .max_concurrency(1)
        .build()
        .unwrap();
    for i in 0..5 {
        input.write(&format!("f{i}.aa"), "AAAA").await.unwrap();
    }
    let report = engine.convert(&request(r#"{"version":1,"outputs":[{"from":"*.aa","to":{"type":"beta"}}]}"#), &respak_core::CancellationToken::new()).await.unwrap();
    let froms: Vec<_> = report.outputs.iter().map(|o| o.from.clone()).collect();
    assert_eq!(froms, ["f0.aa", "f1.aa", "f2.aa", "f3.aa", "f4.aa"]);
}

#[tokio::test]
async fn engine_open_takes_a_path_or_a_glob() {
    let env = env().await;
    assert_eq!(env.engine.open("a/*.aa", &respak_core::CancellationToken::new()).await.unwrap().len(), 2);
    assert_eq!(env.engine.open("a/1.aa", &respak_core::CancellationToken::new()).await.unwrap().len(), 1);
    assert!(matches!(env.engine.open("a/*.zz", &respak_core::CancellationToken::new()).await.unwrap_err().kind(), ErrorKind::NoMatch { .. }));
    assert!(matches!(env.engine.open("a/9.aa", &respak_core::CancellationToken::new()).await.unwrap_err().kind(), ErrorKind::NotFound { .. }));
}
