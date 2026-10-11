//! `rpak`: the Respawn pak container, and `rpak.asset`, the typed entry its volume hands out.
//!
//! # The volume
//! ```text
//! common.rpak/
//!   0x8A3F12C4D5E6F701.txtr    the asset header (a name hashed to the same GUID resolves too)
//! ```
//! Each asset is a file named by its GUID and kind. The file's bytes are the asset header as
//! stored in the pak (pointers are not relocated until the slice that reads assets). Names are
//! not known yet, so listing shows GUIDs; a path that gives a *name* (`texture/foo.txtr`) is
//! hashed to a GUID and resolves.
//!
//! # Cost
//! An uncompressed pak is read with ranged reads: the tables, then only the header bytes asked
//! for. An RTech-compressed pak is one stream: the first access reads the whole file and
//! decompresses the whole body into memory (for a big pak that is the decompressed size), and the
//! result is kept for the life of the container. A bounded streaming decoder is on the backlog.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Container, Dependency, Metadata};
use respak_core::value::FileData;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{
    magic, u16le, Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey,
    Version,
};
use tokio::sync::OnceCell;

use crate::guid::string_to_guid;
use crate::pak::{AssetEntry, PakHeader, Tables};
use crate::rtech;

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// The flat file of all decompressed pages (not listed, but it resolves). Pointers in served
/// headers are offsets into it.
pub const MEM: &str = ".mem";

/// Where the body of a pak comes from.
enum Body {
    /// Stored as is: ranged reads of the file.
    Raw { op: Operator, path: String, start: u64 },
    /// RTech-compressed: decompressed once, on first use.
    Rtech { op: Operator, path: String, start: u64, header_len: usize, len: u64, cell: OnceCell<Bytes> },
}

impl Body {
    async fn decompressed(&self) -> Result<&Bytes> {
        let Self::Rtech { op, path, start, header_len, len, cell } = self else {
            unreachable!("only compressed bodies are decompressed")
        };
        cell.get_or_try_init(|| async {
            // The compressed body is a single stream, so the whole file is needed.
            let read = op.read_with(path);
            let file = if *start == 0 { read.await? } else { read.range(*start..).await? };
            let (file, header_len, len) = (file.to_bytes(), *header_len, *len);
            tokio::task::spawn_blocking(move || {
                rtech::decompress(&file, header_len, Some(len)).map(Bytes::from).map_err(|e| Error::custom("rtech", e))
            })
            .await
            .map_err(|e| Error::custom("decompression task", e))?
        })
        .await
    }

    /// `range` is relative to the start of the body (the byte after the header).
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        match self {
            Self::Raw { op, path, start } => {
                Ok(op.read_with(path).range(start + range.start..start + range.end).await?.to_bytes())
            }
            Self::Rtech { .. } => {
                let body = self.decompressed().await?;
                let (s, e) = (usize::try_from(range.start), usize::try_from(range.end));
                match (s, e) {
                    (Ok(s), Ok(e)) if s <= e && e <= body.len() => Ok(body.slice(s..e)),
                    _ => Err(corrupt("read past the end of the decompressed pak")),
                }
            }
        }
    }
}

/// The parts of an open pak that the volume and the entries share.
struct Inner {
    header: PakHeader,
    tables: Tables,
    body: Body,
    /// GUID -> index into `tables.assets`; the first entry wins on a duplicate.
    by_guid: HashMap<u64, usize>,
}

impl Inner {
    /// Finds the asset a volume path names: `0x<GUID>.<kind>`, or `<name>.<kind>` (the name is
    /// hashed). The kind must match.
    fn locate(&self, path: &str) -> Option<usize> {
        let (stem, kind) = path.rsplit_once('.')?;
        let guid = match stem.strip_prefix("0x").or_else(|| stem.strip_prefix("0X")) {
            Some(hex) if !hex.is_empty() && hex.len() <= 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u64::from_str_radix(hex, 16).ok()?
            }
            _ => string_to_guid(stem),
        };
        let index = *self.by_guid.get(&guid)?;
        (self.tables.assets[index].kind_string().eq_ignore_ascii_case(kind)).then_some(index)
    }

    fn file_name(asset: &AssetEntry) -> String {
        format!("0x{:016X}.{}", asset.guid, asset.kind_string())
    }
}

struct RpakSource(Arc<Inner>);

