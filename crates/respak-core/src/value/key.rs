//! Value keys: identity for dedup and cycle detection.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use serde_json::Value as Json;

use super::format::FormatId;

/// The identity of a value.
///
/// - A **file** is identified by its full virtual path (containers included, e.g.
///   `paks/Win64/common.rpak/texture/foo.txtr`) plus an optional byte range.
/// - A **converted** value is identified by its source's key, the target format and the
///   **normalized options**.
///
/// # Normalization
/// Options are normalized before they enter a key, so equal requests give equal keys:
/// 1. defaults are filled in by the caller (the typed target parses options into a struct
///    with `Default` and serializes it back; [`ValueKey::converted`] cannot know the defaults);
/// 2. object fields are sorted by name, recursively; array order is kept;
/// 3. the result is stored as compact JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKey {
    /// A file at a virtual path, optionally a byte range of it.
    File {
        /// Full virtual path.
        path: String,
        /// Byte range, `None` for the whole file.
        range: Option<Range<u64>>,
    },
    /// A value produced by a conversion.
    Converted {
        /// Key of the value that was converted.
        source: Arc<ValueKey>,
        /// The target format.
        target: FormatId,
        /// Normalized options as compact JSON text.
        options: String,
    },
}

impl ValueKey {
    /// Key of a whole file.
    pub fn file(path: impl Into<String>) -> Self {
        Self::File {
            path: path.into(),
            range: None,
        }
    }

    /// Key of a byte range of a file.
    pub fn file_range(path: impl Into<String>, range: Range<u64>) -> Self {
        Self::File {
            path: path.into(),
            range: Some(range),
        }
    }

    /// Key of a converted value. `options` is normalized here (fields sorted); defaults must
    /// already be filled in. See the type docs.
    pub fn converted(source: Arc<ValueKey>, target: FormatId, options: &Json) -> Self {
        Self::Converted {
            source,
            target,
            options: normalize(options).to_string(),
        }
    }
}

fn normalize(v: &Json) -> Json {
    match v {
        Json::Object(m) => {
            let mut entries: Vec<_> = m.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            Json::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k.clone(), normalize(v)))
                    .collect(),
            )
        }
        Json::Array(a) => Json::Array(a.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

impl fmt::Display for ValueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path, range: None } => f.write_str(path),
            Self::File {
                path,
                range: Some(r),
            } => write!(f, "{path}[{}..{}]", r.start, r.end),
            Self::Converted {
                source,
                target,
                options,
            } => write!(f, "{source} => {target} {options}"),
        }
    }
}
