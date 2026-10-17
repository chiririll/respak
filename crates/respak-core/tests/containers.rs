//! Virtual paths and `discover --depth` on a toy container format `pak`.
//!
//! A pak file is `PAK1` followed by entries `path:content`, separated by `\n` (or by `|` if the first character after the magic is `|`, so
//! that a pak can be nested in another pak's entry).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use common::*;
use opendal::Operator;
use respak_core::capability::{Container, Metadata};
use respak_core::value::FileData;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{
    magic, Cx, Data, Detect, Detected, DiscoverOptions, ErrorKind, FormatId, Probe, ReadHandler,
    Registry, Result, Value, ValueKey, Version,
};

struct MapVolume(BTreeMap<String, Bytes>);

#[async_trait]
impl VolumeSource for MapVolume {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() || self.0.keys().any(|k| k.starts_with(&format!("{path}/"))) {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        match self.0.get(path) {
            Some(b) => Ok(VolumeStat {
                is_dir: false,
                len: b.len() as u64,
            }),
            None => Err(respak_core::Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })),
        }
    }
    async fn read(&self, path: &str, range: std::ops::Range<u64>) -> Result<Bytes> {
        Ok(self.0[path].slice(range.start as usize..range.end as usize))
    }
    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<VolumeEntry> = Vec::new();
        for (k, v) in self.0.iter().filter(|(k, _)| k.starts_with(&prefix)) {
            let rest = &k[prefix.len()..];
            let (name, is_dir) = rest
                .split_once('/')
                .map_or((rest, false), |(d, _)| (d, true));
            if !out.iter().any(|e| e.name == name) {
                out.push(VolumeEntry {
                    name: name.to_owned(),
                    is_dir,
                    len: if is_dir { 0 } else { v.len() as u64 },
                });
            }
        }
        Ok(out)
    }
}

/// A typed entry, like `rpak.asset`.
#[derive(Debug)]
struct Item {
    format: FormatId,
    key: ValueKey,
    name: String,
}

impl Metadata for Item {
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn kind(&self) -> Option<&str> {
        Some("dat")
    }
    fn guid(&self) -> Option<u64> {
        None
    }
    fn dependencies(&self) -> Vec<respak_core::capability::Dependency> {
        Vec::new()
    }
}

