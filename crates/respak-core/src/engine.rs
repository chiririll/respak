//! The public entry point (PROJECT.md §8). Slice 05: single-path `convert` only.

use opendal::Operator;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::mediator::Mediator;
use crate::plugin::Plugin;
use crate::value::FormatId;

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

/// Builds an [`Engine`].
#[derive(Default)]
pub struct EngineBuilder {
    input: Option<Operator>,
    output: Option<Operator>,
    plugins: Vec<Box<dyn Plugin>>,
}

impl EngineBuilder {
    /// Where inputs are read from (required).
    pub fn input(self, op: Operator) -> Self {
        todo!()
    }
    /// Where outputs are written (required).
    pub fn output(self, op: Operator) -> Self {
        todo!()
    }
    /// Adds a plugin.
    pub fn plugin(self, p: impl Plugin + 'static) -> Self {
        todo!()
    }
    /// Fails on a missing operator or a format defined twice.
    pub fn build(self) -> Result<Engine> {
        todo!()
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

    /// Runs every output; one failing output does not stop the others.
    /// `Err` only for an invalid request (unknown `version`).
    pub async fn convert(&self, request: &ConvertRequest) -> Result<ConvertReport> {
        todo!()
    }
}
