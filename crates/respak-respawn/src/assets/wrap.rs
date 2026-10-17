//! `wrap`: a loose game file (script, map, config) kept in a pak, read from `rpak.asset`. Apex
//! Legends, asset versions 1 and 7. The value is the original file as a [`Blob`].
//!
//! Ported from rsx `src/game/rtech/assets/wrap.{h,cpp}` (`WrapAssetHeader_v1_t`,
//! `WrapAssetHeader_v7_t`, `GetWrapAssetData`, `Wrap_GetPathWithSwappedExtensions`); see
//! `docs/formats/wrap.md`. Read edge only.
//!
//! The data is read and decoded when first asked for, then kept.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Blob, Dependency, Metadata};
use respak_core::reader::Reader;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};
use tokio::sync::OnceCell;

use crate::guid::hex;
use crate::oodle;
use crate::rpak::{detect_kind, rpak_asset, RpakAsset, MEM};

const V1_LEN: usize = 0x10;
const V7_LEN: usize = 0x28;

/// Flags of a v7 header.
const COMPRESSED: u16 = 0x1;
const PERMANENT: u16 = 0x4;
const STREAMED: u16 = 0x10;

/// Longest path taken.
const MAX_PATH: u64 = 4096;

/// Biggest file accepted; real ones are a few megabytes.
const MAX_SIZE: u64 = 1 << 30;

/// Extensions rsx treats as text (a trailing NUL is dropped).
const TEXT: [&str; 5] = ["txt", "nut", "gnut", "res", "ent"];

/// A parsed `wrap` header (either version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapHeader {
    /// The asset version, 1 or 7.
    pub version: u32,
    /// Where the path is in `.mem` (v7 only).
    pub path_offset: Option<u64>,
    /// Where the data is in `.mem`, `None` if null.
    pub data_offset: Option<u64>,
    /// Stored size (v1: the file size).
    pub cmp_size: u32,
    /// File size.
    pub dcmp_size: u32,
    /// Length of the path up to the end of the real file name (v7).
    pub path_size: u16,
    /// Where the name rsx shows starts in the path (v7).
    pub skip_first_folder: u16,
    /// The flags (v7).
    pub flags: u16,
}

impl WrapHeader {
    /// Parses a relocated header; a length other than the version's is `Corrupt`, a version other
    /// than 1 or 7 is `UnsupportedVersion`.
    pub fn parse(version: u32, header: &[u8]) -> Result<Self> {
        let r = Reader::new("wrap", header);
        let pointer = |at: usize| -> Result<Option<u64>> {
            let p = r.u64(at)?;
            Ok((p != 0 && p != u64::MAX).then_some(p))
        };
        let expect = |len: usize| {
            if header.len() == len {
                Ok(())
            } else {
                Err(Error::corrupt(format!(
                    "wrap v{version} header is {} bytes, expected {len}",
                    header.len()
                )))
            }
        };
        match version {
            1 => {
                expect(V1_LEN)?;
                let size = r.u32(0)?;
                Ok(Self {
                    version,
                    path_offset: None,
                    data_offset: pointer(8)?,
                    cmp_size: size,
                    dcmp_size: size,
                    path_size: 0,
                    skip_first_folder: 0,
                    flags: 0,
                })
            }
            7 => {
                expect(V7_LEN)?;
                Ok(Self {
                    version,
                    path_offset: pointer(0)?,
                    data_offset: pointer(8)?,
                    cmp_size: r.u32(0x14)?,
                    dcmp_size: r.u32(0x18)?,
                    path_size: r.u16(0x1C)?,
                    skip_first_folder: r.u16(0x1E)?,
                    flags: r.u16(0x22)?,
                })
            }
            v => Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "wrap".into(),
                version: v.to_string(),
            })),
        }
    }

    /// The stored data is smaller than the file, so it is encoded. Equal sizes are stored data
    /// (rsx asserts on them).
    fn should_decompress(&self) -> bool {
        self.cmp_size < self.dcmp_size
    }

    /// Bytes in front of the data in a streamed entry ("typically 2").
    fn skip_size(&self) -> u64 {
        if self.should_decompress() {
            0
        } else {
            u64::from(self.cmp_size - self.dcmp_size)
        }
    }

    /// The data is Oodle compressed.
    pub fn is_compressed(&self) -> bool {
        self.flags & COMPRESSED != 0 && self.should_decompress()
    }

    /// The data is in a starpak.
    pub fn is_streamed(&self) -> bool {
        self.flags & STREAMED != 0
    }

    /// Bytes read from the store.
    fn wrap_size(&self) -> u64 {
        u64::from(if self.is_compressed() {
            self.cmp_size
        } else {
            self.dcmp_size
        })
    }
}

