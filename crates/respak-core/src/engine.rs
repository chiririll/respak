//! The public entry point (PROJECT.md §8). Slice 05: single-path `convert` only.

use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::discover::{Alternative, DiscoverOptions, Discovery, DiscoveryEntry};
use crate::error::{Error, ErrorKind, Result};
use crate::graph::FormatGraph;
use crate::mediator::{Cx, Mediator, Target};
use crate::plugin::{Plugin, Registry};
use crate::value::{FormatId, Value, FILE};

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
    /// Source path on the input operator. (Later: glob, list, `{path, format}`; the JSON
    /// stays compatible.)
    pub from: String,
    /// Target format and options.
    pub to: TargetSpec,
    /// Output path; default = source path with the target's extension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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
    /// The request's `from`.
    pub from: String,
    /// Files written, or the error.
    pub result: OutputResult,
}

/// `{ "Ok": { "files": [..] } }` or `{ "Err": { "error": {..} } }`
#[derive(Debug, Serialize)]
pub enum OutputResult {
    /// Success.
    Ok {
        /// Paths written on the output operator.
        files: Vec<String>,
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
    plugins: Vec<Box<dyn Plugin>>,
}

impl EngineBuilder {
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
        Ok(Engine { mediator: Mediator::new(registry), input, output })
    }
}

/// Converts requests using the registered plugins.
pub struct Engine {
    mediator: Mediator,
    input: Operator,
    output: Operator,
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

    /// Detects what `path` is and which formats it can be converted to, without decoding.
    /// `NotFound` if the path does not exist; an unrecognised file is an entry with no format.
    ///
    /// `depth` is how many container levels to open: 0 describes the item itself; 1 lists the
    /// contents of a container (one entry per item, instead of the container's own entry); and so on.
    pub async fn discover(&self, path: &str, options: &DiscoverOptions) -> Result<Discovery> {
        let cx = self.cx();
        let value = self.mediator.open(path, &cx).await?;
        let mut entries = Vec::new();
        self.describe(&value, path.trim_matches('/'), options.depth, &cx, &mut entries).await?;
        Ok(Discovery { path: path.to_owned(), entries })
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
            let child_path = format!("{}/{rel}", mount_path.trim_end_matches('/'));
            match c.entry(&rel).await {
                Ok(child) => Box::pin(self.describe(&child, &child_path, depth - 1, cx, out)).await?,
                Err(error) => {
                    tracing::warn!("cannot describe {child_path}: {error}");
                    out.push(unrecognised(&child_path));
                }
            }
        }
        Ok(())
    }

    fn cx(&self) -> Cx {
        Cx::new(self.mediator.clone(), self.input.clone(), self.output.clone())
    }

    /// Runs every output; one failing output does not stop the others.
    /// `Err` only for an invalid request (unknown `version`).
    pub async fn convert(&self, request: &ConvertRequest) -> Result<ConvertReport> {
        if request.version != 1 {
            return Err(Error::new(ErrorKind::InvalidRequest {
                message: format!("unknown request version {}", request.version),
            }));
        }
        let cx = self.cx();
        let mut outputs = Vec::with_capacity(request.outputs.len());
        for out in &request.outputs {
            let result = match self.convert_one(out, &cx).await {
                Ok(files) => OutputResult::Ok { files },
                Err(error) => OutputResult::Err { error },
            };
            outputs.push(OutputReport { from: out.from.clone(), result });
        }
        Ok(ConvertReport { outputs })
    }

    async fn convert_one(&self, out: &OutputRequest, cx: &Cx) -> Result<Vec<String>> {
        let source = self.mediator.open(&out.from, cx).await?;
        let options = if out.to.options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(out.to.options.clone())
        };
        let target = Target { format: out.to.format.clone(), options, via: out.to.via.clone() };
        let result = self.mediator.convert(source, target, cx).await?;
        let blob = result.as_blob().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: format!("a `{}` value cannot be written to a file", result.format()),
            })
        })?;
        let path = match &out.path {
            Some(p) => p.clone(),
            None => {
                let name = out.to.format.name();
                let inner = cx.mounts.path_in_container(out.from.trim_matches('/'));
                with_extension(inner, self.mediator.extension_of(name).unwrap_or(name))
            }
        };
        self.output.write(&path, blob.bytes().clone()).await?;
        Ok(vec![path])
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
async fn list_volume(op: &Operator) -> Result<Vec<String>> {
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