impl Data for Item {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

#[derive(Debug)]
struct Pak {
    format: FormatId,
    key: ValueKey,
    mount: String,
    volume: Operator,
}

#[async_trait]
impl Container for Pak {
    fn volume(&self) -> Operator {
        self.volume.clone()
    }
    fn mount_path(&self) -> &str {
        &self.mount
    }
    /// `*.dat` paths are typed entries and resolve even when not listed; the rest are files.
    async fn entry(&self, path: &str) -> Result<Value> {
        let virtual_path = format!("{}/{path}", self.mount);
        if path.ends_with(".dat") {
            let format = FormatId::new("pak.item").with_version(Version::new(7));
            return Ok(Arc::new(Item {
                format,
                key: ValueKey::file(virtual_path),
                name: path.to_owned(),
            }));
        }
        self.volume.stat(path).await?;
        Ok(Arc::new(FileData::new(
            self.volume.clone(),
            path,
            virtual_path,
            None,
        )))
    }
}

impl Data for Pak {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

struct PakRead;

#[async_trait]
impl Detect for PakRead {
    async fn score(&self, input: &Probe<'_>, _: &Cx) -> Result<Option<Detected>> {
        Ok(magic(input.header, 0, b"PAK1").then_some(Detected {
            score: 10,
            version: None,
        }))
    }
    fn header_len(&self) -> usize {
        4
    }
}

#[async_trait]
impl ReadHandler for PakRead {
    async fn read(&self, source: Value, _: &Detected, _: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().unwrap();
        let body = file.op.read(&file.path).await?.to_vec();
        let text = String::from_utf8(body[4..].to_vec()).unwrap();
        let mut entries = BTreeMap::new();
        let sep = if text.starts_with('|') { '|' } else { '\n' };
        for line in text.split(sep).filter(|l| !l.is_empty()) {
            let (path, content) = line.split_once(':').unwrap();
            entries.insert(path.to_owned(), Bytes::from(content.to_owned()));
        }
        let mount = source.key().to_string();
        let volume = volume_operator(MapVolume(entries), &mount);
        Ok(Arc::new(Pak {
            format: FormatId::new("pak"),
            key: source.key().clone(),
            mount,
            volume,
        }))
    }
}

fn register(r: &mut Registry) {
    r.format("pak")
        .from("file", PakRead)
        .extensions(["pak"])
        .provides::<dyn Container>();
    r.format("pak.item");
}

async fn fixture() -> Env {
    let env = env_with(register);
    let outer = "PAK1\ninner.pak:PAK1|a/x.dat:hi|a/y.bin:raw\nother.txt:yo\nz.dat:zz";
    env.input.write("outer.pak", outer).await.unwrap();
    env.input.write("plain.txt", "just text").await.unwrap();
    env
}

fn cx(env: &Env) -> Cx {
    // A fresh context, as one `Engine` call would have.
    let mut r = Registry::new();
    r.add(&TestPlugin { id: "t", register });
    Cx::new(
        respak_core::Mediator::new(r),
        env.input.clone(),
        env.output.clone(),
    )
}

#[tokio::test]
async fn paths_resolve_into_nested_containers_and_mounts_are_kept() {
    let env = fixture().await;
    let cx = cx(&env);
    let item = cx
        .mediator
        .open("outer.pak/inner.pak/a/x.dat", &cx)
        .await
        .unwrap();
    assert_eq!(item.format().to_string(), "pak.item@7");
    assert_eq!(item.as_metadata().unwrap().name(), Some("a/x.dat"));
    assert_eq!(cx.mounts.len(), 2, "outer and inner are mounted");

    cx.mediator.open("outer.pak/z.dat", &cx).await.unwrap();
    assert_eq!(cx.mounts.len(), 2, "mounted volumes are reused");
}

#[tokio::test]
async fn an_unlisted_entry_still_resolves_through_the_container() {
    let env = fixture().await;
    let cx = cx(&env);
    let item = cx.mediator.open("outer.pak/0x1234.dat", &cx).await.unwrap();
    assert_eq!(item.as_metadata().unwrap().name(), Some("0x1234.dat"));
}

#[tokio::test]
async fn a_plain_file_inside_a_container_is_a_file_value() {
    let env = fixture().await;
    let cx = cx(&env);
    let v = cx.mediator.open("outer.pak/other.txt", &cx).await.unwrap();
    assert_eq!(v.format().name(), "file");
    assert_eq!(v.key().to_string(), "outer.pak/other.txt");
}

#[tokio::test]
async fn a_real_file_wins_over_a_container_entry() {
    let env = fixture().await;
    env.input.write("outer.pak/z.dat", "real").await.unwrap();
    let cx = cx(&env);
    let v = cx.mediator.open("outer.pak/z.dat", &cx).await.unwrap();
    assert_eq!(v.format().name(), "file");
}

#[tokio::test]
async fn missing_paths_and_non_containers_are_not_found() {
    let env = fixture().await;
    let cx = cx(&env);
    for path in [
        "nope.pak",
        "plain.txt/x.dat",
        "outer.pak/missing.bin/deeper",
        "",
    ] {
        let err = cx.mediator.open(path, &cx).await.unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::NotFound { .. }),
            "{path}: {err}"
        );
    }
}

