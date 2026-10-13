//! `rpak`: the Respawn pak container, and `rpak.asset`, the typed entry its volume hands out.
//!
//! # The volume
//! ```text
//! common.rpak/
//!   0x8A3F12C4D5E6F701.txtr          the asset header (a name hashed to the same GUID resolves too)
//!   0x8A3F12C4D5E6F701.txtr.stream   its entry in a mandatory starpak (resolves, not listed)
//!   0x8A3F12C4D5E6F701.txtr.opt      its entry in an optional starpak (resolves, not listed)
//! ```
//! The starpaks are looked up next to the pak by file name and opened on first use (their table
//! of contents only). A missing one is `MissingDependency`.
//! Each asset is a file named by its GUID and kind. The file's bytes are the asset header as
//! stored in the pak (pointers are not relocated until the slice that reads assets). Names are
//! not known yet, so listing shows GUIDs; a path that gives a *name* (`texture/foo.txtr`) is
//! hashed to a GUID and resolves.
//!
//! # Cost
//! An uncompressed pak is read with ranged reads: the tables, then only the header bytes asked
//! for. An RTech-compressed pak is a stream of output chunks decoded front to back: a read at
//! offset `X` reads and decodes the stream up to the chunk containing `X` (never past it), and the
//! decoded prefix is kept for the life of the container. Listing a pak decodes only its tables, so
//! it costs the start of the file, not all of it; reading a page near the end costs the file.

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
use tokio::sync::{Mutex, OnceCell};

use crate::guid::string_to_guid;
use crate::pak::{AssetEntry, PakHeader, Tables};
use crate::rtech;
use crate::starpak::{self, Starpak, StarpakEntry};

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
    /// RTech-compressed: the body is decoded front to back, only as far as reads need.
    Rtech { op: Operator, path: String, start: u64, header_len: usize, len: u64, file_len: u64, state: Mutex<Progress> },
}

/// How far an RTech body has been read and decoded.
#[derive(Default)]
struct Progress {
    /// The start of the file (header included) read so far.
    input: Vec<u8>,
    /// The decoded start of the body.
    body: Bytes,
    /// `body` is the whole body.
    done: bool,
}

/// First read of a compressed pak, and the least growth of the input between decoding attempts.
const MIN_INPUT: u64 = 1 << 20;

impl Body {
    /// The decoded body, at least `upto` bytes of it (fewer only if the body is shorter).
    async fn decoded(&self, upto: u64) -> Result<Bytes> {
        let Self::Rtech { op, path, start, header_len, len, file_len, state } = self else {
            unreachable!("only compressed bodies are decoded")
        };
        let mut progress = state.lock().await;
        while (progress.body.len() as u64) < upto && !progress.done {
            // Decode a bit ahead, so a run of small reads does not decode again each time.
            let want = upto.max(2 * progress.body.len() as u64).max(MIN_INPUT).min(*len);
            let complete = progress.input.len() as u64 >= *file_len;
            let (input, header_len) = (std::mem::take(&mut progress.input), *header_len);
            let expected = *len;
            let (input, result) = tokio::task::spawn_blocking(move || {
                let result = rtech::decompress_prefix(&input, complete, header_len, Some(expected), want);
                (input, result)
            })
            .await
            .map_err(|e| Error::custom("decompression task", e))?;
            progress.input = input;
            match result {
                Ok(prefix) => {
                    progress.done = prefix.complete;
                    progress.body = Bytes::from(prefix.body);
                }
                Err(rtech::RtechError::NeedInput { at_least }) => {
                    let have = progress.input.len() as u64;
                    let target = at_least.max(2 * have).max(MIN_INPUT).min(*file_len);
                    if target <= have {
                        return Err(corrupt("rpak: the compressed stream runs past the end of the file"));
                    }
                    let more = op.read_with(path).range(start + have..start + target).await?;
                    progress.input.extend_from_slice(&more.to_bytes());
                }
                Err(e) => return Err(Error::custom("rtech", e)),
            }
        }
        Ok(progress.body.clone())
    }

    /// `range` is relative to the start of the body (the byte after the header).
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        match self {
            Self::Raw { op, path, start } => {
                Ok(op.read_with(path).range(start + range.start..start + range.end).await?.to_bytes())
            }
            Self::Rtech { .. } => {
                let body = self.decoded(range.end).await?;
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
    /// Where the pak lives, to find the starpaks next to it.
    host: Operator,
    /// The pak's directory on `host`: empty, or ending in `/`.
    dir: String,
    /// The starpaks, opened on first use (`starpaks` first, then `opt_starpaks`).
    opened: Vec<OnceCell<Arc<Starpak>>>,
}

/// Which file of an asset a volume path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The relocated asset header.
    Header,
    /// `<asset>.stream`: its entry in a mandatory starpak.
    Stream,
    /// `<asset>.opt`: its entry in an optional starpak.
    Opt,
}

impl Inner {
    /// Finds the asset a volume path names: `0x<GUID>.<kind>`, or `<name>.<kind>` (the name is
    /// hashed). The kind must match.
    fn locate(&self, path: &str) -> Option<usize> {
        self.locate_part(path).map(|(i, _)| i)
    }