#[async_trait]
impl VolumeSource for RpakSource {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() {
            return Ok(VolumeStat { is_dir: true, len: 0 });
        }
        if path == MEM {
            return Ok(VolumeStat { is_dir: false, len: self.0.tables.mem_len() });
        }
        let index = self.0.locate(path).ok_or_else(|| Error::new(ErrorKind::NotFound { path: path.to_owned() }))?;
        Ok(VolumeStat { is_dir: false, len: u64::from(self.0.tables.assets[index].header_size) })
    }

    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        if path == MEM {
            let base = self.0.tables.page_data_offset;
            return self.0.body.read(base + range.start..base + range.end).await;
        }
        let index = self.0.locate(path).ok_or_else(|| Error::new(ErrorKind::NotFound { path: path.to_owned() }))?;
        let asset = &self.0.tables.assets[index];
        let header = self
            .0
            .tables
            .header_range(asset)
            .ok_or_else(|| corrupt(format!("asset {:#018x}: header does not fit its page", asset.guid)))?;
        let mut raw = self.0.body.read(header).await?.to_vec();
        self.0.tables.relocate_header(asset, &mut raw)?;
        raw.get(range.start as usize..range.end as usize)
            .map(Bytes::copy_from_slice)
            .ok_or_else(|| corrupt("read past the end of an asset header"))
    }

    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        if !dir.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .0
            .tables
            .assets
            .iter()
            .enumerate()
            // A duplicate GUID is listed once.
            .filter(|(i, a)| self.0.by_guid.get(&a.guid) == Some(i))
            .map(|(_, a)| VolumeEntry { name: Inner::file_name(a), is_dir: false, len: u64::from(a.header_size) })
            .collect())
    }
}

/// An opened pak: `Data` + `Container`.
pub struct RpakData {
    format: FormatId,
    key: ValueKey,
    mount: String,
    volume: Operator,
    inner: Arc<Inner>,
}

impl std::fmt::Debug for RpakData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpakData").field("format", &self.format).field("mount", &self.mount).finish_non_exhaustive()
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
        let index = self.inner.locate(path).ok_or_else(|| Error::new(ErrorKind::NotFound { path: path.to_owned() }))?;
        let entry = self.inner.tables.assets[index].clone();
        let format = FormatId::new("rpak.asset").with_version(Version::new(entry.version));
        let kind = entry.kind_string();
        let data_mem_offset = if entry.data.is_null() { None } else { self.inner.tables.mem_offset(entry.data) };
        Ok(Arc::new(RpakAsset {
            format,
            key: ValueKey::file(format!("{}/{path}", self.mount)),
            entry,
            kind,
            volume: self.volume.clone(),
            header_path: Inner::file_name(&self.inner.tables.assets[index]),
            data_mem_offset,
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
    /// Offset in `.mem` of the asset's CPU data, if it has any.
    pub data_mem_offset: Option<u64>,
}

impl RpakAsset {
    /// The asset header with pointers relocated, as served by the volume.
    pub async fn read_header(&self) -> Result<Bytes> {
        Ok(self.volume.read(&self.header_path).await?.to_bytes())
    }

    /// `len` bytes of the flat page memory at `offset`.
    pub async fn read_mem(&self, offset: u64, len: u64) -> Result<Bytes> {
        Ok(self.volume.read_with(MEM).range(offset..offset + len).await?.to_bytes())
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

/// Read edge `file -> rpak`.
pub struct RpakRead;

#[async_trait]
impl Detect for RpakRead {
    /// Magic `RPak` = 10, a supported version (6, 7, 8) = +2, extension `rpak` = +1.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, b"RPak") {
            return Ok(None);
        }
        let version = u16le(input.header, 4);
        let known = version.is_some_and(|v| PakHeader::len_for(v).is_some());
        let score = 10 + 2 * u32::from(known) + u32::from(input.extension == Some("rpak"));
        Ok(Some(Detected { score, version: version.map(|v| Version::new(u32::from(v))) }))
    }

    fn header_len(&self) -> usize {
        8
    }
}

#[async_trait]
impl ReadHandler for RpakRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = source
            .downcast_ref::<FileData>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "rpak must be read from a file".into() }))?;
        let start = file.range.as_ref().map_or(0, |r| r.start);
        let file_len = match &file.range {
            Some(r) => r.end - r.start,
            None => file.op.stat(&file.path).await?.content_length(),
        };
        let head_len = file_len.min(PakHeader::MAX_LEN as u64);
        let head = file.op.read_with(&file.path).range(start..start + head_len).await?.to_vec();
        let header = PakHeader::parse(&head)?;

        let compressed = header.is_rtech();
        let needed = if compressed { header.compressed_size } else { header.decompressed_size };
        if file_len < needed {
            return Err(corrupt(format!("rpak: file has {file_len} bytes, its header says {needed}")));
        }
        let header_len = header.header_len;
        let body = if compressed {
            Body::Rtech {
                op: file.op.clone(),
                path: file.path.clone(),
                start,
                header_len,
                len: header.body_len(),
                cell: OnceCell::new(),
            }
        } else {
            Body::Raw { op: file.op.clone(), path: file.path.clone(), start: start + header_len as u64 }
        };
        let table_bytes = if compressed {
            body.decompressed().await?.clone()
        } else {
            body.read(0..header.tables_len()).await?
        };
        let tables = Tables::parse(&header, &table_bytes)?;

        let mut by_guid = HashMap::new();
        for (i, a) in tables.assets.iter().enumerate() {
            by_guid.entry(a.guid).or_insert(i);
        }
        let inner = Arc::new(Inner { header, tables, body, by_guid });
        let mount = source.key().to_string();
        let volume = volume_operator(RpakSource(inner.clone()), &mount);
        let format = FormatId::new("rpak").with_version(Version::new(u32::from(inner.header.version)));
        Ok(Arc::new(RpakData { format, key: source.key().clone(), mount, volume, inner }))
    }
}
