//! The public entry point (PROJECT.md §8). Slice 05: single-path `convert` only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::discover::{Alternative, DiscoverOptions, Discovery, DiscoveryEntry, ListedEntry};
use crate::error::{Error, ErrorKind, Result};
use crate::event::{Event, Events, DEFAULT_EVENT_CAPACITY};
use crate::graph::FormatGraph;
use crate::mediator::{Cx, Mediator, Target};
use crate::middleware::Middleware;
use crate::plugin::{Plugin, Registry};
use crate::CancellationToken;
use crate::capability::Container;
use crate::value::{BundleData, FormatId, Value, FILE};

/// Entries read at once by `list` with names.
const NAME_CONCURRENCY: usize = 32;

/// `{ "version": 1, "outputs": [...] }`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConvertRequest {
    /// Request schema version, must be 1.
    pub version: u32,
    /// What to produce.
    pub outputs: Vec<OutputRequest>,
}

/// One output: `{ "from": "icon.png", "to": { "type": "dds" }, "path": "x/icon.dds" }`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OutputRequest {
    /// What to convert: a path, a glob, `{path, format}`, or a list of those.
    pub from: FromSpec,
    /// Target format and options.
    pub to: TargetSpec,
    /// Output path (the folder, for a composite output); default = the path inside the innermost
    /// container with the target's extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// `from`: one source or several.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FromSpec {
    /// One path or glob, or one `{path, format}`.
    One(SourceSpec),
    /// Several.
    Many(Vec<SourceSpec>),
}

/// One source: a path or glob, or a path whose format is given instead of detected (an asset
/// outside any container).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SourceSpec {
    /// A path or glob.
    Path(String),
    /// A path read as `format` (`txtr@10`).
    As {
        /// Path on the input.
        path: String,
        /// The format to read it as.
        format: FormatId,
    },
}

impl From<String> for FromSpec {
    fn from(path: String) -> Self {
        Self::One(SourceSpec::Path(path))
    }
}

impl From<&str> for FromSpec {
    fn from(path: &str) -> Self {
        path.to_owned().into()
    }
}

