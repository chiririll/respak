//! The mediator: detection, path search and conversion (PROJECT.md §5.5). No middleware yet.

use std::sync::Arc;

use opendal::Operator;

use crate::error::Result;
use crate::plugin::Registry;
use crate::value::{FormatId, Value};

/// What to convert to: a format and its options.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Target format.
    pub format: FormatId,
    /// Target options, `Null` if none. Unused in this slice.
    pub options: serde_json::Value,
}

/// Detects formats and converts values along the cheapest path.
#[derive(Clone)]
pub struct Mediator {
    registry: Arc<Registry>,
}

impl Mediator {
    /// A mediator over a checked registry.
    pub fn new(registry: Registry) -> Self {
        todo!()
    }

    /// Scores read edges from `source`'s format and applies the winner.
    ///
    /// Extension candidates are scored first; if none matches, all read edges are. Highest
    /// score wins; a tie is `AmbiguousFormat`, no match `UnknownFormat`.
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value> {
        todo!()
    }

    /// Identifies `source`, then follows the cheapest path to `to.format`
    /// (`NoPath` if there is none). A source already in the target format is returned as is.
    pub async fn convert(&self, source: Value, to: Target, cx: &Cx) -> Result<Value> {
        todo!()
    }

    /// The extension output files of `format` get (its first registered extension).
    pub fn extension_of(&self, format: &str) -> Option<&str> {
        todo!()
    }
}

/// Per-call context handed to handlers.
#[derive(Clone)]
pub struct Cx {
    /// For handlers that need another conversion.
    pub mediator: Mediator,
    /// Where inputs live.
    pub input: Operator,
    /// Where outputs go.
    pub output: Operator,
}
