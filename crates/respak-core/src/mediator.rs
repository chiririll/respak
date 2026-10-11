//! The mediator: detection, path search and conversion (PROJECT.md §5.5). No middleware yet.

use std::sync::Arc;

use opendal::Operator;

use crate::detect::{Detected, Probe};
use crate::error::{Error, ErrorKind, Result};
use crate::graph::{self, FormatGraph, Path, TargetInfo};
use crate::plugin::{FormatDef, ReadEdge, Registry};
use crate::value::{FileData, FormatId, Value, ValueKey};

/// What to convert to: a format and its options.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Target format.
    pub format: FormatId,
    /// Target options, `Null` if none. Unused so far.
    pub options: serde_json::Value,
    /// Formats the path must pass through, in order. Empty = cheapest path.
    pub via: Vec<String>,
}

/// One format that scored for an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The format, with its detected version.
    pub format: FormatId,
    /// Its score.
    pub score: u32,
}

/// Detects formats and converts values along the cheapest path.
#[derive(Clone)]
pub struct Mediator {
    registry: Arc<Registry>,
}

type Candidate<'a> = (&'a FormatDef, &'a ReadEdge);
type Scored<'a> = (&'a FormatDef, &'a ReadEdge, Detected);

/// The result of scoring: `primary` is what `identify` chooses from (extension candidates if
/// any of them matched, otherwise every read edge); `rest` are the hits of the other phase.
struct Scan<'a> {
    primary: Vec<Scored<'a>>,
    rest: Vec<Scored<'a>>,
}

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

    /// Scores read edges. With `full` false the second phase is skipped when the first
    /// matched (what `identify` needs); with `full` true both phases are scored.
    async fn scan<'a>(&'a self, source: &Value, cx: &Cx, full: bool) -> Result<Scan<'a>> {
        let candidates = self.candidates(source.format().name());
        let header_len = candidates.iter().map(|(_, e)| e.handler.header_len()).max().unwrap_or(0);
        let (header, len) = probe_data(source, header_len).await?;
        let extension = extension_of_key(source.key());
        let probe = Probe { source, extension: extension.as_deref(), len, header: &header };

        let (by_ext, others): (Vec<_>, Vec<_>) = candidates
            .into_iter()
            .partition(|(f, _)| extension.as_deref().is_some_and(|x| f.extensions.contains(&x)));

        let mut first = score_all(&by_ext, &probe, cx).await?;
        if first.is_empty() {
            let primary = score_all(&others, &probe, cx).await?;
            return Ok(Scan { primary: sorted(primary), rest: Vec::new() });
        }
        let rest = if full { score_all(&others, &probe, cx).await? } else { Vec::new() };
        first = sorted(first);
        Ok(Scan { primary: first, rest: sorted(rest) })
    }

    /// Scores read edges from `source`'s format and applies the winner.
    ///
    /// Extension candidates are scored first; if none matches, all read edges are. Highest
    /// score wins; a tie is `AmbiguousFormat`, no match `UnknownFormat`.
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value> {
        let scan = self.scan(&source, cx, false).await?;
        let top = scan.primary.first().map(|h| h.2.score).ok_or_else(|| Error::new(ErrorKind::UnknownFormat))?;
        let mut winners: Vec<_> = scan.primary.into_iter().filter(|h| h.2.score == top).collect();
        if winners.len() > 1 {
            return Err(Error::new(ErrorKind::AmbiguousFormat {
                candidates: winners.iter().map(|(f, _, _)| f.name.to_owned()).collect(),
            }));
        }
        let (_, edge, detected) = winners.remove(0);
        edge.handler.read(source, &detected, cx).await
    }

    /// Scores read edges like [`identify`](Self::identify) but applies none. The winner comes
    /// first, then the other hits of the same phase by score (ties in registration order), then
    /// the hits of the other phase. Empty = unrecognised. Never fails on a tie.
    pub async fn detect(&self, source: &Value, cx: &Cx) -> Result<Vec<Hit>> {
        let scan = self.scan(source, cx, true).await?;
        Ok(scan
            .primary
            .into_iter()
            .chain(scan.rest)
            .map(|(f, _, d)| {
                let mut format = FormatId::new(f.name);
                if let Some(v) = d.version {
                    format = format.with_version(v);
                }
                Hit { format, score: d.score }
            })
            .collect())
    }

    /// Identifies `source`, then follows the cheapest path to `to.format` that visits `to.via`
    /// (`NoPath` if there is none). A source already in the target format is returned as is.
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
        let planned = self.plan_inner(value.format(), target, &to.via)?;
        let last = planned.handlers.len().saturating_sub(1);
        for (i, (step, handler)) in planned.path.steps.iter().zip(&planned.handlers).enumerate() {
            let next = if i == last {
                to.clone()
            } else {
                Target { format: FormatId::new(step.to.clone()), options: serde_json::Value::Null, via: Vec::new() }
            };
            value = handler.convert(value, &next, cx).await?;
        }
        Ok(value)
    }

    fn plan_inner(&self, from: &FormatId, to: &str, via: &[String]) -> Result<graph::Planned> {
        let planned = graph::plan(&self.registry, from.name(), to, via)
            .ok_or_else(|| Error::new(ErrorKind::NoPath { from: from.to_string(), to: to.to_owned() }))?;
        if planned.alternatives > 0 {
            tracing::warn!(
                "path {} -> {to}: chose `{}` over {} other path(s)",
                from.name(),
                planned.path,
                planned.alternatives
            );
        }
        Ok(planned)
    }

    /// The cheapest path from `from` to `to` that visits `via` in order.
    ///
    /// Candidates are ordered by: total cost, then fewer capability steps (direct edges first),
    /// then fewer hops, then registration order. `via` must be a subsequence of the path's
    /// formats (both ends count). When more than one complete path existed, the chosen one is
    /// logged with `tracing::warn!`. `NoPath` if there is none.
    pub fn plan(&self, from: &FormatId, to: &str, via: &[String]) -> Result<Path> {
        Ok(self.plan_inner(from, to, via)?.path)
    }

    /// Formats reachable from `from` (excluding itself), found from declared capabilities
    /// without decoding anything. In registration order.
    pub fn targets_from(&self, from: &str) -> Vec<TargetInfo> {
        graph::targets_from(&self.registry, from)
    }

    /// Whether values of `format` provide the [`Container`](crate::capability::Container) capability.
    pub fn provides_container(&self, format: &str) -> bool {
        let id = crate::plugin::CapId::of::<dyn crate::capability::Container>();
        self.registry.formats.iter().any(|f| f.name == format && f.provides.contains(&id))
    }

    /// The whole graph as data.
    pub fn formats(&self) -> FormatGraph {
        graph::format_graph(&self.registry)
    }

    /// The extension output files of `format` get (its first registered extension).
    pub fn extension_of(&self, format: &str) -> Option<&str> {
        self.registry.formats.iter().find(|f| f.name == format).and_then(|f| f.extensions.first().copied())
    }
}

/// Best score first; equal scores keep registration order (stable sort).
fn sorted(mut hits: Vec<Scored<'_>>) -> Vec<Scored<'_>> {
    hits.sort_by_key(|h| std::cmp::Reverse(h.2.score));
    hits
}

async fn score_all<'a>(candidates: &[Candidate<'a>], probe: &Probe<'_>, cx: &Cx) -> Result<Vec<Scored<'a>>> {
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
    /// Containers mounted so far in this call.
    pub mounts: Arc<crate::resolve::Mounts>,
}

impl Cx {
    /// A context with nothing mounted yet. One context serves one `Engine` call.
    pub fn new(mediator: Mediator, input: Operator, output: Operator) -> Self {
        Self { mediator, input, output, mounts: Arc::default() }
    }
}