    /// Like [`locate`](Self::locate), also telling `foo.txtr`, `foo.txtr.stream` and
    /// `foo.txtr.opt` apart.
    fn locate_part(&self, path: &str) -> Option<(usize, Part)> {
        let (path, part) = if let Some(p) = path.strip_suffix(".stream") {
            (p, Part::Stream)
        } else if let Some(p) = path.strip_suffix(".opt") {
            (p, Part::Opt)
        } else {
            (path, Part::Header)
        };
        let (stem, kind) = path.rsplit_once('.')?;
        let guid = match stem.strip_prefix("0x").or_else(|| stem.strip_prefix("0X")) {
            Some(hex) if !hex.is_empty() && hex.len() <= 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u64::from_str_radix(hex, 16).ok()?
            }
            _ => string_to_guid(stem),
        };
        let index = *self.by_guid.get(&guid)?;
        (self.tables.assets[index].kind_string().eq_ignore_ascii_case(kind)).then_some((index, part))
    }

    /// Opens starpak number `index` of the mandatory (or optional) list. A starpak file that is
    /// not next to the pak is `MissingDependency`; an index past the list is `Corrupt`.
    async fn starpak(&self, asset: &AssetEntry, optional: bool, index: usize) -> Result<&Arc<Starpak>> {
        let list = if optional { &self.tables.opt_starpaks } else { &self.tables.starpaks };
        let path = list.get(index).ok_or_else(|| {
            corrupt(format!("asset {:#018x}: starpak index {index} is past the pak's {} starpak(s)", asset.guid, list.len()))
        })?;
        let slot = &self.opened[if optional { self.tables.starpaks.len() + index } else { index }];
        slot.get_or_try_init(|| async {
            let file = format!("{}{}", self.dir, starpak::file_name(path));
            match Starpak::open(self.host.clone(), file.clone(), None).await {
                Ok(s) => Ok(Arc::new(s)),
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    Err(Error::new(ErrorKind::MissingDependency { item: Self::file_name(asset), file }))
                }
                Err(e) => Err(e),
            }
        })
        .await
    }

    /// The starpak and the entry an asset's `.stream` / `.opt` file maps onto. `NotFound` if the
    /// asset has no such reference, or the starpak's table of contents has no entry at its offset.
    async fn stream_entry(&self, asset: &AssetEntry, optional: bool) -> Result<(&Arc<Starpak>, StarpakEntry)> {
        let reference = if optional { asset.opt_starpak } else { asset.starpak };
        let none = || Error::new(ErrorKind::NotFound { path: format!("{} streamed data", Self::file_name(asset)) });
        let (index, offset) = starpak::split_reference(reference).ok_or_else(none)?;
        let sp = self.starpak(asset, optional, index).await?;
        let entry = sp.entry(offset).ok_or_else(none)?;
        Ok((sp, entry))
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
        let (index, part) =
            self.0.locate_part(path).ok_or_else(|| Error::new(ErrorKind::NotFound { path: path.to_owned() }))?;
        let asset = &self.0.tables.assets[index];
        if part != Part::Header {
            let (_, entry) = self.0.stream_entry(asset, part == Part::Opt).await?;
            return Ok(VolumeStat { is_dir: false, len: entry.size });
        }
        Ok(VolumeStat { is_dir: false, len: u64::from(asset.header_size) })
    }

    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        if path == MEM {
            let base = self.0.tables.page_data_offset;
            return self.0.body.read(base + range.start..base + range.end).await;
        }
        let (index, part) =
            self.0.locate_part(path).ok_or_else(|| Error::new(ErrorKind::NotFound { path: path.to_owned() }))?;
        let asset = &self.0.tables.assets[index];
        if part != Part::Header {
            let (sp, entry) = self.0.stream_entry(asset, part == Part::Opt).await?;
            return sp.read(entry.offset + range.start..entry.offset + range.end).await;
        }
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
        let has_stream = starpak::split_reference(entry.starpak).is_some();
        let has_opt = starpak::split_reference(entry.opt_starpak).is_some();
        let data_mem_offset = if entry.data.is_null() { None } else { self.inner.tables.mem_offset(entry.data) };
        Ok(Arc::new(RpakAsset {
            format,
            key: ValueKey::file(format!("{}/{path}", self.mount)),
            entry,
            kind,
            volume: self.volume.clone(),
            header_path: Inner::file_name(&self.inner.tables.assets[index]),
            stream: has_stream.then(|| format!("{}.stream", Inner::file_name(&self.inner.tables.assets[index]))),
            opt: has_opt.then(|| format!("{}.opt", Inner::file_name(&self.inner.tables.assets[index]))),
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
    /// Path in `volume` of the asset's streamed data (mandatory starpak), if it has any.
    pub stream: Option<String>,
    /// Path in `volume` of the asset's optional streamed data, if it has any.
    pub opt: Option<String>,
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
                file_len: header.compressed_size,
                state: Mutex::default(),
            }
        } else {
            Body::Raw { op: file.op.clone(), path: file.path.clone(), start: start + header_len as u64 }
        };
        let table_bytes = body.read(0..header.tables_len()).await?;
        let tables = Tables::parse(&header, &table_bytes)?;

        let mut by_guid = HashMap::new();
        for (i, a) in tables.assets.iter().enumerate() {
            by_guid.entry(a.guid).or_insert(i);
        }
        let dir = file.path.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
        let opened = (0..tables.starpaks.len() + tables.opt_starpaks.len()).map(|_| OnceCell::new()).collect();
        let inner = Arc::new(Inner { header, tables, body, by_guid, host: file.op.clone(), dir, opened });
        let mount = source.key().to_string();
        let volume = volume_operator(RpakSource(inner.clone()), &mount);
        let format = FormatId::new("rpak").with_version(Version::new(u32::from(inner.header.version)));
        Ok(Arc::new(RpakData { format, key: source.key().clone(), mount, volume, inner }))
    }
}