impl FromSpec {
    fn sources(&self) -> Vec<&SourceSpec> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v.iter().collect(),
        }
    }

    /// The request's own spelling, for the report of a request that expanded to nothing.
    fn label(&self) -> String {
        self.sources()
            .iter()
            .map(|s| match s {
                SourceSpec::Path(p) | SourceSpec::As { path: p, .. } => p.as_str(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// `{ "type": "dds", ...options }`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TargetSpec {
    /// Target format.
    #[serde(rename = "type")]
    pub format: FormatId,
    /// Formats the conversion path must pass through, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
    /// All other fields. Not interpreted in this slice.
    #[serde(flatten)]
    pub options: serde_json::Map<String, serde_json::Value>,
}

/// One entry per output.
#[derive(Debug, Serialize)]
pub struct ConvertReport {
    /// In request order.
    pub outputs: Vec<OutputReport>,
}

/// The outcome of one output.
#[derive(Debug, Serialize)]
pub struct OutputReport {
    /// The source this conversion read (a glob is reported once per match).
    pub from: String,
    /// Files written, or the error.
    pub result: OutputResult,
}

/// A value a converter needed to produce its output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedValue {
    /// Key of the value that was converted.
    pub key: String,
    /// The format it was converted to.
    pub format: String,
}

/// `{ "Ok": { "files": [..] } }` or `{ "Err": { "error": {..} } }`
#[derive(Debug, Serialize)]
pub enum OutputResult {
    /// Success.
    Ok {
        /// Paths written on the output operator.
        files: Vec<String>,
        /// The values this output's converters asked the mediator for directly (e.g. a
        /// material's textures), each once, in request order.
        dependencies: Vec<LinkedValue>,
    },
    /// Failure of this output only.
    Err {
        /// What went wrong.
        error: Error,
    },
}

/// `icon.png` + `dds` -> `icon.dds`; a dot in a directory name is not an extension.
fn with_extension(path: &str, ext: &str) -> String {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => format!("{}.{ext}", &path[..name_start + dot]),
        _ => format!("{path}.{ext}"),
    }
}

/// Builds an [`Engine`].
#[derive(Default)]
pub struct EngineBuilder {
    input: Option<Operator>,
    output: Option<Operator>,
    cache: Option<Operator>,
    plugins: Vec<Box<dyn Plugin>>,
    max_concurrency: Option<usize>,
    middleware: Vec<Arc<dyn Middleware>>,
    blocking_limit: Option<usize>,
    io_limit: Option<usize>,
    event_capacity: Option<usize>,
}

impl EngineBuilder {
    /// How many outputs run at once (default 4, at least 1).
    pub fn max_concurrency(mut self, n: usize) -> Self {
        self.max_concurrency = Some(n.max(1));
        self
    }

    /// Adds host middleware, run after the built-ins in the order added.
    pub fn middleware(mut self, m: impl Middleware + 'static) -> Self {
        self.middleware.push(Arc::new(m));
        self
    }

    /// How many `Cx::blocking` tasks run at once (default: unlimited).
    pub fn blocking_limit(mut self, n: usize) -> Self {
        self.blocking_limit = Some(n);
        self
    }

    /// How many `Cx::io` futures run at once (default: unlimited).
    pub fn io_limit(mut self, n: usize) -> Self {
        self.io_limit = Some(n);
        self
    }

    /// Unread events a subscriber may fall behind by before it misses some (default 1024).
    pub fn event_capacity(mut self, n: usize) -> Self {
        self.event_capacity = Some(n);
        self
    }

    /// Where inputs are read from (required).
    pub fn input(mut self, op: Operator) -> Self {
        self.input = Some(op);
        self
    }
    /// Where outputs are written (required).
    pub fn output(mut self, op: Operator) -> Self {
        self.output = Some(op);
        self
    }
    /// Where plugins keep cached data (optional).
    pub fn cache(mut self, op: Operator) -> Self {
        self.cache = Some(op);
        self
    }
    /// Adds a plugin.
    pub fn plugin(mut self, p: impl Plugin + 'static) -> Self {
        self.plugins.push(Box::new(p));
        self
    }
    /// Fails on a missing operator or a format defined twice.
    pub fn build(self) -> Result<Engine> {
        let missing = |what: &str| {
            Error::new(ErrorKind::Custom { source: None, message: format!("engine has no {what} operator") })
        };
        let input = self.input.ok_or_else(|| missing("input"))?;
        let output = self.output.ok_or_else(|| missing("output"))?;
        let mut registry = Registry::new();
        for p in &self.plugins {
            registry.add(p.as_ref());
        }
        registry.check()?;
        Ok(Engine {
            mediator: Mediator::new(registry)
                .with_events(Events::new(self.event_capacity.unwrap_or(DEFAULT_EVENT_CAPACITY)))
                .with_middleware(self.middleware),
            input,
            output,
            cache: self.cache,
            max_concurrency: self.max_concurrency.unwrap_or(4),
            limits: (self.blocking_limit, self.io_limit),
        })
    }
}

/// Converts requests using the registered plugins.
pub struct Engine {
    mediator: Mediator,
    input: Operator,
    output: Operator,
    cache: Option<Operator>,
    max_concurrency: usize,
    limits: (Option<usize>, Option<usize>),
}

impl Engine {
    /// Starts a builder.
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    /// The format graph, for `respak formats`.
    pub fn formats(&self) -> FormatGraph {
        self.mediator.formats()
    }

    /// A receiver for the events of every call from now on.
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.mediator.subscribe()
    }

    /// Detects what `path` is and which formats it can be converted to, without decoding.
    /// `NotFound` if the path does not exist; an unrecognised file is an entry with no format.
    ///
    /// `depth` is how many container levels to open: 0 describes the item itself; 1 lists the
    /// contents of a container (one entry per item, instead of the container's own entry); and so on.
    pub async fn discover(&self, path: &str, options: &DiscoverOptions, cancel: &CancellationToken) -> Result<Discovery> {
        let cx = self.cx(cancel);
        let value = self.mediator.open(path, &cx).await?;
        let mut entries = Vec::new();
        self.describe(&value, path.trim_matches('/'), options.depth, &cx, &mut entries).await?;
        Ok(Discovery { path: path.to_owned(), entries })
    }

    /// Lists the entries of the container at `path`, sorted, as paths relative to the container
    /// (`assets/some_asset.type`). Without `names` nothing is opened (no detection, no sibling
    /// files). With `names` every entry is read like `discover` does, and its `Metadata` name
    /// (if it has one that differs from the path) is reported too; an entry that cannot be read
    /// is listed without a name. `NotFound` if `path` is not a container.
    pub async fn list(&self, path: &str, names: bool, cancel: &CancellationToken) -> Result<Vec<ListedEntry>> {
        use futures::StreamExt;
        let cx = self.cx(cancel);
        let value = self.mediator.open(path, &cx).await?;
        let container = self.mediator.mount(value, &cx).await?;
        let c = container.as_container().expect("mount checks this");
        let files = list_volume(&c.volume()).await?;
        if !names {
            return Ok(files.into_iter().map(|path| ListedEntry { path, name: None }).collect());
        }
        let (cx, c) = (&cx, c);
        Ok(futures::stream::iter(files)
            .map(|path| async move { ListedEntry { name: self.entry_name(c, &path, cx).await, path } })
            .buffered(NAME_CONCURRENCY)
            .collect()
            .await)
    }

    /// The `Metadata` name of the entry `path`, with the entry's extension (names are stored
    /// without the kind), unless it is just the path again.
    async fn entry_name(&self, c: &dyn Container, path: &str, cx: &Cx) -> Option<String> {
        let mut value = c.entry(path).await.ok()?;
        if value.as_metadata().and_then(|m| m.name()).is_none() {
            value = self.mediator.identify(value, cx).await.ok()?;
        }
        let name = value.as_metadata()?.name()?;
        let name = match path.rsplit_once('.') {
            Some((_, ext)) if !name.ends_with(&format!(".{ext}")) => format!("{name}.{ext}"),
            _ => name.to_owned(),
        };
        (name != path).then_some(name)
    }

    async fn describe(
        &self,
        value: &Value,
        path: &str,
        depth: u32,
        cx: &Cx,
        out: &mut Vec<DiscoveryEntry>,
    ) -> Result<()> {
        let version = |f: &FormatId| f.version().map(|v| v.to_string());
        let mut hits = self.mediator.detect(value, cx).await?.into_iter();
        let mut entry = match hits.next() {
            Some(winner) => DiscoveryEntry {
                path: path.to_owned(),
                format: Some(winner.format.name().to_owned()),
                version: version(&winner.format),
                score: winner.score,
                alternatives: hits
                    .map(|h| Alternative {
                        format: h.format.name().to_owned(),
                        version: version(&h.format),
                        score: h.score,
                    })
                    .collect(),
                name: None,
                kind: None,
                targets: self.mediator.targets_from(winner.format.name()),
            },
            // A typed entry handed out by a container carries its own format.
            None if value.format().name() != FILE => DiscoveryEntry {
                path: path.to_owned(),
                format: Some(value.format().name().to_owned()),
                version: version(value.format()),
                score: 10,
                alternatives: Vec::new(),
                name: None,
                kind: None,
                targets: self.mediator.targets_from(value.format().name()),
            },
            None => unrecognised(path),
        };
        if let Some(meta) = value.as_metadata() {
            entry.name = meta.name().map(str::to_owned);
            entry.kind = meta.kind().map(str::to_owned);
        }

        let opens = depth > 0 && entry.format.as_deref().is_some_and(|f| self.mediator.provides_container(f));
        if !opens {
            out.push(entry);
            return Ok(());
        }
        let container = self.mediator.mount(value.clone(), cx).await?;
        let c = container.as_container().expect("mount checks this");
        let mount_path = c.mount_path().to_owned();
        for rel in list_volume(&c.volume()).await? {
            cx.check_cancelled()?;
            let child_path = format!("{}/{rel}", mount_path.trim_end_matches('/'));
            match c.entry(&rel).await {
                Ok(child) => Box::pin(self.describe(&child, &child_path, depth - 1, cx, out)).await?,
                Err(error) => {
                    cx.warn(format!("cannot describe {child_path}: {error}"));
                    out.push(unrecognised(&child_path));
                }
            }
        }
        Ok(())
    }

    fn cx(&self, cancel: &CancellationToken) -> Cx {
        let cx = Cx::new(self.mediator.clone(), self.input.clone(), self.output.clone()).with_cancel(cancel.clone());
        let cx = match &self.cache {
            Some(op) => cx.with_cache(op.clone()),
            None => cx,
        };
        match self.limits {
            (None, None) => cx,
            (blocking, io) => cx.with_limits(blocking.unwrap_or(usize::MAX), io.unwrap_or(usize::MAX)),
        }
    }

    /// Opens a path or glob on the input: one value per match (`NoMatch` if a glob matches
    /// nothing, `NotFound` for a path that is not there).
    pub async fn open(&self, path_or_glob: &str, cancel: &CancellationToken) -> Result<Vec<Value>> {
        let cx = self.cx(cancel);
        let mut values = Vec::new();
        for path in self.mediator.expand(path_or_glob, &cx).await? {
            values.push(self.mediator.open(&path, &cx).await?);
        }
        Ok(values)
    }

    /// Runs every output, up to `max_concurrency` at a time; one failing output does not stop
    /// the others. A glob or a list in `from` gives one report entry per conversion.
    /// `Err` only for an invalid request (unknown `version`).
    pub async fn convert(&self, request: &ConvertRequest, cancel: &CancellationToken) -> Result<ConvertReport> {
        if request.version != 1 {
            return Err(Error::new(ErrorKind::InvalidRequest {
                message: format!("unknown request version {}", request.version),
            }));
        }
        let cx = self.cx(cancel);

        // Expand every `from` first, in request order, so the report has a stable order.
        let mut jobs: Vec<Job<'_>> = Vec::new();
        for out in &request.outputs {
            let sources = out.from.sources();
            if sources.is_empty() {
                let error = Error::new(ErrorKind::InvalidRequest { message: "`from` is an empty list".into() });
                jobs.push(Job { from: out.from.label(), format: None, out, error: Some(error) });
            }
            for source in sources {
                let (pattern, format) = match source {
                    SourceSpec::Path(p) => (p.as_str(), None),
                    SourceSpec::As { path, format } => (path.as_str(), Some(format)),
                };
                match self.mediator.expand(pattern, &cx).await {
                    Ok(paths) => {
                        jobs.extend(paths.into_iter().map(|from| Job { from, format, out, error: None }));
                    }
                    Err(error) => jobs.push(Job { from: pattern.to_owned(), format, out, error: Some(error) }),
                }
            }
        }

        use futures::StreamExt;
        let (total, done) = (jobs.len() as u64, AtomicU64::new(0));
        let outputs = futures::stream::iter(jobs)
            .map(|job| {
                let (cx, done) = (&cx, &done);
                async move {
                    let scoped = cx.scoped();
                    let result = match job.error {
                        Some(error) => Err(error),
                        None => self.convert_one(job.out, &job.from, job.format, &scoped).await,
                    };
                    let result = match result {
                        Ok(files) => OutputResult::Ok { files, dependencies: scoped.dependencies() },
                        Err(error) => OutputResult::Err { error },
                    };
                    cx.mediator.publish(Event::Progress {
                        task: "convert".into(),
                        done: done.fetch_add(1, Ordering::SeqCst) + 1,
                        total,
                    });
                    OutputReport { from: job.from, result }
                }
            })
            .buffered(self.max_concurrency)
            .collect()
            .await;
        Ok(ConvertReport { outputs })
    }

    async fn convert_one(
        &self,
        out: &OutputRequest,
        from: &str,
        format: Option<&FormatId>,
        cx: &Cx,
    ) -> Result<Vec<String>> {
        cx.check_cancelled()?;
        let mut source = self.mediator.open(from, cx).await?;
        if let Some(format) = format {
            source = self.mediator.identify_as(source, format, cx).await?;
        }
        let options = if out.to.options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(out.to.options.clone())
        };
        let target = Target { format: out.to.format.clone(), options, via: out.to.via.clone() };
        let result = self.mediator.convert(source, target, cx).await?;

        let cannot_write = || {
            Error::new(ErrorKind::Custom {
                source: None,
                message: format!("a `{}` value cannot be written to a file", result.format()),
            })
        };
        // The source's path inside its innermost container (or on the input).
        let inner = cx.mounts.path_in_container(from.trim_matches('/'));
        if let Some(bundle) = result.downcast_ref::<BundleData>() {
            // A composite output gets its own folder.
            let folder = match &out.path {
                Some(p) => p.trim_end_matches('/').to_owned(),
                None => without_extension(inner).to_owned(),
            };
            let mut files = Vec::new();
            for (name, item) in bundle.names.iter().zip(&bundle.items) {
                let blob = item.as_blob().ok_or_else(cannot_write)?;
                let path = format!("{folder}/{name}");
                cx.write_blob(&path, item.key(), blob).await?;
                files.push(path);
            }
            return Ok(files);
        }
        let blob = result.as_blob().ok_or_else(cannot_write)?;
        let path = match &out.path {
            Some(p) => p.clone(),
            // A copy of the source keeps its name.
            None if result.format().name() == crate::value::FILE => inner.to_owned(),
            None => {
                let name = out.to.format.name();
                with_extension(inner, self.mediator.extension_of(name).unwrap_or(name))
            }
        };
        cx.write_blob(&path, result.key(), blob).await?;
        Ok(vec![path])
    }
}

/// One conversion: a concrete source of an output request.
struct Job<'a> {
    from: String,
    format: Option<&'a FormatId>,
    out: &'a OutputRequest,
    /// The source could not be expanded (`NoMatch`, bad glob).
    error: Option<Error>,
}

/// `texture/foo.txtr` -> `texture/foo`.
fn without_extension(path: &str) -> &str {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => &path[..name_start + dot],
        _ => path,
    }
}

fn unrecognised(path: &str) -> DiscoveryEntry {
    DiscoveryEntry {
        path: path.to_owned(),
        format: None,
        version: None,
        score: 0,
        alternatives: Vec::new(),
        name: None,
        kind: None,
        targets: Vec::new(),
    }
}

/// Every file in a volume, as paths relative to it, sorted.
pub(crate) async fn list_volume(op: &Operator) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut dirs = vec!["/".to_owned()];
    while let Some(dir) = dirs.pop() {
        for e in op.list(&dir).await? {
            if e.path() == dir {
                continue;
            }
            if e.metadata().is_dir() {
                dirs.push(e.path().to_owned());
            } else {
                files.push(e.path().trim_start_matches('/').to_owned());
            }
        }
    }
    files.sort();
    Ok(files)
}
