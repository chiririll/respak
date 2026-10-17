//! Request types of [`Engine::convert`](super::Engine::convert).

use serde::{Deserialize, Serialize};

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
    pub(super) fn sources(&self) -> Vec<&SourceSpec> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v.iter().collect(),
        }
    }

    /// The request's own spelling, for the report of a request that expanded to nothing.
    pub(super) fn label(&self) -> String {
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

/// `icon.png` + `dds` -> `icon.dds`; a dot in a directory name is not an extension.
pub(super) fn with_extension(path: &str, ext: &str) -> String {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => format!("{}.{ext}", &path[..name_start + dot]),
        _ => format!("{path}.{ext}"),
    }
}

/// `texture/foo.txtr` -> `texture/foo`.
pub(super) fn without_extension(path: &str) -> &str {
    let name_start = path.rfind('/').map_or(0, |i| i + 1);
    match path[name_start..].rfind('.') {
        Some(dot) if dot > 0 => &path[..name_start + dot],
        _ => path,
    }
}
