//! `asqd`: the animation data of one animation (`mstudioanimdesc`), read from `rpak.asset`. Apex
//! Legends, asset version 1. Read edge only: bytes and metadata, no convert of its own.
//!
//! Ported from rsx `src/game/rtech/assets/animseq_data.{h,cpp}` (`AnimSeqDataAssetHeader_v1_t`,
//! `LoadAnimSeqDataAsset`). The blob has no stored size and no name; the `aseq` that references it
//! (by guid, per animdesc) knows where its sections start, so only ranged reads are offered.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{Dependency, Metadata};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::matl_json::hex;
use crate::rmdl::B;
use crate::rpak::{RpakAsset, MEM};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

/// The 8-byte asset header: the page pointer to the blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsqdHeader {
    /// Where the blob is in `.mem`, `None` if null.
    pub data_offset: Option<u64>,
}

impl AsqdHeader {
    /// Parses a relocated header; any other length than 8 is `Corrupt`.
    pub fn parse(header: &[u8]) -> Result<Self> {
        if header.len() != 8 {
            return Err(corrupt(format!(
                "asqd header is {} bytes, expected 8",
                header.len()
            )));
        }
        let at = B(header).u64(0)?;
        Ok(Self {
            data_offset: (at != 0 && at != u64::MAX).then_some(at),
        })
    }
}

/// An `asqd`: `Data` + `Metadata`.
#[derive(Debug)]
pub struct AsqdAsset {
    format: FormatId,
    key: ValueKey,
    header: AsqdHeader,
    guid: u64,
    volume: Operator,
}

impl AsqdAsset {
    /// The parsed header.
    pub fn header(&self) -> &AsqdHeader {
        &self.header
    }

    /// `len` bytes at `offset` inside the blob (bounded by the pak's `.mem`; `Corrupt` outside it,
    /// `MissingDependency` when the header has no data pointer).
    pub async fn read(&self, offset: u64, len: u64) -> Result<Bytes> {
        let (start, mem_len) = self.start(offset).await?;
        match start.checked_add(len) {
            Some(end) if end <= mem_len => Ok(self
                .volume
                .read_with(MEM)
                .range(start..end)
                .await?
                .to_bytes()),
            _ => Err(corrupt(format!(
                "asqd {}: {len} bytes at {offset:#x} are outside the pak's data",
                hex(self.guid)
            ))),
        }
    }

    /// Up to `max` bytes at `offset`: fewer when the pak's data ends first (and none when `offset`
    /// is at or past the end). For readers that do not know how long the data is.
    pub async fn read_upto(&self, offset: u64, max: u64) -> Result<Bytes> {
        let (start, mem_len) = self.start(offset).await?;
        let end = start.saturating_add(max).min(mem_len);
        if start >= end {
            return Ok(Bytes::new());
        }
        Ok(self
            .volume
            .read_with(MEM)
            .range(start..end)
            .await?
            .to_bytes())
    }

    /// The absolute `.mem` offset of `offset` in the blob and the length of `.mem`.
    async fn start(&self, offset: u64) -> Result<(u64, u64)> {
        let base = self.header.data_offset.ok_or_else(|| {
            Error::new(ErrorKind::MissingDependency {
                item: format!("asqd {}", hex(self.guid)),
                file: "(the asqd has no data pointer)".into(),
            })
        })?;
        let mem_len = self.volume.stat(MEM).await?.content_length();
        let start = base.checked_add(offset).ok_or_else(|| {
            corrupt(format!(
                "asqd {}: offset {offset:#x} overflows",
                hex(self.guid)
            ))
        })?;
        Ok((start, mem_len))
    }
}

impl Metadata for AsqdAsset {
    fn name(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> Option<&str> {
        Some("asqd")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for AsqdAsset {
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

/// Read edge `rpak.asset -> asqd`.
pub struct AsqdRead;

#[async_trait]
impl Detect for AsqdRead {
    /// The asset kind is `asqd` = 10, version `1.0`; any other asset version is `UnsupportedVersion`
    /// when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"asqd").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

#[async_trait]
impl ReadHandler for AsqdRead {
    /// Parses the header; the blob itself is read later, by [`AsqdAsset::read`].
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "asqd must be read from an rpak.asset".into(),
            })
        })?;
        if asset.entry.kind != *b"asqd" {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: "the asset is not an asqd".into(),
            }));
        }
        if asset.entry.version != 1 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "asqd".into(),
                version: asset.entry.version.to_string(),
            }));
        }
        let header = AsqdHeader::parse(&asset.read_header().await?)?;
        Ok(Arc::new(AsqdAsset {
            format: FormatId::new("asqd").with_version(Version::new(1)),
            key: source.key().clone(),
            header,
            guid: asset.entry.guid,
            volume: asset.volume.clone(),
        }))
    }
}
