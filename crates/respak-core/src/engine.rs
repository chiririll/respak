//! The public entry point (PROJECT.md §8). Slice 05: single-path `convert` only.

use std::sync::Arc;

use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::discover::{Alternative, DiscoverOptions, Discovery, DiscoveryEntry};
use crate::error::{Error, ErrorKind, Result};
use crate::graph::FormatGraph;
use crate::mediator::{Cx, Mediator, Target};
use crate::plugin::{Plugin, Registry};
use crate::value::{FileData, FormatId, Value};

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
    /// `options.depth` must be 0 until slice 07 (`InvalidRequest` otherwise).
    pub async fn discover(&self, path: &str, options: &DiscoverOptions) -> Result<Discovery> {
        if options.depth > 0 {
            return Err(Error::new(ErrorKind::InvalidRequest {
                message: "discover depth > 0 needs container support, which is not implemented yet".into(),
            }));
        }
        self.input.stat(path).await?;
        let cx = Cx { mediator: self.mediator.clone(), input: self.input.clone(), output: self.output.clone() };
        let source: Value = Arc::new(FileData::new(self.input.clone(), path, path, None));
        let mut hits = self.mediator.detect(&source, &cx).await?.into_iter();
        let version = |f: &FormatId| f.version().map(|v| v.to_string());
        let entry = match hits.next() {
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
                targets: self.mediator.targets_from(winner.format.name()),
            },
            None => DiscoveryEntry {
                path: path.to_owned(),
                format: None,
                version: None,
                score: 0,
                alternatives: Vec::new(),
                name: None,
                targets: Vec::new(),
            },
        };
        Ok(Discovery { path: path.to_owned(), entries: vec![entry] })
    }

    /// Runs every output; one failing output does not stop the others.
    /// `Err` only for an invalid request (unknown `version`).
    pub async fn convert(&self, request: &ConvertRequest) -> Result<ConvertReport> {
        if request.version != 1 {
            return Err(Error::new(ErrorKind::InvalidRequest {
                message: format!("unknown request version {}", request.version),
            }));
        }
        let mut outputs = Vec::with_capacity(request.outputs.len());
        for out in &request.outputs {
            let result = match self.convert_one(out).await {
                Ok(files) => OutputResult::Ok { files },
                Err(error) => OutputResult::Err { error },
            };
            outputs.push(OutputReport { from: out.from.clone(), result });
        }
        Ok(ConvertReport { outputs })
    }

    async fn convert_one(&self, out: &OutputRequest) -> Result<Vec<String>> {
        self.input.stat(&out.from).await?;
        let cx = Cx { mediator: self.mediator.clone(), input: self.input.clone(), output: self.output.clone() };
        let source: Value = Arc::new(FileData::new(self.input.clone(), out.from.clone(), out.from.clone(), None));
        let options = if out.to.options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(out.to.options.clone())
        };
        let target = Target { format: out.to.format.clone(), options, via: out.to.via.clone() };
        let result = self.mediator.convert(source, target, &cx).await?;
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
                with_extension(&out.from, self.mediator.extension_of(name).unwrap_or(name))
            }
        };
        self.output.write(&path, blob.bytes().clone()).await?;
        Ok(vec![path])
    }
}
