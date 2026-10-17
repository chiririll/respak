//! `starpak`: streaming data files that hold the big parts of rpak assets (texture mips, model
//! vertex data).
//!
//! Ported from rsx `src/game/rtech/starpak_source.h` and `cpakfile.{h,cpp}`
//! (`CPakFile::LoadStarpakEntries`, the starpak offset encoding). See the rsx review in
//! `docs/tasks/10-slice-streamed-mips.md`.
//!
//! # File layout
//! No header and no magic. The **table of contents** is at the end of the file: the last 8
//! bytes are a `u64` entry count, before them come `count` entries of `{u64 offset, u64 size}`.
//! Entries with size 0 are dropped. Entry offsets are 4096-aligned.
//!
//! # The volume
//! ```text
//! common.starpak/
//!   0x0000000000012000     the entry that starts at that offset, sized by the table of contents
//! ```
//!
//! # Cost
//! Opening reads the last 8 bytes, then the table of contents. Nothing else is read until a
//! range is asked for, and the starpak itself is never read whole.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::Container;
use respak_core::value::FileData;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey,
};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

/// Bytes of one table-of-contents entry.
const ENTRY_LEN: u64 = 16;
/// Bytes of the trailing entry count.
const TRAILER_LEN: u64 = 8;

/// One streamed blob of a starpak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarpakEntry {
    /// Offset in the starpak file.
    pub offset: u64,
    /// Size in bytes.
    pub size: u64,
}

/// An asset's reference into a starpak (`AssetEntry::starpak`): `None` for `-1` (or any
/// negative value), else the starpak index (low 12 bits) and the offset (the rest).
pub fn split_reference(reference: i64) -> Option<(usize, u64)> {
    (reference >= 0).then_some(((reference & 0xFFF) as usize, (reference & !0xFFF) as u64))
}

/// The file name of a starpak path as the pak stores it (`paks\Win64\pc_all.starpak`): split on
/// both kinds of slash.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// An opened starpak: its length and table of contents.
#[derive(Debug)]
pub struct Starpak {
    op: Operator,
    path: String,
    start: u64,
    len: u64,
    entries: Vec<StarpakEntry>,
}

impl Starpak {
    /// Reads the table of contents of the starpak at `path` (the file, or `range` of it).
    ///
    /// - A missing file → `NotFound`.
    /// - A table of contents that does not fit the file → `Corrupt`.
    pub async fn open(op: Operator, path: String, range: Option<Range<u64>>) -> Result<Self> {
        let (start, len) = match range {
            Some(r) => (r.start, r.end - r.start),
            None => (0, op.stat(&path).await?.content_length()),
        };
        if len < TRAILER_LEN {
            return Err(corrupt(format!(
                "starpak {path}: {len} bytes is too short for a table of contents"
            )));
        }
        let end = start + len;
        let tail = op
            .read_with(&path)
            .range(end - TRAILER_LEN..end)
            .await?
            .to_vec();
        let count = u64::from_le_bytes(tail[..8].try_into().expect("8 bytes"));
        let toc_len = count
            .checked_mul(ENTRY_LEN)
            .filter(|t| t + TRAILER_LEN <= len);
        let toc_len = toc_len.ok_or_else(|| {
            corrupt(format!(
                "starpak {path}: table of contents ({count} entries) does not fit"
            ))
        })?;
        let raw = if toc_len == 0 {
            Bytes::new()
        } else {
            op.read_with(&path)
                .range(end - TRAILER_LEN - toc_len..end - TRAILER_LEN)
                .await?
                .to_bytes()
        };
        let entries = raw
            .chunks_exact(ENTRY_LEN as usize)
            .map(|c| StarpakEntry {
                offset: u64::from_le_bytes(c[..8].try_into().expect("8 bytes")),
                size: u64::from_le_bytes(c[8..].try_into().expect("8 bytes")),
            })
            .filter(|e| e.size != 0)
            .collect();
        Ok(Self {
            op,
            path,
            start,
            len,
            entries,
        })
    }

    /// The entries, in table order.
    pub fn entries(&self) -> &[StarpakEntry] {
        &self.entries
    }

