//! The format graph as data (`respak formats`) and path planning (PROJECT.md §5.2).

use std::fmt;
use std::sync::Arc;

use serde::Serialize;

use crate::detect::ConvertHandler;
use crate::plugin::Registry;

/// Longest path (in formats) the planner will consider.
const MAX_PATH_FORMATS: usize = 8;

/// Every registered format and its edges, for `respak formats`. Serializes as JSON.
#[derive(Debug, Clone, Serialize)]
pub struct FormatGraph {
    /// In registration order.
    pub formats: Vec<FormatInfo>,
}

/// One format and what connects to it.
#[derive(Debug, Clone, Serialize)]
pub struct FormatInfo {
    /// Format name, e.g. `png`.
    pub name: String,
    /// Human-readable label (defaults to the name).
    pub label: String,
    /// The plugin that defines it.
    pub plugin: String,
    /// Extensions that make it a detection candidate first.
    pub extensions: Vec<String>,
    /// Capabilities its values implement.
    pub provides: Vec<String>,
    /// Formats it can be read from (read edges), e.g. `file`.
    pub read_from: Vec<String>,
    /// Edges that produce it.
    pub produced_by: Vec<EdgeInfo>,
}

/// An edge that produces a format.
#[derive(Debug, Clone, Serialize)]
pub struct EdgeInfo {
    /// `"format"` for a direct edge, `"capability"` for a capability edge.
    pub kind: &'static str,
    /// The source format (direct) or capability name (capability).
    pub from: String,
    /// Edge cost (1 = lossless, higher = lossy).
    pub cost: u32,
}

/// A format a value can be converted to, as listed by discovery. `type` + `defaults` go
/// straight into a request's `to`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetInfo {
    /// Target format name.
    #[serde(rename = "type")]
    pub format: String,
    /// Human-readable label.
    pub label: String,
    /// Default options. `{}` until a target has options.
    pub defaults: serde_json::Value,
}

/// One step of a conversion path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Format the step starts from.
    pub from: String,
    /// Format it produces.
    pub to: String,
    /// The capability a capability edge starts from, `None` for a direct edge.
    pub capability: Option<String>,
    /// Edge cost.
    pub cost: u32,
}

/// A planned conversion path, from a detected format to the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    /// The steps in order; empty if the value is already in the target format.
    pub steps: Vec<Step>,
}

impl Path {
    /// Sum of the step costs.
    pub fn cost(&self) -> u32 {
        self.steps.iter().map(|s| s.cost).sum()
    }
}

impl fmt::Display for Path {
    /// `png =(Bitmap)=> dds => cast`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(first) = self.steps.first() else {
            return f.write_str("(already there)");
        };
        f.write_str(&first.from)?;
        for s in &self.steps {
            match &s.capability {
                Some(c) => write!(f, " =({c})=> {}", s.to)?,
                None => write!(f, " => {}", s.to)?,
            }
        }
        Ok(())
    }
}

/// A chosen path with the handlers to run, and how many other complete paths existed.
pub(crate) struct Planned {
    pub path: Path,
    pub handlers: Vec<Arc<dyn ConvertHandler>>,
    pub alternatives: usize,
}

#[derive(Clone)]
struct OutEdge {
    to: &'static str,
    capability: Option<&'static str>,
    cost: u32,
    order: usize,
    handler: Arc<dyn ConvertHandler>,
}

/// Edges leaving a value of format `from`: direct edges, and capability edges of any other
/// format whose capability `from` provides.
fn edges_from(reg: &Registry, from: &str) -> Vec<OutEdge> {
    let provides = reg
        .formats
        .iter()
        .find(|f| f.name == from)
        .map(|f| f.provides.as_slice())
        .unwrap_or(&[]);
    let mut out: Vec<OutEdge> = reg
        .direct_edges
        .iter()
        .filter(|d| d.from == from && d.to != from)
        .map(|d| OutEdge {
            to: d.to,
            capability: None,
            cost: d.cost,
            order: d.order,
            handler: d.handler.clone(),
        })
        .collect();
    for f in reg.formats.iter().filter(|f| f.name != from) {
        for e in f.cap_edges.iter().filter(|e| provides.contains(&e.cap)) {
            out.push(OutEdge {
                to: f.name,
                capability: Some(e.cap.name),
                cost: e.cost,
                order: e.order,
                handler: e.handler.clone(),
            });
        }
    }
    out
}

fn walk(
    reg: &Registry,
    at: &str,
    to: &str,
    visited: &mut Vec<String>,
    stack: &mut Vec<(String, OutEdge)>,
    found: &mut Vec<Vec<(String, OutEdge)>>,
) {
    if at == to {
        found.push(stack.clone());
        return;
    }
    if visited.len() >= MAX_PATH_FORMATS {
        return;
    }
    for e in edges_from(reg, at) {
        if visited.iter().any(|v| v == e.to) {
            continue;
        }
        visited.push(e.to.to_owned());
        stack.push((at.to_owned(), e.clone()));
        walk(reg, e.to, to, visited, stack, found);
        stack.pop();
        visited.pop();
    }
}

