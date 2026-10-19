//! A session: one `Engine` built from the saved settings (docs/design/webui.md section 4).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use opendal::{services::Memory, Operator};
use respak_core::{CancellationToken, Engine, Events, FormatGraph, ListedEntry};
use respak_host::{build_engine, build_engine_with, open_input, OpenedInput, Settings, Source};
use serde::Serialize;
use tokio::sync::{broadcast, OnceCell};

use crate::events::UiEvent;

/// One entry of a listed container: the path to open, and the path to show.
#[derive(Debug, Clone)]
pub struct Item {
    /// Relative to the container; goes into virtual paths.
    pub path: String,
    /// The name the container knows (`texture/foo.txtr`), else `path`.
    pub display: String,
}

/// The entries of one container.
#[derive(Debug)]
pub struct Listing {
    /// Sorted by display path.
    pub items: Vec<Item>,
    /// Whether names were read (slow) when it was listed.
    pub named: bool,
}

/// A file of the input, for name search.
#[derive(Debug)]
pub struct IndexedFile {
    /// Virtual path.
    pub path: String,
    /// Lower case, to match against.
    pub lower: String,
}

/// Converted previews kept for a moment: switching a mip back and forth, or fetching the two
/// halves of an atlas, must not convert twice.
#[derive(Default)]
struct PreviewCache {
    order: VecDeque<String>,
    entries: HashMap<String, Arc<Vec<(String, Bytes)>>>,
    bytes: usize,
}

const PREVIEW_ENTRIES: usize = 12;
const PREVIEW_BYTES: usize = 64 << 20;

/// Everything built from the settings.
pub struct Session {
    /// What it was built from.
    pub settings: Settings,
    /// Does the real work; writes to the output directory.
    pub engine: Arc<Engine>,
    /// Converts previews into memory.
    pub preview: Arc<Engine>,
    preview_out: Operator,
    preview_seq: AtomicU64,
    /// The input.
    pub input: Operator,
    /// The output directory, for downloads.
    pub output: Operator,
    opened: OpenedInput,
    /// Cancelled when the session ends; jobs hold children of it.
    pub cancel: CancellationToken,
    /// Extensions (lower case, no dot) of the formats that are containers.
    pub containers: HashSet<String>,
    /// Every registered format.
    pub graph: FormatGraph,
    listings: Mutex<HashMap<String, Arc<Listing>>>,
    files: OnceCell<Arc<Vec<IndexedFile>>>,
    previews: Mutex<PreviewCache>,
}

/// What a session says about itself.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    /// `root` or `steam`.
    pub source: &'static str,
    /// The input directory, for `root`.
    pub root: Option<String>,
    /// The output directory.
    pub out: String,
    /// Steam app and depots, for `steam`.
    pub steam: Option<serde_json::Value>,
    /// Extensions of container formats.
    pub containers: Vec<String>,
    /// Formats something converts into: the possible targets.
    pub formats: Vec<FormatLabel>,
}

/// A format and its label.
#[derive(Debug, Serialize)]
pub struct FormatLabel {
    /// Format name.
    pub name: String,
    /// Human-readable label.
    pub label: String,
}

impl Session {
    /// Opens the input and builds the engines. Events worth showing are sent to `ui`.
    pub async fn connect(
        settings: Settings,
        ui: broadcast::Sender<UiEvent>,
    ) -> anyhow::Result<Arc<Self>> {
        let cancel = CancellationToken::new();
        let steam_events = Events::new(respak_core::event::DEFAULT_EVENT_CAPACITY);
        crate::events::forward(steam_events.subscribe(), ui.clone(), cancel.clone());
        let opened = open_input(&settings, &cancel, &steam_events).await?;
        let input = opened.operator.clone();
        let engine = Arc::new(build_engine(&settings, input.clone())?);
        let preview_out = Operator::new(Memory::default())?;
        let preview = Arc::new(build_engine_with(
            &settings,
            input.clone(),
            preview_out.clone(),
        )?);
        crate::events::forward(engine.events(), ui, cancel.clone());
        let graph = engine.formats();
        let containers = graph
            .formats
            .iter()
            .filter(|f| f.provides.iter().any(|c| c == "Container"))
            .flat_map(|f| {
                f.extensions
                    .iter()
                    .map(|e| e.trim_start_matches('.').to_lowercase())
            })
            .collect();
        let output = respak_host::fs(&settings.out)?;
        Ok(Arc::new(Self {
            settings,
            engine,
            preview,
            preview_out,
            preview_seq: AtomicU64::new(0),
            input,
            output,
            opened,
            cancel,
            containers,
            graph,
            listings: Mutex::default(),
            files: OnceCell::new(),
            previews: Mutex::default(),
        }))
    }

