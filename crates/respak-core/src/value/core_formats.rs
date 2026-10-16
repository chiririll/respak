//! The formats core itself defines: `file`, `blob`, `bundle`, `compressed`.

use std::ops::Range;
use std::sync::OnceLock;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;

use super::{Data, FormatId, Value, ValueKey};
use crate::capability::Blob;
use crate::error::Result;
use crate::Cx;

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
        Self {
            op,
            path: path.into(),
            range,
            key,
        }
    }
}

impl Data for FileData {
    fn format(&self) -> &FormatId {
        Self::format_id()
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_blob(&self) -> Option<&dyn Blob> {
        Some(self)
    }
}

/// The file's bytes, read by range from its operator (relative to the value's own range).
#[async_trait]
impl Blob for FileData {
    fn len(&self) -> Option<u64> {
        self.range.as_ref().map(|r| r.end - r.start)
    }

    async fn read(&self, range: Range<u64>, _cx: &Cx) -> Result<Bytes> {
        let base = self.range.as_ref().map_or(0, |r| r.start);
        let total = match &self.range {
            Some(r) => r.end - r.start,
            None => self.op.stat(&self.path).await?.content_length(),
        };
        let (start, end) = (
            range.start.min(total),
            range.end.min(total).max(range.start.min(total)),
        );
        if start == end {
            return Ok(Bytes::new());
        }
        Ok(self
            .op
            .read_with(&self.path)
            .range(base + start..base + end)
            .await?
            .to_bytes())
    }
}

/// `blob`: bytes in memory. A converter that encodes a format (e.g. `png`) returns a blob
/// tagged with that format (see [`BlobData::with_format`]), so core can write it out.
#[derive(Debug)]
pub struct BlobData {
    bytes: Bytes,
    key: ValueKey,
    format: FormatId,
}

impl BlobData {
    /// A plain `blob` with the given identity.
    pub fn new(bytes: Bytes, key: ValueKey) -> Self {
        Self {
            bytes,
            key,
            format: FormatId::from_static(BLOB),
        }
    }

    /// Encoded bytes of `format`; still a [`Blob`] capability, so it can be written to the output.
    pub fn with_format(format: FormatId, bytes: Bytes, key: ValueKey) -> Self {
        Self { bytes, key, format }
    }

    /// The bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }
}

#[async_trait]
impl Blob for BlobData {
    fn len(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }

    async fn read(&self, range: Range<u64>, _cx: &Cx) -> Result<Bytes> {
        let len = self.bytes.len() as u64;
        let (start, end) = (
            range.start.min(len),
            range.end.min(len).max(range.start.min(len)),
        );
        Ok(self.bytes.slice(start as usize..end as usize))
    }
}

impl Data for BlobData {
    fn format(&self) -> &FormatId {
        &self.format
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

/// `bundle`: several values that form one output (e.g. a model with its textures, a texture
/// with all its mips). Written as a composite output: a folder with one file per named member.
#[derive(Debug)]
pub struct BundleData {
    /// The members, in order.
    pub items: Vec<Value>,
    /// The file name of each member inside the output folder, parallel to `items`.
    pub names: Vec<String>,
    key: ValueKey,
}

core_format!(BundleData, BUNDLE);

impl BundleData {
    /// A bundle of unnamed members (named `0`, `1`, ... when written).
    pub fn new(items: Vec<Value>, key: ValueKey) -> Self {
        let names = (0..items.len()).map(|i| i.to_string()).collect();
        Self { items, names, key }
    }

    /// A bundle whose members are written under the given file names.
    pub fn named(items: Vec<(String, Value)>, key: ValueKey) -> Self {
        let (names, items) = items.into_iter().unzip();
        Self { items, names, key }
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
        Self {
            codec: codec.into(),
            data,
            decoded_len,
            key,
        }
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