/// Plans the cheapest path from `from` to `to` visiting `via` in order (a subsequence of the
/// path's formats, ends included). See [`crate::Mediator::plan`] for the ordering rules.
pub(crate) fn plan(reg: &Registry, from: &str, to: &str, via: &[String]) -> Option<Planned> {
    if from == to {
        return via.is_empty().then(|| Planned {
            path: Path { steps: Vec::new() },
            handlers: Vec::new(),
            alternatives: 0,
        });
    }
    let mut found = Vec::new();
    walk(
        reg,
        from,
        to,
        &mut vec![from.to_owned()],
        &mut Vec::new(),
        &mut found,
    );

    let satisfies_via = |p: &Vec<(String, OutEdge)>| {
        let mut nodes = std::iter::once(from).chain(p.iter().map(|(_, e)| e.to));
        via.iter().all(|v| nodes.any(|n| n == v))
    };
    found.retain(satisfies_via);
    let key = |p: &Vec<(String, OutEdge)>| {
        let cost: u32 = p.iter().map(|(_, e)| e.cost).sum();
        let caps = p.iter().filter(|(_, e)| e.capability.is_some()).count();
        let orders: Vec<usize> = p.iter().map(|(_, e)| e.order).collect();
        (cost, caps, p.len(), orders)
    };
    let alternatives = found.len().saturating_sub(1);
    let best = found.into_iter().min_by_key(|p| key(p))?;
    let path = Path {
        steps: best
            .iter()
            .map(|(f, e)| Step {
                from: f.clone(),
                to: e.to.to_owned(),
                capability: e.capability.map(str::to_owned),
                cost: e.cost,
            })
            .collect(),
    };
    Some(Planned {
        path,
        handlers: best.into_iter().map(|(_, e)| e.handler).collect(),
        alternatives,
    })
}

/// Formats reachable from `from` (not `from` itself), in registration order.
pub(crate) fn targets_from(reg: &Registry, from: &str) -> Vec<TargetInfo> {
    let mut seen = vec![from.to_owned()];
    let mut queue = vec![from.to_owned()];
    while let Some(at) = queue.pop() {
        for e in edges_from(reg, &at) {
            if !seen.iter().any(|s| s == e.to) {
                seen.push(e.to.to_owned());
                queue.push(e.to.to_owned());
            }
        }
    }
    reg.formats
        .iter()
        .filter(|f| f.name != from && seen.iter().any(|s| s == f.name))
        .map(|f| TargetInfo {
            format: f.name.to_owned(),
            label: f.label().to_owned(),
            defaults: serde_json::json!({}),
        })
        .collect()
}

/// The whole registry as data.
pub(crate) fn format_graph(reg: &Registry) -> FormatGraph {
    let formats = reg
        .formats
        .iter()
        .map(|f| {
            let mut produced_by: Vec<EdgeInfo> = reg
                .direct_edges
                .iter()
                .filter(|d| d.to == f.name)
                .map(|d| EdgeInfo {
                    kind: "format",
                    from: d.from.to_owned(),
                    cost: d.cost,
                })
                .collect();
            produced_by.extend(f.cap_edges.iter().map(|e| EdgeInfo {
                kind: "capability",
                from: e.cap.name.to_owned(),
                cost: e.cost,
            }));
            FormatInfo {
                name: f.name.to_owned(),
                label: f.label().to_owned(),
                plugin: f.plugin.clone(),
                extensions: f.extensions.iter().map(|e| (*e).to_owned()).collect(),
                provides: f.provides.iter().map(|c| c.name.to_owned()).collect(),
                read_from: f.read_edges.iter().map(|e| e.from.to_owned()).collect(),
                produced_by,
            }
        })
        .collect();
    FormatGraph { formats }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::error::Result;
    use crate::mediator::{Cx, Target};
    use crate::value::Value;

    struct Noop;

    #[async_trait]
    impl ConvertHandler for Noop {
        async fn convert(&self, source: Value, _: &Target, _: &Cx) -> Result<Value> {
            Ok(source)
        }
    }

    #[test]
    fn alternatives_are_counted_only_when_there_was_a_choice() {
        let mut r = Registry::new();
        r.format("a").to("b", Noop).to("c", Noop);
        r.format("b").to("d", Noop);
        r.format("c");
        r.format("d");
        r.check().unwrap();

        let single = plan(&r, "a", "c", &[]).unwrap();
        assert_eq!(single.alternatives, 0);

        let mut r2 = Registry::new();
        r2.format("a").to("b", Noop).to("d", Noop);
        r2.format("b").to("d", Noop);
        r2.format("d");
        let choice = plan(&r2, "a", "d", &[]).unwrap();
        assert_eq!((choice.alternatives, choice.path.steps.len()), (1, 1));

        let forced = plan(&r2, "a", "d", &["b".to_owned()]).unwrap();
        assert_eq!((forced.alternatives, forced.path.steps.len()), (0, 2));
    }
}