    /// What to tell the page.
    pub fn info(&self) -> SessionInfo {
        let mut containers: Vec<String> = self.containers.iter().cloned().collect();
        containers.sort();
        let (source, root, steam) = match &self.settings.source {
            Source::Root(root) => ("root", Some(root.display().to_string()), None),
            #[cfg(feature = "steam")]
            Source::Steam(steam) => (
                "steam",
                None,
                Some(serde_json::json!({
                    "app": steam.app,
                    "branch": steam.branch,
                    "depots": steam.depots.iter().map(|d| match d.manifest {
                        Some(m) => format!("{}={m}", d.depot),
                        None => d.depot.to_string(),
                    }).collect::<Vec<_>>(),
                })),
            ),
        };
        SessionInfo {
            source,
            root,
            out: self.settings.out.display().to_string(),
            steam,
            containers,
            formats: self
                .graph
                .formats
                .iter()
                .filter(|f| f.name != "file" && !f.produced_by.is_empty())
                .map(|f| FormatLabel {
                    name: f.name.clone(),
                    label: f.label.clone(),
                })
                .collect(),
        }
    }

    /// Download statistics of a Steam input.
    #[cfg(feature = "steam")]
    pub fn steam_stats(&self) -> Option<serde_json::Value> {
        self.opened.steam_stats().map(|s| {
            serde_json::json!({
                "chunksDownloaded": s.chunks_downloaded,
                "bytesDownloaded": s.bytes_downloaded,
                "cacheHits": s.cache_hits,
            })
        })
    }

    /// Download statistics of a Steam input (none in this build).
    #[cfg(not(feature = "steam"))]
    pub fn steam_stats(&self) -> Option<serde_json::Value> {
        let _ = &self.opened;
        None
    }

    /// Whether `name` has the extension of a container format.
    pub fn is_container_name(&self, name: &str) -> bool {
        extension(name).is_some_and(|ext| self.containers.contains(&ext))
    }

    /// The entries of the container at `path`, listed once; with `named`, listed again if the
    /// cached listing has no names.
    pub async fn listing(&self, path: &str, named: bool) -> respak_core::Result<Arc<Listing>> {
        if let Some(found) = self.listings.lock().unwrap().get(path) {
            if found.named || !named {
                return Ok(found.clone());
            }
        }
        let entries = self.engine.list(path, named, &self.cancel).await?;
        let mut items: Vec<Item> = entries.into_iter().map(item).collect();
        items.sort_by_key(|a| a.display.to_lowercase());
        let listing = Arc::new(Listing { items, named });
        self.listings
            .lock()
            .unwrap()
            .insert(path.to_owned(), listing.clone());
        Ok(listing)
    }