    /// The entry that starts exactly at `offset`.
    pub fn entry(&self, offset: u64) -> Option<StarpakEntry> {
        self.entries.iter().find(|e| e.offset == offset).copied()
    }

    /// The file's length.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the file is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bytes `range` of the file (ranged read; `Corrupt` past the end).
    pub async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        if range.start > range.end || range.end > self.len {
            return Err(corrupt(format!(
                "starpak {}: read {}..{} past the end ({} bytes)",
                self.path, range.start, range.end, self.len
            )));
        }
        if range.is_empty() {
            return Ok(Bytes::new());
        }
        Ok(self
            .op
            .read_with(&self.path)
            .range(self.start + range.start..self.start + range.end)
            .await?
            .to_bytes())
    }
}

/// The volume name of the entry at `offset`.
fn entry_name(offset: u64) -> String {
    format!("0x{offset:016X}")
}

struct StarpakSource(Arc<Starpak>);

impl StarpakSource {
    fn locate(&self, path: &str) -> Option<StarpakEntry> {
        let hex = path
            .strip_prefix("0x")
            .or_else(|| path.strip_prefix("0X"))?;
        let offset = u64::from_str_radix(hex, 16).ok()?;
        self.0.entry(offset)
    }
}

#[async_trait]
impl VolumeSource for StarpakSource {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        let e = self.locate(path).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })
        })?;
        Ok(VolumeStat {
            is_dir: false,
            len: e.size,
        })
    }

    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        let e = self.locate(path).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })
        })?;
        self.0
            .read(e.offset + range.start..e.offset + range.end)
            .await
    }

    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        if !dir.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .0
            .entries
            .iter()
            .map(|e| VolumeEntry {
                name: entry_name(e.offset),
                is_dir: false,
                len: e.size,
            })
            .collect())
    }
}

/// An opened starpak: `Data` + `Container`.
pub struct StarpakData {
    format: FormatId,
    key: ValueKey,
    mount: String,
    volume: Operator,
    starpak: Arc<Starpak>,
}

impl std::fmt::Debug for StarpakData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StarpakData")
            .field("mount", &self.mount)
            .finish_non_exhaustive()
    }
}

impl StarpakData {
    /// The opened starpak.
    pub fn starpak(&self) -> &Starpak {
        &self.starpak
    }
}

#[async_trait]
impl Container for StarpakData {
    fn volume(&self) -> Operator {
        self.volume.clone()
    }

    fn mount_path(&self) -> &str {
        &self.mount
    }
}

impl Data for StarpakData {
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

/// Read edge `file -> starpak`.
pub struct StarpakRead;

#[async_trait]
impl Detect for StarpakRead {
    /// A starpak has no magic. The extension `starpak` = 1, and a table of contents that fits
    /// the file = +5 (one ranged read of the last 8 bytes).
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if input.extension != Some("starpak") {
            return Ok(None);
        }
        let mut score = 1;
        if let (Some(file), Some(len)) = (input.source.downcast_ref::<FileData>(), input.len) {
            if let Some(tail_at) = len.checked_sub(TRAILER_LEN) {
                let start = file.range.as_ref().map_or(0, |r| r.start);
                let tail = file
                    .op
                    .read_with(&file.path)
                    .range(start + tail_at..start + len)
                    .await;
                if let Ok(tail) = tail {
                    let count = u64::from_le_bytes(tail.to_vec()[..8].try_into().expect("8 bytes"));
                    if count
                        .checked_mul(ENTRY_LEN)
                        .is_some_and(|t| t + TRAILER_LEN <= len)
                    {
                        score += 5;
                    }
                }
            }
        }
        Ok(Some(Detected {
            score,
            version: None,
        }))
    }
}

#[async_trait]
impl ReadHandler for StarpakRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "starpak must be read from a file".into(),
            })
        })?;
        let starpak =
            Arc::new(Starpak::open(file.op.clone(), file.path.clone(), file.range.clone()).await?);
        let mount = source.key().to_string();
        let volume = volume_operator(StarpakSource(starpak.clone()), &mount);
        Ok(Arc::new(StarpakData {
            format: FormatId::new("starpak"),
            key: source.key().clone(),
            mount,
            volume,
            starpak,
        }))
    }
}
