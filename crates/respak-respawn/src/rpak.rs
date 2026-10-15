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
//! # Patches
//! A patched pak (`common(03).rpak`) stores the final tables and only part of the page data. Its
//! `.mem` is a map built from the pak's edit stream (see `patch.rs`): a read follows the map into
//! the pak itself and the lower files of the chain, which are opened, and the map built, on the
//! first read of page data. Listing never touches them. A lower file that is not there is
//! `MissingDependency`. Opening `common.rpak` next to a `patch_master.rpak` opens the newest patch
//! it names instead; opening a patch file directly opens that file.
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
use respak_core::CancellationToken;
use respak_core::capability::{Container, Dependency, Metadata};
use respak_core::value::FileData;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{
    magic, u16le, Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey,
    Version,
};
use tokio::sync::{Mutex, OnceCell};

use crate::guid::string_to_guid;
use crate::oodle::{self, OodleError, Scanner};
use crate::pak::{AssetEntry, PagePtr, PakHeader, Tables};
use crate::patch::{PageMap, Source};
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
    Rtech {
        op: Operator,
        path: String,
        start: u64,
        header_len: usize,
        len: u64,
        file_len: u64,
        state: Mutex<Progress>,
        cancel: CancellationToken,
    },
    /// Oodle-compressed: cut into segments that decode on their own, each only when a read needs it.
    Oodle {
        op: Operator,
        path: String,
        start: u64,
        len: u64,
        stream_len: u64,
        state: Mutex<OodleProgress>,
        cancel: CancellationToken,
    },
}

/// How far an Oodle body has been read and scanned, and the segments decoded so far.
#[derive(Default)]
struct OodleProgress {
    /// The start of the compressed stream (after the pak header) read so far.
    input: Vec<u8>,
    /// Where the segments start, as far as the input has been walked.
    scan: Scanner,
    /// Decoded segments by index.
    segments: HashMap<usize, Bytes>,
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

/// `Cancelled` once the host has triggered the call's token.
fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(Error::new(ErrorKind::Cancelled));
    }
    Ok(())
}

/// First read of a compressed pak, and the least growth of the input between decoding attempts.
const MIN_INPUT: u64 = 1 << 20;