    /// The containers listed so far, with their listings.
    pub fn loaded(&self) -> Vec<(String, Arc<Listing>)> {
        let mut all: Vec<_> = self
            .listings
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// Every file of the input, listed once.
    pub async fn file_index(&self) -> anyhow::Result<Arc<Vec<IndexedFile>>> {
        self.files
            .get_or_try_init(|| async {
                let mut files = Vec::new();
                let mut dirs = vec!["/".to_owned()];
                while let Some(dir) = dirs.pop() {
                    for e in self.input.list(&dir).await? {
                        if e.path() == dir {
                            continue;
                        }
                        if e.metadata().is_dir() {
                            dirs.push(e.path().to_owned());
                        } else {
                            let path = e.path().trim_start_matches('/').to_owned();
                            files.push(IndexedFile {
                                lower: path.to_lowercase(),
                                path,
                            });
                        }
                    }
                }
                files.sort_by(|a, b| a.lower.cmp(&b.lower));
                anyhow::Ok(Arc::new(files))
            })
            .await
            .cloned()
    }

    /// Converts `from` into `to` on the preview engine, and returns the files it wrote as
    /// `(path relative to the output folder, bytes)`. Nothing reaches the output directory.
    pub async fn preview(
        &self,
        from: &str,
        to: respak_core::TargetSpec,
        composite: bool,
    ) -> respak_core::Result<Arc<Vec<(String, Bytes)>>> {
        let key = format!(
            "{from}\u{0}{}",
            serde_json::to_string(&to).unwrap_or_default()
        );
        if let Some(hit) = self.previews.lock().unwrap().entries.get(&key) {
            return Ok(hit.clone());
        }
        let n = self.preview_seq.fetch_add(1, Ordering::Relaxed);
        let ext = to.format.name().to_owned();
        let out = if composite {
            format!("p{n}")
        } else {
            format!("p{n}.{ext}")
        };
        let request = respak_core::ConvertRequest::new(vec![respak_core::OutputRequest {
            from: from.into(),
            to,
            path: Some(out.clone()),
            skip_unsupported: false,
        }]);
        let report = detached_convert(self.preview.clone(), request, self.cancel.clone()).await?;
        let mut written = Vec::new();
        for output in report.outputs {
            match output.result {
                respak_core::OutputResult::Ok { files, .. } => written.extend(files),
                respak_core::OutputResult::Err { error } => {
                    self.discard(&out).await;
                    return Err(error);
                }
            }
        }
        let mut files = Vec::new();
        for path in written {
            let data = self
                .preview_out
                .read(&path)
                .await
                .map_err(|e| respak_core::Error::custom("preview", e))?
                .to_bytes();
            let relative = path
                .strip_prefix(&format!("{out}/"))
                .unwrap_or(&path)
                .to_owned();
            files.push((relative, data));
        }
        self.discard(&out).await;
        let files = Arc::new(files);
        self.remember(key, files.clone());
        Ok(files)
    }

    /// Forgets what a preview wrote (a file, or a folder of files) from the memory output.
    async fn discard(&self, out: &str) {
        let _ = self.preview_out.delete(out).await;
        let _ = self
            .preview_out
            .delete_with(&format!("{out}/"))
            .recursive(true)
            .await;
    }

    fn remember(&self, key: String, files: Arc<Vec<(String, Bytes)>>) {
        let mut cache = self.previews.lock().unwrap();
        cache.bytes += files.iter().map(|(_, b)| b.len()).sum::<usize>();
        cache.order.push_back(key.clone());
        cache.entries.insert(key, files);
        while cache.order.len() > PREVIEW_ENTRIES || cache.bytes > PREVIEW_BYTES {
            let Some(old) = cache.order.pop_front() else {
                break;
            };
            if let Some(gone) = cache.entries.remove(&old) {
                cache.bytes -= gone.iter().map(|(_, b)| b.len()).sum::<usize>();
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

fn item(entry: ListedEntry) -> Item {
    Item {
        display: entry.name.unwrap_or_else(|| entry.path.clone()),
        path: entry.path,
    }
}

/// The lower-case extension of the last segment of `name`.
pub fn extension(name: &str) -> Option<String> {
    let last = name.rsplit('/').next().unwrap_or(name);
    let (stem, ext) = last.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_lowercase())
}

/// Runs `engine.convert` on a thread of its own and waits for the answer. The engine's future is
/// not provably `Send` (a closure inside `convert` is not general over its lifetimes), so it
/// cannot be awaited in a request handler directly; driving it in place on another thread can.
pub async fn detached_convert(
    engine: Arc<Engine>,
    request: respak_core::ConvertRequest,
    cancel: CancellationToken,
) -> respak_core::Result<respak_core::ConvertReport> {
    let handle = tokio::runtime::Handle::current();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let report = handle.block_on(async { engine.convert(&request, &cancel).await });
        let _ = tx.send(report);
    });
    rx.await.unwrap_or_else(|_| {
        Err(respak_core::Error::msg(
            "the conversion stopped unexpectedly",
        ))
    })
}
