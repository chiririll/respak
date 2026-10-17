//! The shared state of an open pak and the volume it serves.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::volume::{VolumeEntry, VolumeSource, VolumeStat};
use respak_core::CancellationToken;
use respak_core::{Error, ErrorKind, Result};
use tokio::sync::OnceCell;

use super::body::Body;
use super::chain::Resolved;
use super::MEM;
use crate::guid::string_to_guid;
use crate::pak::{AssetEntry, PakHeader, Tables};
use crate::starpak::{self, Starpak, StarpakEntry};

/// The parts of an open pak that the volume and the entries share.
pub(super) struct Inner {
    pub(super) header: PakHeader,
    pub(super) tables: Tables,
    pub(super) body: Arc<Body>,
    /// The pak's own path on `host`.
    pub(super) file: String,
    /// The page map and the chain, for a patched pak; built on the first read of page data.
    pub(super) patched: Option<OnceCell<Resolved>>,
    /// GUID -> index into `tables.assets`; the first entry wins on a duplicate.
    pub(super) by_guid: HashMap<u64, usize>,
    /// Where the pak lives, to find the starpaks and the patch chain next to it.
    pub(super) host: Operator,
    pub(super) cancel: CancellationToken,
    /// The pak's directory on `host`: empty, or ending in `/`.
    pub(super) dir: String,
    /// The starpaks, opened on first use (`starpaks` first, then `opt_starpaks`).
    pub(super) opened: Vec<OnceCell<Arc<Starpak>>>,
}

/// Which file of an asset a volume path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
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
    pub(super) fn locate(&self, path: &str) -> Option<usize> {
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
            Some(hex)
                if !hex.is_empty()
                    && hex.len() <= 16
                    && hex.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                u64::from_str_radix(hex, 16).ok()?
            }
            _ => string_to_guid(stem),
        };
        if let Some(&index) = self.by_guid.get(&guid) {
            if self.tables.assets[index]
                .kind_string()
                .eq_ignore_ascii_case(kind)
            {
                return Some((index, part));
            }
        }
        // The guid is the hash of the whole asset name, extension included
        // (`mdl/fx/spark_blast.rmdl`), whose kind (`mdl_`) differs from that extension.
        let index = *self.by_guid.get(&string_to_guid(path))?;
        Some((index, part))
    }

    /// Opens starpak number `index` of the mandatory (or optional) list. A starpak file that is
    /// not next to the pak is `MissingDependency`; an index past the list is `Corrupt`.
    async fn starpak(
        &self,
        asset: &AssetEntry,
        optional: bool,
        index: usize,
    ) -> Result<&Arc<Starpak>> {
        let list = if optional {
            &self.tables.opt_starpaks
        } else {
            &self.tables.starpaks
        };
        let path = list.get(index).ok_or_else(|| {
            Error::corrupt(format!(
                "asset {:#018x}: starpak index {index} is past the pak's {} starpak(s)",
                asset.guid,
                list.len()
            ))
        })?;
        let slot = &self.opened[if optional {
            self.tables.starpaks.len() + index
        } else {
            index
        }];
        slot.get_or_try_init(|| async {
            let file = format!("{}{}", self.dir, starpak::file_name(path));
            match Starpak::open(self.host.clone(), file.clone(), None).await {
                Ok(s) => Ok(Arc::new(s)),
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    Err(Error::new(ErrorKind::MissingDependency {
                        item: Self::file_name(asset),
                        file,
                    }))
                }
                Err(e) => Err(e),
            }
        })
        .await
    }

    /// The starpak and the entry an asset's `.stream` / `.opt` file maps onto. `NotFound` if the
    /// asset has no such reference, or the starpak's table of contents has no entry at its offset.
    async fn stream_entry(
        &self,
        asset: &AssetEntry,
        optional: bool,
    ) -> Result<(&Arc<Starpak>, StarpakEntry)> {
        let reference = if optional {
            asset.opt_starpak
        } else {
            asset.starpak
        };
        let none = || {
            Error::new(ErrorKind::NotFound {
                path: format!("{} streamed data", Self::file_name(asset)),
            })
        };
        let (index, offset) = starpak::split_reference(reference).ok_or_else(none)?;
        let sp = self.starpak(asset, optional, index).await?;
        let entry = sp.entry(offset).ok_or_else(none)?;
        Ok((sp, entry))
    }

    pub(super) fn file_name(asset: &AssetEntry) -> String {
        format!("0x{:016X}.{}", asset.guid, asset.kind_string())
    }
}

pub(super) struct RpakSource(pub(super) Arc<Inner>);

#[async_trait]
impl VolumeSource for RpakSource {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        if path == MEM {
            return Ok(VolumeStat {
                is_dir: false,
                len: self.0.tables.mem_len(),
            });
        }
        let (index, part) = self.0.locate_part(path).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })
        })?;
        let asset = &self.0.tables.assets[index];
        if part != Part::Header {
            let (_, entry) = self.0.stream_entry(asset, part == Part::Opt).await?;
            return Ok(VolumeStat {
                is_dir: false,
                len: entry.size,
            });
        }
        Ok(VolumeStat {
            is_dir: false,
            len: u64::from(asset.header_size),
        })
    }

    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        if path == MEM {
            return self.0.read_mem(range).await;
        }
        let (index, part) = self.0.locate_part(path).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })
        })?;
        let asset = &self.0.tables.assets[index];
        if part != Part::Header {
            let (sp, entry) = self.0.stream_entry(asset, part == Part::Opt).await?;
            return sp
                .read(entry.offset + range.start..entry.offset + range.end)
                .await;
        }
        let header = self.0.tables.header_range(asset).ok_or_else(|| {
            Error::corrupt(format!(
                "asset {:#018x}: header does not fit its page",
                asset.guid
            ))
        })?;
        let mut raw = self.0.read_mem(header).await?.to_vec();
        self.0.tables.relocate_header(asset, &mut raw)?;
        raw.get(range.start as usize..range.end as usize)
            .map(Bytes::copy_from_slice)
            .ok_or_else(|| Error::corrupt("read past the end of an asset header"))
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
            .map(|(_, a)| VolumeEntry {
                name: Inner::file_name(a),
                is_dir: false,
                len: u64::from(a.header_size),
            })
            .collect())
    }
}
