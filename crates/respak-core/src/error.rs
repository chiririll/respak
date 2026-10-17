//! The shared error type of the whole library.

use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use serde::ser::{Serialize, SerializeMap, Serializer};

/// Result alias used across respak.
pub type Result<T> = std::result::Result<T, Error>;

/// A cheaply cloneable error. Inspect it with [`Error::kind`].
#[derive(Debug, Clone)]
pub struct Error(Arc<ErrorKind>);

impl Error {
    /// Wraps a kind into an error.
    pub fn new(kind: ErrorKind) -> Self {
        Self(Arc::new(kind))
    }

    /// The error's kind.
    pub fn kind(&self) -> &ErrorKind {
        &self.0
    }

    /// Malformed or inconsistent data ([`ErrorKind::Corrupt`]).
    pub fn corrupt(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Corrupt {
            context: context.into(),
        })
    }

    /// A known format whose `version` (or variant) is not supported
    /// ([`ErrorKind::UnsupportedVersion`]).
    pub fn unsupported(format: impl Into<String>, version: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnsupportedVersion {
            format: format.into(),
            version: version.into(),
        })
    }

    /// Whether this is [`ErrorKind::Cancelled`].
    pub fn is_cancelled(&self) -> bool {
        matches!(self.kind(), ErrorKind::Cancelled)
    }

    /// A message-only [`ErrorKind::Custom`], with no underlying error.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Custom {
            source: None,
            message: message.into(),
        })
    }

    /// Wraps any error as [`ErrorKind::Custom`].
    pub fn custom(
        message: impl Into<String>,
        source: impl Into<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        Self::new(ErrorKind::Custom {
            source: Some(source.into()),
            message: message.into(),
        })
    }
}

/// What went wrong. See docs/design/project.md §9.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Nothing scored the input: no registered format recognises it.
    #[error("unknown format")]
    UnknownFormat,
    /// Several formats share the top score and the choice is not clear.
    #[error("ambiguous format, candidates: {}", .candidates.join(", "))]
    AmbiguousFormat {
        /// Format ids with equal top scores.
        candidates: Vec<String>,
    },
    /// The format is known, but this version or variant of it is not.
    #[error("unsupported version {version} of format {format}")]
    UnsupportedVersion {
        /// Format id.
        format: String,
        /// The version (or variant) that was found.
        version: String,
    },
    /// The conversion graph has no route between two formats.
    #[error("no conversion path from {from} to {to}")]
    NoPath {
        /// Source format id.
        from: String,
        /// Target format id.
        to: String,
    },
    /// A glob pattern matched nothing.
    #[error("no match for {pattern}")]
    NoMatch {
        /// The glob that matched nothing.
        pattern: String,
    },
    /// A concrete path does not exist.
    #[error("not found: {path}")]
    NotFound {
        /// The missing path.
        path: String,
    },
    /// The request JSON is malformed, or its `version` is unknown.
    #[error("invalid request: {message}")]
    InvalidRequest {
        /// What is wrong with the request.
        message: String,
    },
    /// Target options do not parse.
    #[error("invalid options: {message}")]
    InvalidOptions {
        /// What is wrong with the options.
        message: String,
    },
    /// Two different values were written to the same output path.
    #[error("output conflict at {path}")]
    OutputConflict {
        /// The contested output path.
        path: String,
    },
    /// An OpenDAL storage operation failed.
    #[error("storage error: {0}")]
    Storage(#[source] opendal::Error),
    /// A needed companion file or item (starpak, referenced asset) is missing.
    #[error("missing dependency {item} (file: {file})")]
    MissingDependency {
        /// The item that is needed.
        item: String,
        /// The file that should contain it.
        file: String,
    },
    /// Items depend on each other in a loop.
    #[error("dependency cycle: {}", .chain.join(" -> "))]
    DependencyCycle {
        /// The items in the cycle, in order.
        chain: Vec<String>,
    },
    /// The data is malformed or inconsistent.
    #[error("corrupt data: {context}")]
    Corrupt {
        /// Where and how the data is broken.
        context: String,
    },
    /// The operation was cancelled through its cancellation token.
    #[error("cancelled")]
    Cancelled,
    /// Any other error, from a plugin or the host.
    #[error("{message}")]
    Custom {
        /// The underlying error, if any.
        #[source]
        source: Option<Box<dyn StdError + Send + Sync>>,
        /// Human-readable description.
        message: String,
    },
}

impl ErrorKind {
    /// Stable machine-readable name of the variant, used in reports.
    pub fn name(&self) -> &'static str {
        match self {
            Self::UnknownFormat => "UnknownFormat",
            Self::AmbiguousFormat { .. } => "AmbiguousFormat",
            Self::UnsupportedVersion { .. } => "UnsupportedVersion",
            Self::NoPath { .. } => "NoPath",
            Self::NoMatch { .. } => "NoMatch",
            Self::NotFound { .. } => "NotFound",
            Self::InvalidRequest { .. } => "InvalidRequest",
            Self::InvalidOptions { .. } => "InvalidOptions",
            Self::OutputConflict { .. } => "OutputConflict",
            Self::Storage(_) => "Storage",
            Self::MissingDependency { .. } => "MissingDependency",
            Self::DependencyCycle { .. } => "DependencyCycle",
            Self::Corrupt { .. } => "Corrupt",
            Self::Cancelled => "Cancelled",
            Self::Custom { .. } => "Custom",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.0.source()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self::new(kind)
    }
}

impl From<opendal::Error> for Error {
    fn from(e: opendal::Error) -> Self {
        // A volume reports its own errors through OpenDAL; hand those back as they were.
        if let Some(inner) = std::error::Error::source(&e).and_then(|s| s.downcast_ref::<Error>()) {
            return inner.clone();
        }
        // A missing path is `NotFound`, not a generic storage failure.
        if e.kind() == opendal::ErrorKind::NotFound {
            return Self::new(ErrorKind::NotFound {
                path: e.to_string(),
            });
        }
        Self::new(ErrorKind::Storage(e))
    }
}

/// Serializes as `{ "kind": "...", "message": "...", ...fields }`.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let kind = self.kind();
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("kind", kind.name())?;
        m.serialize_entry("message", &kind.to_string())?;
        match kind {
            ErrorKind::AmbiguousFormat { candidates } => {
                m.serialize_entry("candidates", candidates)?
            }
            ErrorKind::UnsupportedVersion { format, version } => {
                m.serialize_entry("format", format)?;
                m.serialize_entry("version", version)?;
            }
            ErrorKind::NoPath { from, to } => {
                m.serialize_entry("from", from)?;
                m.serialize_entry("to", to)?;
            }
            ErrorKind::NoMatch { pattern } => m.serialize_entry("pattern", pattern)?,
            ErrorKind::NotFound { path } | ErrorKind::OutputConflict { path } => {
                m.serialize_entry("path", path)?
            }
            ErrorKind::MissingDependency { item, file } => {
                m.serialize_entry("item", item)?;
                m.serialize_entry("file", file)?;
            }
            ErrorKind::DependencyCycle { chain } => m.serialize_entry("chain", chain)?,
            ErrorKind::Corrupt { context } => m.serialize_entry("context", context)?,
            _ => {}
        }
        m.end()
    }
}

#[allow(dead_code)]
fn _assert_bounds() {
    fn is<T: Clone + Send + Sync + 'static>() {}
    is::<Error>();
}