/// The names of a file: the path rsx shows, and its extension.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    name: String,
    extension: String,
}

/// Splits a stored path (`scripts/vscripts/foo.gnut.client`) into the name rsx shows and the
/// extension of the real file. `path_size` ends the real file name; what follows is the VM
/// extension, which `swap` moves in front of the file's extension (`foo.client.gnut`).
fn split_path(path: &[u8], path_size: u16, skip: u16, swap: bool) -> Result<Name> {
    let (size, skip) = (usize::from(path_size), usize::from(skip));
    if size > path.len() || skip > size {
        return Err(Error::corrupt(format!(
            "wrap path of {} bytes has sizes {size} and {skip}",
            path.len()
        )));
    }
    let real = &path[skip..size];
    let vm = &path[size..];
    let dot = real
        .iter()
        .rposition(|&b| b == b'.')
        .filter(|&dot| !real[dot..].contains(&b'/'));
    let extension = dot.map_or(&[][..], |dot| &real[dot + 1..]);
    let named = match dot {
        Some(dot) if swap => [&real[..dot], vm, &real[dot..]].concat(),
        _ => [real, vm].concat(),
    };
    Ok(Name {
        name: String::from_utf8_lossy(&named).into_owned(),
        extension: String::from_utf8_lossy(extension).to_ascii_lowercase(),
    })
}

/// A `wrap`: `Data` + `Blob` + `Metadata`.
#[derive(Debug)]
pub struct WrapAsset {
    format: FormatId,
    key: ValueKey,
    header: WrapHeader,
    name: Option<String>,
    text: bool,
    guid: u64,
    volume: Operator,
    stream: Option<String>,
    opt: Option<String>,
    bytes: OnceCell<Bytes>,
}

impl WrapAsset {
    /// The parsed header.
    pub fn header(&self) -> &WrapHeader {
        &self.header
    }

    /// The file, decoded.
    async fn load(&self) -> Result<&Bytes> {
        self.bytes.get_or_try_init(|| self.fetch()).await
    }

    async fn fetch(&self) -> Result<Bytes> {
        let h = &self.header;
        let size = h.wrap_size();
        let stored = if h.is_streamed() {
            let path = self
                .stream
                .as_deref()
                .or(self.opt.as_deref())
                .ok_or_else(|| {
                    Error::new(ErrorKind::MissingDependency {
                        item: format!("wrap {}", hex(self.guid)),
                        file: "(no starpak reference)".into(),
                    })
                })?;
            let from = h.skip_size();
            let len = self.volume.stat(path).await?.content_length();
            self.range(path, from, size, len).await?
        } else {
            let at = h.data_offset.ok_or_else(|| {
                Error::new(ErrorKind::MissingDependency {
                    item: format!("wrap {}", hex(self.guid)),
                    file: "(the wrap has no data pointer)".into(),
                })
            })?;
            // A build leftover: permanent files that are not compressed start 2 bytes late.
            let lead = u64::from(
                !h.is_compressed() && h.flags & (COMPRESSED | PERMANENT) == COMPRESSED | PERMANENT,
            ) * 2;
            let len = self.volume.stat(MEM).await?.content_length();
            self.range(MEM, at.saturating_add(lead), size, len).await?
        };

        let mut file = if h.is_compressed() {
            Bytes::from(
                oodle::decompress(&stored, u64::from(h.dcmp_size))
                    .map_err(|e| Error::custom("oodle", e))?,
            )
        } else {
            stored
        };
        if self.text && file.last() == Some(&0) {
            file.truncate(file.len() - 1);
        }
        Ok(file)
    }

    async fn range(&self, path: &str, from: u64, len: u64, file_len: u64) -> Result<Bytes> {
        match from.checked_add(len) {
            Some(end) if end <= file_len => Ok(self
                .volume
                .read_with(path)
                .range(from..end)
                .await?
                .to_bytes()),
            _ => Err(Error::corrupt(format!(
                "wrap {}: {len} bytes at {from:#x} are outside {path}",
                hex(self.guid)
            ))),
        }
    }
}

#[async_trait]
impl Blob for WrapAsset {
    fn len(&self) -> Option<u64> {
        (!self.text).then_some(u64::from(self.header.dcmp_size))
    }

    async fn read(&self, range: Range<u64>, _cx: &Cx) -> Result<Bytes> {
        let file = self.load().await?;
        let end = usize::try_from(range.end).map_or(file.len(), |e| e.min(file.len()));
        let start = usize::try_from(range.start).map_or(end, |s| s.min(end));
        Ok(file.slice(start..end))
    }
}