impl Body {
    /// The decoded body, at least `upto` bytes of it (fewer only if the body is shorter).
    async fn decoded(&self, upto: u64) -> Result<Bytes> {
        let Self::Rtech { op, path, start, header_len, len, file_len, state, cancel } = self else {
            unreachable!("only compressed bodies are decoded")
        };
        let mut progress = state.lock().await;
        while (progress.body.len() as u64) < upto && !progress.done {
            check(cancel)?;
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

    /// The decoded bytes of an Oodle body in `range` (inside the body).
    async fn oodle_read(&self, range: Range<u64>) -> Result<Bytes> {
        let Self::Oodle { op, path, start, len, stream_len, state, cancel } = self else {
            unreachable!("only Oodle bodies are read this way")
        };
        if range.start > range.end || range.end > *len {
            return Err(corrupt("read past the end of the decompressed pak"));
        }
        let (from, to) = (usize::try_from(range.start), usize::try_from(range.end));
        let (Ok(from), Ok(to), Ok(total)) = (from, to, usize::try_from(*len)) else {
            return Err(corrupt("rpak: body too large"));
        };
        if from == to {
            return Ok(Bytes::new());
        }
        let mut progress = state.lock().await;
        let p = &mut *progress;
        // Walk the stream until the last segment needed is known, and where it ends.
        let segment_of = |p: &OodleProgress, at: usize| p.scan.segments.iter().rposition(|s| s.output <= at);
        let (first, last) = loop {
            check(cancel)?;
            if p.scan.scanned() >= to {
                let last = segment_of(p, to - 1).ok_or_else(|| corrupt("rpak: no Oodle segment"))?;
                if p.scan.scanned() >= total || p.scan.segments.len() > last + 1 {
                    let first = segment_of(p, from).ok_or_else(|| corrupt("rpak: no Oodle segment"))?;
                    break (first, last);
                }
            }
            // One block further once the range is covered, else up to its end.
            let upto = if p.scan.scanned() >= to { p.scan.scanned() + 1 } else { to };
            match p.scan.advance(&p.input, total, upto) {
                Ok(()) => {}
                Err(OodleError::NeedInput { at_least }) => {
                    let have = p.input.len() as u64;
                    let target = (at_least as u64).max(2 * have).max(MIN_INPUT).min(*stream_len);
                    if target <= have {
                        return Err(corrupt("rpak: the compressed stream runs past the end of the file"));
                    }
                    let more = op.read_with(path).range(start + have..start + target).await?;
                    p.input.extend_from_slice(&more.to_bytes());
                }
                Err(e) => return Err(Error::custom("oodle", e)),
            }
        };
        for k in first..=last {
            if p.segments.contains_key(&k) {
                continue;
            }
            check(cancel)?;
            let seg = p.scan.segments[k];
            let (out_end, in_end) = match p.scan.segments.get(k + 1) {
                Some(next) => (next.output, next.input),
                None => (total, p.scan.position()),
            };
            let input = p.input[seg.input..in_end].to_vec();
            let decoded = tokio::task::spawn_blocking(move || {
                let mut out = vec![0u8; out_end - seg.output];
                oodle::decode_segment(&input, &mut out).map(|()| out)
            })
            .await
            .map_err(|e| Error::custom("decompression task", e))?
            .map_err(|e| Error::custom("oodle", e))?;
            p.segments.insert(k, Bytes::from(decoded));
        }
        let at = |k: usize| p.scan.segments[k].output;
        if first == last {
            let base = at(first);
            return Ok(p.segments[&first].slice(from - base..to - base));
        }
        let mut out = Vec::with_capacity(to - from);
        for k in first..=last {
            let seg = &p.segments[&k];
            let base = at(k);
            out.extend_from_slice(&seg[from.max(base) - base..(to - base).min(seg.len())]);
        }
        Ok(Bytes::from(out))
    }

    /// `range` is relative to the start of the body (the byte after the header).
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        match self {
            Self::Oodle { .. } => self.oodle_read(range).await,
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
    body: Arc<Body>,
    /// The pak's own path on `host`.
    file: String,
    /// The page map and the chain, for a patched pak; built on the first read of page data.
    patched: Option<OnceCell<Resolved>>,
    /// GUID -> index into `tables.assets`; the first entry wins on a duplicate.
    by_guid: HashMap<u64, usize>,
    /// Where the pak lives, to find the starpaks and the patch chain next to it.
    host: Operator,
    cancel: CancellationToken,
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

/// One file of a patch chain: its body, and where it starts in `F` (the bodies back to back).
struct Link {
    start: u64,
    len: u64,
    body: Arc<Body>,
}

/// A patched pak's `.mem`: the map, the replacement data and the files it points into.
struct Resolved {
    map: PageMap,
    /// The edit stream; the replacement data starts at `map.replacement_at`.
    stream: Bytes,
    chain: Vec<Link>,
}

impl Resolved {
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        let mut out = Vec::with_capacity((range.end - range.start) as usize);
        for piece in self.map.locate(range)? {
            match piece.source {
                Source::Replacement(at) => {
                    let from = self.map.replacement_at as u64 + at;
                    let bytes = self.stream.get(from as usize..(from + piece.len) as usize);
                    out.extend_from_slice(bytes.ok_or_else(|| corrupt("rpak patch: replacement data past the edit stream"))?);
                }
                Source::File(mut at) => {
                    let mut left = piece.len;
                    while left > 0 {
                        let link = self
                            .chain
                            .iter()
                            .find(|l| (l.start..l.start + l.len).contains(&at))
                            .ok_or_else(|| corrupt("rpak patch: the script reads past the end of the pak files"))?;
                        let n = left.min(link.start + link.len - at);
                        out.extend_from_slice(&link.body.read(at - link.start..at - link.start + n).await?);
                        at += n;
                        left -= n;
                    }
                }
            }
        }
        Ok(Bytes::from(out))
    }
}

impl Inner {
    /// The bytes of `.mem` (the pages back to back) in `range`.
    async fn read_mem(&self, range: Range<u64>) -> Result<Bytes> {
        let Some(patched) = &self.patched else {
            let base = self.tables.page_data_offset;
            return self.body.read(base + range.start..base + range.end).await;
        };
        patched.get_or_try_init(|| self.resolve()).await?.read(range).await
    }

    /// Reads the edit stream, builds the page map and opens the lower files of the chain.
    async fn resolve(&self) -> Result<Resolved> {
        let patch = self.tables.patch.as_ref().expect("a patched pak has a patch block");
        let tables_end = self.header.tables_len();
        let stream = self.body.read(tables_end..tables_end + patch.stream_size).await?;
        let stored: u64 = self.tables.pages[patch.patch_pages as usize..].iter().map(|p| u64::from(p.size)).sum();
        let map = PageMap::build(&stream, self.tables.mem_len(), stored, self.tables.page_data_offset)?;

        let top_len = self.header.body_len();
        let mut chain = vec![Link { start: 0, len: top_len, body: self.body.clone() }];
        let stem = self.file.rsplit_once('/').map_or(self.file.as_str(), |(_, f)| f);
        let stem = stem.strip_suffix(".rpak").unwrap_or(stem);
        let stem = stem.split('(').next().unwrap_or(stem);
        let mut start = top_len;
        for &index in &patch.indices {
            let name = if index == 0 { format!("{stem}.rpak") } else { format!("{stem}({index:02}).rpak") };
            let path = format!("{}{name}", self.dir);
            let (header, body) = match open_body(&self.host, &path, 0, None, &self.cancel).await {
                Ok(opened) => opened,
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    return Err(Error::new(ErrorKind::MissingDependency { item: self.file.clone(), file: path }));
                }
                Err(e) => return Err(e),
            };
            let len = header.body_len();
            chain.push(Link { start, len, body: Arc::new(body) });
            start += len;
        }
        Ok(Resolved { map, stream, chain })
    }

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
            return self.0.read_mem(range).await;
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
        let mut raw = self.0.read_mem(header).await?.to_vec();
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

/// Opens the pak at `path` (the whole file, or `len` bytes from `start`): reads and checks its
/// header and returns it with the body reader. `Corrupt` if the file is shorter than the header
/// says.
async fn open_body(
    op: &Operator,
    path: &str,
    start: u64,
    len: Option<u64>,
    cancel: &CancellationToken,
) -> Result<(PakHeader, Body)> {
    let file_len = match len {
        Some(len) => len,
        None => op.stat(path).await?.content_length(),
    };
    let head_len = file_len.min(PakHeader::MAX_LEN as u64);
    let head = op.read_with(path).range(start..start + head_len).await?.to_vec();
    let header = PakHeader::parse(&head)?;

    let compressed = header.is_rtech();
    let needed = if compressed {
        header.compressed_size
    } else if header.is_oodle() {
        // The compressed size leaves out the header.
        header.compressed_size.saturating_add(header.header_len as u64)
    } else {
        header.decompressed_size
    };
    if file_len < needed {
        return Err(corrupt(format!("rpak: file has {file_len} bytes, its header says {needed}")));
    }
    let header_len = header.header_len;
    let body = if header.is_oodle() {
        Body::Oodle {
            op: op.clone(),
            path: path.to_owned(),
            start: start + header_len as u64,
            len: header.body_len(),
            stream_len: header.compressed_size,
            state: Mutex::default(),
            cancel: cancel.clone(),
        }
    } else if compressed {
        Body::Rtech {
            op: op.clone(),
            path: path.to_owned(),
            start,
            header_len,
            len: header.body_len(),
            file_len: header.compressed_size,
            state: Mutex::default(),
            cancel: cancel.clone(),
        }
    } else {
        Body::Raw { op: op.clone(), path: path.to_owned(), start: start + header_len as u64 }
    };
    Ok((header, body))
}

/// The path of the newest patch of `path` that a `patch_master.rpak` in its directory names, if
/// there is one and `path` is not a patch itself. Anything wrong with the master means no patch.
async fn newest_patch(op: &Operator, path: &str, cancel: &CancellationToken) -> Option<String> {
    let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, f)| (&path[..d.len() + 1], f));
    let stem = name.strip_suffix(".rpak")?;
    if stem.eq_ignore_ascii_case("patch_master") || stem.ends_with(')') {
        return None;
    }
    let entries = patch_master(op, &format!("{dir}patch_master.rpak"), cancel).await?;
    let (_, number) = entries.into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    if number == 0 {
        return None;
    }
    let patch = format!("{dir}{stem}({number:02}).rpak");
    op.stat(&patch).await.ok().map(|_| patch)
}

/// The entries of the `Ptch` asset of a patch master: pak file name and the number of its newest
/// patch. The asset header is `i32 unk, i32 count, ptr names, ptr versions`: `names` points at
/// `count` pointers to NUL-terminated strings, `versions` at `count` bytes.
async fn patch_master(op: &Operator, path: &str, cancel: &CancellationToken) -> Option<Vec<(String, u8)>> {
    let (header, body) = open_body(op, path, 0, None, cancel).await.ok()?;
    if header.patch_count != 0 {
        return None;
    }
    let tables = Tables::parse(&header, &body.read(0..header.tables_len()).await.ok()?).ok()?;
    let asset = tables.assets.iter().find(|a| a.kind_string() == "Ptch")?;
    let mem = |at: u64, len: u64| {
        let (body, base) = (&body, tables.page_data_offset + at);
        async move { body.read(base..base + len).await.ok() }
    };
    let ptr = |b: &[u8]| {
        let p = PagePtr {
            page: u32::from_le_bytes(b.get(..4)?.try_into().ok()?),
            offset: u32::from_le_bytes(b.get(4..8)?.try_into().ok()?),
        };
        if p.is_null() {
            return None;
        }
        tables.mem_offset(p)
    };
    let head = tables.header_range(asset)?;
    let h = mem(head.start, head.end - head.start).await?;
    let count = u64::from(u32::from_le_bytes(h.get(4..8)?.try_into().ok()?));
    let (names, versions) = (ptr(h.get(8..16)?)?, ptr(h.get(16..24)?)?);
    if count > 4096 {
        return None;
    }
    let pointers = mem(names, count * 8).await?;
    let versions = mem(versions, count).await?;
    let mut out = Vec::new();
    for (i, p) in pointers.chunks_exact(8).enumerate() {
        let at = ptr(p)?;
        let bytes = mem(at, 256.min(tables.mem_len().saturating_sub(at))).await?;
        let name = bytes.split(|&b| b == 0).next()?;
        out.push((String::from_utf8_lossy(name).into_owned(), *versions.get(i)?));
    }
    Some(out)
}

/// Read edge `file -> rpak`.
#[derive(Debug, Clone, Copy)]
pub struct RpakRead {
    patches: bool,
}

impl Default for RpakRead {
    fn default() -> Self {
        Self::new(true)
    }
}

impl RpakRead {
    /// With `patches` off, a patch master is not consulted and a patch file is refused as
    /// `UnsupportedVersion`.
    pub fn new(patches: bool) -> Self {
        Self { patches }
    }
}

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
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let file = source
            .downcast_ref::<FileData>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "rpak must be read from a file".into() }))?;
        let start = file.range.as_ref().map_or(0, |r| r.start);
        let len = file.range.as_ref().map(|r| r.end - r.start);
        // `common.rpak` next to a `patch_master.rpak` is opened as its newest patch.
        let patch = match (self.patches, &file.range) {
            (true, None) => newest_patch(&file.op, &file.path, &cx.cancel).await,
            _ => None,
        };
        let path = patch.as_ref().unwrap_or(&file.path).clone();
        let (header, body) = open_body(&file.op, &path, start, if patch.is_some() { None } else { len }, &cx.cancel).await?;
        if !self.patches && header.patch_count > 0 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "rpak".into(),
                version: format!("{} (patch file, patchCount {}; patches are ignored)", header.version, header.patch_count),
            }));
        }
        let table_bytes = body.read(0..header.tables_len()).await?;
        let tables = Tables::parse(&header, &table_bytes)?;

        let mut by_guid = HashMap::new();
        for (i, a) in tables.assets.iter().enumerate() {
            by_guid.entry(a.guid).or_insert(i);
        }
        let dir = path.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
        let opened = (0..tables.starpaks.len() + tables.opt_starpaks.len()).map(|_| OnceCell::new()).collect();
        let patched = tables.patch.is_some().then(OnceCell::new);
        let inner = Arc::new(Inner {
            header,
            tables,
            body: Arc::new(body),
            file: path,
            patched,
            by_guid,
            host: file.op.clone(),
            cancel: cx.cancel.clone(),
            dir,
            opened,
        });
        let mount = source.key().to_string();
        let volume = volume_operator(RpakSource(inner.clone()), &mount);
        let format = FormatId::new("rpak").with_version(Version::new(u32::from(inner.header.version)));
        Ok(Arc::new(RpakData { format, key: source.key().clone(), mount, volume, inner }))
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    use crate::testing::oodle_stored;

    const BLOCK: usize = 0x40000;

    async fn body(stream: Vec<u8>, len: usize) -> (Body, Operator, CancellationToken) {
        let op = Operator::new(opendal::services::Memory::default()).unwrap();
        let stream_len = stream.len() as u64;
        let at = 100; // some bytes before the stream: the header
        let cancel = CancellationToken::new();
        let mut file = vec![0u8; at];
        file.extend(stream);
        op.write("p", file).await.unwrap();
        let body = Body::Oodle {
            op: op.clone(),
            path: "p".into(),
            start: at as u64,
            len: len as u64,
            stream_len,
            state: Mutex::default(),
            cancel: cancel.clone(),
        };
        (body, op, cancel)
    }

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8 ^ (i / BLOCK) as u8).collect()
    }

    #[tokio::test]
    async fn reads_across_segments_and_decodes_only_what_it_needs() {
        let data = data(7 * BLOCK + 123);
        // segments of 2 blocks: [0,2) [2,4) [4,6) [6,7.x)
        let (body, _, _) = body(oodle_stored(&data, 2), data.len()).await;
        // inside the last segment first: earlier ones stay undecoded
        let tail = (6 * BLOCK + 10) as u64..data.len() as u64;
        assert_eq!(body.read(tail.clone()).await.unwrap(), Bytes::copy_from_slice(&data[tail.start as usize..]));
        let Body::Oodle { state, .. } = &body else { unreachable!() };
        assert_eq!(state.lock().await.segments.len(), 1);
        // across three segments
        let r = (BLOCK as u64 + 5)..(5 * BLOCK as u64 + 5);
        assert_eq!(body.read(r.clone()).await.unwrap(), Bytes::copy_from_slice(&data[r.start as usize..r.end as usize]));
        assert_eq!(state.lock().await.segments.len(), 4);
        // empty and out of range
        assert!(body.read(5..5).await.unwrap().is_empty());
        assert!(body.read(0..data.len() as u64 + 1).await.is_err());
    }

    #[tokio::test]
    async fn a_triggered_token_stops_decoding_between_segments() {
        let data = data(7 * BLOCK + 123);
        let (body, _, cancel) = body(oodle_stored(&data, 2), data.len()).await;
        let tail = (6 * BLOCK) as u64..data.len() as u64;
        body.read(tail).await.unwrap();
        cancel.cancel();
        let err = body.read(0..(5 * BLOCK) as u64).await.unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
        let Body::Oodle { state, .. } = &body else { unreachable!() };
        assert_eq!(state.lock().await.segments.len(), 1, "no segment was decoded after the cancel");
    }

    #[tokio::test]
    async fn a_stream_that_ends_early_is_corrupt() {
        let data = data(3 * BLOCK);
        let mut stream = oodle_stored(&data, 1);
        stream.truncate(stream.len() - 1000);
        let (body, _, _) = body(stream, data.len()).await;
        assert!(body.read(0..data.len() as u64).await.is_err());
        // the part before the cut still reads
        assert_eq!(body.read(0..10).await.unwrap(), Bytes::copy_from_slice(&data[..10]));
    }
}
