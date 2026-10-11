//! The formats core itself defines: `file`, `blob`, `bundle`, `compressed`.

use std::ops::Range;
use std::sync::OnceLock;

use bytes::Bytes;
use opendal::Operator;

use super::{Data, FormatId, Value, ValueKey};
use crate::capability::Blob;

/// Format name of [`FileData`].
pub const FILE: &str = "file";
/// Format name of [`BlobData`].
pub const BLOB: &str = "blob";
/// Format name of [`BundleData`].
pub const BUNDLE: &str = "bundle";
/// Format name of [`CompressedData`].
pub const COMPRESSED: &str = "compressed";

macro_rules! core_format {
    ($ty:ident, $name:expr) => {
        impl $ty {
            fn format_id() -> &'static FormatId {
                static ID: OnceLock<FormatId> = OnceLock::new();
                ID.get_or_init(|| FormatId::from_static($name))
            }
        }
    };
}

/// `file`: a path on an operator, on the host's operator or inside a container's volume,
/// optionally restricted to a byte range. Nothing is read until a handler asks.
#[derive(Debug)]
pub struct FileData {
    /// The operator the path lives on.
    pub op: Operator,
    /// Path on `op`.
    pub path: String,
    /// Byte range, `None` for the whole file.
    pub range: Option<Range<u64>>,
    key: ValueKey,
}

core_format!(FileData, FILE);

impl FileData {
    /// A file value. `virtual_path` is the full virtual path used for its key (it differs from
    /// `path` when `op` is a container's volume).
    pub fn new(
        op: Operator,
        path: impl Into<String>,
        virtual_path: impl Into<String>,
        range: Option<Range<u64>>,
    ) -> Self {
        let key = match range.clone() {
            Some(r) => ValueKey::file_range(virtual_path, r),
            None => ValueKey::file(virtual_path),
        };
        Self { op, path: path.into(), range, key }
    }
}

impl Data for FileData {
    fn format(&self) -> &FormatId {
        Self::format_id()
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
}

/// `blob`: bytes in memory.
#[derive(Debug)]
pub struct BlobData {
    bytes: Bytes,
    key: ValueKey,
}

core_format!(BlobData, BLOB);

impl BlobData {
    /// A blob with the given identity.
    pub fn new(bytes: Bytes, key: ValueKey) -> Self {
        Self { bytes, key }
    }

    /// The bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

impl Blob for BlobData {
    fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

impl Data for BlobData {
    fn format(&self) -> &FormatId {
        Self::format_id()
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn approx_size(&self) -> usize {
        self.bytes.len()
    }
    fn as_blob(&self) -> Option<&dyn Blob> {
        Some(self)
    }
}

/// `bundle`: several values that form one output (e.g. a model with its textures).
#[derive(Debug)]
pub struct BundleData {
    /// The members, in order.
    pub items: Vec<Value>,
    key: ValueKey,
}

core_format!(BundleData, BUNDLE);

impl BundleData {
    /// A bundle with the given identity.
    pub fn new(items: Vec<Value>, key: ValueKey) -> Self {
        Self { items, key }
    }
}

impl Data for BundleData {
    fn format(&self) -> &FormatId {
        Self::format_id()
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
}

/// `compressed`: encoded bytes that a plugin's codec handler turns into a `blob`.
#[derive(Debug)]
pub struct CompressedData {
    /// Codec name, e.g. `oodle`, `rtech`, `zstd`. Codecs are shared mediator edges.
    pub codec: String,
    /// The encoded bytes.
    pub data: Bytes,
    /// Size after decompression.
    pub decoded_len: u64,
    key: ValueKey,
}

core_format!(CompressedData, COMPRESSED);

impl CompressedData {
    /// A compressed value with the given identity.
    pub fn new(codec: impl Into<String>, data: Bytes, decoded_len: u64, key: ValueKey) -> Self {
        Self { codec: codec.into(), data, decoded_len, key }
    }
}

impl Data for CompressedData {
    fn format(&self) -> &FormatId {
        Self::format_id()
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn approx_size(&self) -> usize {
        self.data.len()
    }
}
