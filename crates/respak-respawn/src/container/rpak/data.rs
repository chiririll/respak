//! `rpak` (a container) and `rpak.asset` (the typed entry its volume hands out).

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Container, Dependency, Metadata};
use respak_core::{Data, Error, ErrorKind, FormatId, Result, Value, ValueKey, Version};

use super::volume::Inner;
use super::MEM;
use crate::pak::{AssetEntry, PagePtr, PakHeader, Tables};
use crate::starpak;

/// An opened pak: `Data` + `Container`.
pub struct RpakData {
    pub(super) format: FormatId,
    pub(super) key: ValueKey,
    pub(super) mount: String,
    pub(super) volume: Operator,
    pub(super) inner: Arc<Inner>,
}

impl std::fmt::Debug for RpakData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpakData")
            .field("format", &self.format)
            .field("mount", &self.mount)
            .finish_non_exhaustive()
    }
}

impl RpakData {
    /// The parsed header.
    pub fn header(&self) -> &PakHeader {
        &self.inner.header
    }

    /// The parsed tables.
    pub fn tables(&self) -> &Tables {
        &self.inner.tables
    }
}

#[async_trait]
impl Container for RpakData {
    fn volume(&self) -> Operator {
        self.volume.clone()
    }

    fn mount_path(&self) -> &str {
        &self.mount
    }

    async fn entry(&self, path: &str) -> Result<Value> {
        let index = self.inner.locate(path).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })
        })?;
        let entry = self.inner.tables.assets[index].clone();
        let format = FormatId::new("rpak.asset").with_version(Version::new(entry.version));
        let kind = entry.kind_string();
        let has_stream = starpak::split_reference(entry.starpak).is_some();
        let has_opt = starpak::split_reference(entry.opt_starpak).is_some();
        let data_mem_offset = if entry.data.is_null() {
            None
        } else {
            self.inner.tables.mem_offset(entry.data)
        };
        Ok(Arc::new(RpakAsset {
            format,
            key: ValueKey::file(format!("{}/{path}", self.mount)),
            entry,
            kind,
            volume: self.volume.clone(),
            header_path: Inner::file_name(&self.inner.tables.assets[index]),
            stream: has_stream.then(|| {
                format!(
                    "{}.stream",
                    Inner::file_name(&self.inner.tables.assets[index])
                )
            }),
            opt: has_opt
                .then(|| format!("{}.opt", Inner::file_name(&self.inner.tables.assets[index]))),
            data_mem_offset,
            pak_created_time: self.inner.header.created_time,
            inner: Pages(self.inner.clone()),
        }))
    }
}

impl Data for RpakData {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

/// `rpak.asset`: what the pak's asset table says about one asset. The asset's own version and
/// header size live here, not in the asset's bytes.
#[derive(Debug)]
pub struct RpakAsset {
    format: FormatId,
    key: ValueKey,
    /// The asset table entry.
    pub entry: AssetEntry,
    kind: String,
    /// The pak's volume.
    pub volume: Operator,
    /// Path of the asset's header file in `volume` (pointers relocated to `.mem` offsets).
    pub header_path: String,
    /// Path in `volume` of the asset's streamed data (mandatory starpak), if it has any.
    pub stream: Option<String>,
    /// Path in `volume` of the asset's optional streamed data, if it has any.
    pub opt: Option<String>,
    /// Offset in `.mem` of the asset's CPU data, if it has any.
    pub data_mem_offset: Option<u64>,
    /// The pak header's `created_time` (see [`PakHeader`]).
    pub pak_created_time: u64,
    inner: Pages,
}

/// The pak a [`RpakAsset`] came from, for resolving pointers.
struct Pages(Arc<Inner>);

impl std::fmt::Debug for Pages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pages")
    }
}

impl RpakAsset {
    /// The asset header with pointers relocated, as served by the volume.
    pub async fn read_header(&self) -> Result<Bytes> {
        Ok(self.volume.read(&self.header_path).await?.to_bytes())
    }

    /// The `.mem` offset a pointer stored inside page data points to. Only the asset header is
    /// relocated by the volume; a pointer in `.mem` is still the stored `{i32 page, i32 offset}`.
    /// `None` for a null pointer or one that leaves the pages.
    pub fn mem_offset_of(&self, slot: [u8; 8]) -> Option<u64> {
        let page = u32::from_le_bytes(slot[..4].try_into().expect("4 bytes"));
        let offset = u32::from_le_bytes(slot[4..].try_into().expect("4 bytes"));
        let ptr = PagePtr { page, offset };
        if ptr.is_null() {
            return None;
        }
        self.inner.0.tables.mem_offset(ptr)
    }

    /// `len` bytes of the flat page memory at `offset`.
    pub async fn read_mem(&self, offset: u64, len: u64) -> Result<Bytes> {
        Ok(self
            .volume
            .read_with(MEM)
            .range(offset..offset + len)
            .await?
            .to_bytes())
    }
}

impl Metadata for RpakAsset {
    fn name(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> Option<&str> {
        Some(&self.kind)
    }
    fn guid(&self) -> Option<u64> {
        Some(self.entry.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for RpakAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}