#[tokio::test]
async fn discover_opens_containers_by_depth() {
    let env = fixture().await;
    let paths =
        |d: &respak_core::Discovery| d.entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>();

    let d0 = env
        .engine
        .discover(
            "outer.pak",
            &DiscoverOptions {
                depth: 0,
                ..Default::default()
            },
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(paths(&d0), ["outer.pak"]);
    assert_eq!(d0.entries[0].format.as_deref(), Some("pak"));

    let d1 = env
        .engine
        .discover(
            "outer.pak",
            &DiscoverOptions {
                depth: 1,
                ..Default::default()
            },
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        paths(&d1),
        [
            "outer.pak/inner.pak",
            "outer.pak/other.txt",
            "outer.pak/z.dat"
        ]
    );
    assert_eq!(
        d1.entries[0].format.as_deref(),
        Some("pak"),
        "a nested pak is not opened at depth 1"
    );
    assert_eq!(d1.entries[1].format, None);
    let z = &d1.entries[2];
    assert_eq!(
        (
            z.format.as_deref(),
            z.version.as_deref(),
            z.kind.as_deref(),
            z.name.as_deref()
        ),
        (Some("pak.item"), Some("7"), Some("dat"), Some("z.dat"))
    );

    let d2 = env
        .engine
        .discover(
            "outer.pak",
            &DiscoverOptions {
                depth: 2,
                ..Default::default()
            },
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        paths(&d2),
        [
            "outer.pak/inner.pak/a/x.dat",
            "outer.pak/inner.pak/a/y.bin",
            "outer.pak/other.txt",
            "outer.pak/z.dat"
        ]
    );
}

#[tokio::test]
async fn discover_accepts_a_path_inside_a_container() {
    let env = fixture().await;
    let d = env
        .engine
        .discover(
            "outer.pak/inner.pak/a/x.dat",
            &DiscoverOptions::default(),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(d.entries[0].path, "outer.pak/inner.pak/a/x.dat");
    assert_eq!(d.entries[0].format.as_deref(), Some("pak.item"));
}

async fn globbed(env: &Env, pattern: &str) -> Result<Vec<String>> {
    Ok(env
        .engine
        .open(pattern, &respak_core::CancellationToken::new())
        .await?
        .iter()
        .map(|v| v.key().to_string())
        .collect())
}

#[tokio::test]
async fn globs_descend_into_a_container_only_where_a_segment_names_it() {
    let env = fixture().await;
    assert_eq!(
        globbed(&env, "outer.pak/*.dat").await.unwrap(),
        ["outer.pak/z.dat"]
    );
    assert_eq!(
        globbed(&env, "*.pak/*.pak/**/*.dat").await.unwrap(),
        ["outer.pak/inner.pak/a/x.dat"]
    );
    assert_eq!(
        globbed(&env, "outer.pak/inner.pak/a/*").await.unwrap(),
        ["outer.pak/inner.pak/a/x.dat", "outer.pak/inner.pak/a/y.bin"]
    );
}

#[tokio::test]
async fn double_star_alone_never_enters_a_container() {
    let env = fixture().await;
    // `**` walks real directories only: outer.pak is a file, so nothing is opened ...
    let err = globbed(&env, "**/*.dat").await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NoMatch { .. }), "{err}");
    // ... and inside outer.pak it does not enter inner.pak either.
    assert_eq!(
        globbed(&env, "outer.pak/**/*.dat").await.unwrap(),
        ["outer.pak/z.dat"]
    );
    // a pattern that ends in `**` lists only real files
    assert_eq!(
        globbed(&env, "**").await.unwrap(),
        ["outer.pak", "plain.txt"]
    );
}

#[tokio::test]
async fn a_pattern_through_a_file_that_is_not_a_container_matches_nothing() {
    let env = fixture().await;
    let err = globbed(&env, "plain.txt/*.dat").await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NoMatch { .. }), "{err}");
}

#[tokio::test]
async fn mounting_a_container_publishes_input_opened() {
    let env = fixture().await;
    let mut rx = env.engine.events();
    env.engine
        .discover(
            "outer.pak",
            &DiscoverOptions {
                depth: 1,
                ..Default::default()
            },
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut opened = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if let respak_core::Event::InputOpened { path, entries } = e {
            opened.push((path, entries));
        }
    }
    assert_eq!(opened, [("outer.pak".to_owned(), 3)]);
}

#[tokio::test]
async fn list_gives_sorted_entries_and_names_only_on_request() {
    let env = fixture().await;
    let token = respak_core::CancellationToken::new();

    let plain = env.engine.list("outer.pak", false, &token).await.unwrap();
    let paths: Vec<_> = plain.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["inner.pak", "other.txt", "z.dat"]);
    assert!(plain.iter().all(|e| e.name.is_none()));

    let named = env.engine.list("outer.pak", true, &token).await.unwrap();
    let paths: Vec<_> = named.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["inner.pak", "other.txt", "z.dat"], "order is kept");
    // A name equal to the path is not reported.
    assert!(named.iter().all(|e| e.name.is_none()));

    let err = env
        .engine
        .list("plain.txt", false, &token)
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::NotFound { .. }), "{err}");
}

async fn inspected_paths(env: &Env) -> Vec<String> {
    let token = respak_core::CancellationToken::new();
    let infos = env.engine.inspect("outer.pak", &token).await.unwrap();
    let z = infos.iter().find(|i| i.path == "z.dat").unwrap();
    assert_eq!(z.name.as_deref(), Some("z.dat"));
    assert_eq!(z.kind.as_deref(), Some("dat"));
    infos.into_iter().map(|i| i.path).collect()
}

#[tokio::test]
async fn inspect_keeps_the_container_order_on_one_thread() {
    let env = fixture().await;
    assert_eq!(
        inspected_paths(&env).await,
        ["inner.pak", "other.txt", "z.dat"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspect_keeps_the_container_order_across_worker_threads() {
    let env = fixture().await;
    assert_eq!(
        inspected_paths(&env).await,
        ["inner.pak", "other.txt", "z.dat"]
    );
}

#[tokio::test]
async fn inspect_stops_when_cancelled() {
    let env = fixture().await;
    let token = respak_core::CancellationToken::new();
    token.cancel();
    let err = env.engine.inspect("outer.pak", &token).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
}