impl Metadata for WrapAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("wrap")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for WrapAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_blob(&self) -> Option<&dyn Blob> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// Read edge `rpak.asset -> wrap`.
pub struct WrapRead;

#[async_trait]
impl Detect for WrapRead {
    /// The asset kind is `wrap` = 10; the version is the asset version, and any other than 1 or 7
    /// is `UnsupportedVersion` when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"wrap"))
    }
}

/// The NUL-terminated path at `.mem` offset `at`.
async fn read_path(asset: &RpakAsset, at: u64) -> Result<Vec<u8>> {
    let mem_len = asset.volume.stat(MEM).await?.content_length();
    let bytes = crate::mem::read_clamped(&asset.volume, MEM, at, MAX_PATH, mem_len).await?;
    let nul = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::corrupt("wrap path has no end"))?;
    Ok(bytes[..nul].to_vec())
}

#[async_trait]
impl ReadHandler for WrapRead {
    /// Parses the header and the path; the file is read later, when the blob is.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"wrap")?;
        let header = WrapHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        if u64::from(header.dcmp_size) > MAX_SIZE || u64::from(header.cmp_size) > MAX_SIZE {
            return Err(Error::corrupt("wrap size is not plausible"));
        }

        let (name, text) = match header.path_offset {
            Some(at) => {
                let path = read_path(asset, at).await?;
                let n = split_path(&path, header.path_size, header.skip_first_folder, false)?;
                let text = TEXT.contains(&n.extension.as_str());
                (Some(n.name), text)
            }
            None => (None, false),
        };

        Ok(Arc::new(WrapAsset {
            format: FormatId::new("wrap").with_version(Version::new(header.version)),
            key: source.key().clone(),
            header,
            name,
            text,
            guid: asset.entry.guid,
            volume: asset.volume.clone(),
            stream: asset.stream.clone(),
            opt: asset.opt.clone(),
            bytes: OnceCell::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v7(cmp: u32, dcmp: u32, flags: u16) -> Vec<u8> {
        let mut h = vec![0u8; V7_LEN];
        h[0x14..0x18].copy_from_slice(&cmp.to_le_bytes());
        h[0x18..0x1C].copy_from_slice(&dcmp.to_le_bytes());
        h[0x22..0x24].copy_from_slice(&flags.to_le_bytes());
        h
    }

    #[test]
    fn derived_fields_follow_the_sizes_and_flags() {
        let h = WrapHeader::parse(7, &v7(10, 10, 0x1)).unwrap();
        assert!(!h.is_compressed(), "equal sizes are stored");
        assert_eq!((h.skip_size(), h.wrap_size()), (0, 10));
        let h = WrapHeader::parse(7, &v7(12, 10, 0x10)).unwrap();
        assert!(h.is_streamed() && !h.is_compressed());
        assert_eq!((h.skip_size(), h.wrap_size()), (2, 10));
        let h = WrapHeader::parse(7, &v7(6, 10, 0x1)).unwrap();
        assert!(h.is_compressed());
        assert_eq!((h.skip_size(), h.wrap_size()), (0, 6));
        let h = WrapHeader::parse(7, &v7(6, 10, 0)).unwrap();
        assert!(!h.is_compressed(), "the flag is needed too");
    }

    #[test]
    fn headers_of_the_wrong_length_or_version_are_errors() {
        assert!(matches!(
            WrapHeader::parse(7, &[0; 0x20]).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
        assert!(matches!(
            WrapHeader::parse(1, &[0; V7_LEN]).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
        assert!(matches!(
            WrapHeader::parse(3, &[0; V7_LEN]).unwrap_err().kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
    }

    #[test]
    fn the_vm_extension_can_move_before_the_extension() {
        let path = b"scripts/vscripts/foo.gnut.client";
        let size = "scripts/vscripts/foo.gnut".len() as u16;
        let n = split_path(path, size, 8, false).unwrap();
        assert_eq!(n.name, "vscripts/foo.gnut.client");
        assert_eq!(n.extension, "gnut");
        let n = split_path(path, size, 8, true).unwrap();
        assert_eq!(n.name, "vscripts/foo.client.gnut");
        let plain = split_path(b"cfg/a.txt", 9, 4, true).unwrap();
        assert_eq!(plain.name, "a.txt");
        assert!(split_path(path, 99, 8, false).is_err());
        assert!(split_path(path, 4, 8, false).is_err());
    }
}
