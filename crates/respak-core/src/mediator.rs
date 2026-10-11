//! The mediator: detection, path search and conversion (PROJECT.md §5.5). No middleware yet.

use std::sync::Arc;

use opendal::Operator;

use crate::detect::{Detected, Probe};
use crate::error::{Error, ErrorKind, Result};
use crate::plugin::{CapEdge, FormatDef, ReadEdge, Registry};
use crate::value::{FileData, FormatId, Value, ValueKey};

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

type Candidate<'a> = (&'a FormatDef, &'a ReadEdge);

impl Mediator {
    /// A mediator over a checked registry.
    pub fn new(registry: Registry) -> Self {
        Self { registry: Arc::new(registry) }
    }

    fn candidates(&self, from: &str) -> Vec<Candidate<'_>> {
        self.registry
            .formats
            .iter()
            .flat_map(|f| f.read_edges.iter().filter(|e| e.from == from).map(move |e| (f, e)))
            .collect()
    }

    /// Scores read edges from `source`'s format and applies the winner.
    ///
    /// Extension candidates are scored first; if none matches, all read edges are. Highest
    /// score wins; a tie is `AmbiguousFormat`, no match `UnknownFormat`.
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value> {
        let candidates = self.candidates(source.format().name());
        let header_len = candidates.iter().map(|(_, e)| e.handler.header_len()).max().unwrap_or(0);
        let (header, len) = probe_data(&source, header_len).await?;
        let extension = extension_of_key(source.key());
        let probe = Probe { source: &source, extension: extension.as_deref(), len, header: &header };

        let (by_ext, others): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .partition(|(f, _)| extension.as_deref().is_some_and(|x| f.extensions.contains(&x)));

        let mut hits = score_all(&by_ext, &probe, cx).await?;
        if hits.is_empty() {
            hits = score_all(&others, &probe, cx).await?;
        }
        let top = hits.iter().map(|h| h.2.score).max().ok_or_else(|| Error::new(ErrorKind::UnknownFormat))?;
        let mut winners: Vec<_> = hits.into_iter().filter(|h| h.2.score == top).collect();
        if winners.len() > 1 {
            return Err(Error::new(ErrorKind::AmbiguousFormat {
                candidates: winners.iter().map(|(f, _, _)| f.name.to_owned()).collect(),
            }));
        }
        let (_, edge, detected) = winners.remove(0);
        edge.handler.read(source, &detected, cx).await
    }

    /// Identifies `source`, then follows the cheapest path to `to.format`
    /// (`NoPath` if there is none). A source already in the target format is returned as is.
    ///
    /// The path is a read edge followed by at most one capability edge; that is all this
    /// slice needs. Longer paths come with the first slice that has one.
    pub async fn convert(&self, source: Value, to: Target, cx: &Cx) -> Result<Value> {
        let target = to.format.name();
        let mut value = source;
        if value.format().name() == target {
            return Ok(value);
        }
        if !self.candidates(value.format().name()).is_empty() {
            value = self.identify(value, cx).await?;
            if value.format().name() == target {
                return Ok(value);
            }
        }
        let no_path = || Error::new(ErrorKind::NoPath { from: value.format().to_string(), to: target.to_owned() });
        let provides = self
            .registry
            .formats
            .iter()
            .find(|f| f.name == value.format().name())
            .map(|f| f.provides.as_slice())
            .unwrap_or(&[]);
        let edge: &CapEdge = self
            .registry
            .formats
            .iter()
            .find(|f| f.name == target)
            // Cheapest first; ties go to the earlier registration (`min_by_key` keeps the first).
            .and_then(|f| f.cap_edges.iter().filter(|e| provides.contains(&e.cap)).min_by_key(|e| e.cost))
            .ok_or_else(no_path)?;
        edge.handler.convert(value, &to, cx).await
    }

    /// The extension output files of `format` get (its first registered extension).
    pub fn extension_of(&self, format: &str) -> Option<&str> {
        self.registry.formats.iter().find(|f| f.name == format).and_then(|f| f.extensions.first().copied())
    }
}

async fn score_all<'a>(
    candidates: &[Candidate<'a>],
    probe: &Probe<'_>,
    cx: &Cx,
) -> Result<Vec<(&'a FormatDef, &'a ReadEdge, Detected)>> {
    let mut hits = Vec::new();
    for &(f, e) in candidates {
        if let Some(d) = e.handler.score(probe, cx).await? {
            hits.push((f, e, d));
        }
    }
    Ok(hits)
}

/// The shared header prefix and the length, for `file` values (one `stat` plus one ranged read).
async fn probe_data(source: &Value, header_len: usize) -> Result<(Vec<u8>, Option<u64>)> {
    let Some(file) = source.downcast_ref::<FileData>() else {
        return Ok((Vec::new(), None));
    };
    let start = file.range.as_ref().map_or(0, |r| r.start);
    let len = match &file.range {
        Some(r) => r.end - r.start,
        None => file.op.stat(&file.path).await?.content_length(),
    };
    let n = (header_len as u64).min(len);
    let header = if n == 0 { Vec::new() } else { file.op.read_with(&file.path).range(start..start + n).await?.to_vec() };
    Ok((header, Some(len)))
}

fn extension_of_key(key: &ValueKey) -> Option<String> {
    match key {
        ValueKey::File { path, .. } => {
            let name = path.rsplit('/').next()?;
            let (stem, ext) = name.rsplit_once('.')?;
            (!stem.is_empty()).then(|| ext.to_ascii_lowercase())
        }
        _ => None,
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
