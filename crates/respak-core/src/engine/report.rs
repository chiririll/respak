//! Report types of [`Engine::convert`](super::Engine::convert).

use serde::Serialize;

use crate::error::Error;
use crate::mediator::LinkedValue;

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
