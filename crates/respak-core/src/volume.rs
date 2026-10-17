//! Containers as OpenDAL services: [`VolumeSource`] is the small read-only trait a container
//! implements, and [`volume_operator`] turns it into an [`Operator`] (docs/design/project.md §6).

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::raw::{oio, OpList, OpRead, OpStat, RpRead, RpStat, ServiceInfo};
use opendal::{Buffer, BytesRange, Capability, MetadataBuilder, OperationContext, Operator};

use crate::error::{Error, ErrorKind, Result};

/// What `stat` says about a path in a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeStat {
    /// Whether the path is a directory.
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub len: u64,
}

/// One child of a directory in a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeEntry {
    /// The child's own name, without its directory.
    pub name: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub len: u64,
}

/// A read-only tree of files, served by a container. Paths are relative to the volume, use `/`
/// and have no leading or trailing slash; the empty path is the root directory.
///
/// Implementations answer from tables they already hold. `read` should touch only the bytes
/// of `range` (docs/design/project.md §6).
#[async_trait]
pub trait VolumeSource: Send + Sync + 'static {
    /// Metadata of `path`; `NotFound` if there is no such path.
    async fn stat(&self, path: &str) -> Result<VolumeStat>;

    /// The bytes `range` of the file at `path`. `range` is already clamped to the file.
    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes>;

    /// The direct children of the directory `dir`, in a stable order.
    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>>;
}

/// Wraps a [`VolumeSource`] as a read-only [`Operator`]. `name` shows up in logs and errors.
pub fn volume_operator(source: impl VolumeSource, name: &str) -> Operator {
    let service = VolumeService {
        source: Arc::new(source),
        info: ServiceInfo::new("respak-volume", "/", name),
    };
    Operator::from_parts(OperationContext::default(), Arc::new(service))
}

fn to_opendal(e: Error) -> opendal::Error {
    let kind = match e.kind() {
        ErrorKind::NotFound { .. } => opendal::ErrorKind::NotFound,
        ErrorKind::Storage(inner) => inner.kind(),
        _ => opendal::ErrorKind::Unexpected,
    };
    // Keep the original error as the source, so it comes back out unchanged (`Error::from`).
    opendal::Error::new(kind, e.to_string()).set_source(e)
}

/// `a/b/` -> `a/b`, `/` -> ``.
fn volume_path(path: &str) -> &str {
    path.trim_matches('/')
}

struct VolumeService {
    source: Arc<dyn VolumeSource>,
    info: ServiceInfo,
}

impl fmt::Debug for VolumeService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VolumeService")
            .field("name", &self.info.name())
            .finish()
    }
}

impl opendal::raw::Service for VolumeService {
    type Reader = oio::StreamReader<VolumeReader>;
    type Writer = ();
    type Lister = VolumeLister;
    type Deleter = ();
    type Copier = ();
    type Composer = ();

    fn info(&self) -> ServiceInfo {
        self.info.clone()
    }

    fn capability(&self) -> Capability {
        Capability {
            read: true,
            stat: true,
            list: true,
            ..Default::default()
        }
    }

    async fn create_dir(
        &self,
        _: &OperationContext,
        _: &str,
        _: opendal::raw::OpCreateDir,
    ) -> opendal::Result<opendal::raw::RpCreateDir> {
        Err(unsupported())
    }

    async fn stat(&self, _: &OperationContext, path: &str, _: OpStat) -> opendal::Result<RpStat> {
        let s = self
            .source
            .stat(volume_path(path))
            .await
            .map_err(to_opendal)?;
        let metadata = if s.is_dir {
            MetadataBuilder::dir()
        } else {
            MetadataBuilder::file(s.len)
        };
        Ok(RpStat::new(metadata.build()))
    }

    fn read(&self, _: &OperationContext, path: &str, _: OpRead) -> opendal::Result<Self::Reader> {
        Ok(oio::StreamReader::new(VolumeReader {
            source: self.source.clone(),
            path: volume_path(path).to_owned(),
        }))
    }

    fn write(
        &self,
        _: &OperationContext,
        _: &str,
        _: opendal::raw::OpWrite,
    ) -> opendal::Result<Self::Writer> {
        Err(unsupported())
    }

    fn delete(&self, _: &OperationContext) -> opendal::Result<Self::Deleter> {
        Err(unsupported())
    }

    fn list(&self, _: &OperationContext, path: &str, _: OpList) -> opendal::Result<Self::Lister> {
        Ok(VolumeLister {
            source: self.source.clone(),
            dir: path.to_owned(),
            entries: None,
        })
    }

    fn copy(
        &self,
        _: &OperationContext,
        _: &str,
        _: &str,
        _: opendal::raw::OpCopy,
    ) -> opendal::Result<Self::Copier> {
        Err(unsupported())
    }

    async fn rename(
        &self,
        _: &OperationContext,
        _: &str,
        _: &str,
        _: opendal::raw::OpRename,
    ) -> opendal::Result<opendal::raw::RpRename> {
        Err(unsupported())
    }

    async fn presign(
        &self,
        _: &OperationContext,
        _: &str,
        _: opendal::raw::OpPresign,
    ) -> opendal::Result<opendal::raw::RpPresign> {
        Err(unsupported())
    }
}

fn unsupported() -> opendal::Error {
    opendal::Error::new(opendal::ErrorKind::Unsupported, "a volume is read-only")
}

struct VolumeReader {
    source: Arc<dyn VolumeSource>,
    path: String,
}

impl oio::StreamRead for VolumeReader {
    async fn open(
        &self,
        range: BytesRange,
    ) -> opendal::Result<(RpRead, Box<dyn oio::ReadStreamDyn>)> {
        let stat = self.source.stat(&self.path).await.map_err(to_opendal)?;
        if stat.is_dir {
            return Err(opendal::Error::new(
                opendal::ErrorKind::IsADirectory,
                "cannot read a directory",
            ));
        }
        let wanted = range.to_content_range(stat.len as usize)?;
        let wanted = wanted.start as u64..wanted.end as u64;
        let bytes = self
            .source
            .read(&self.path, wanted)
            .await
            .map_err(to_opendal)?;
        Ok((
            RpRead::new(MetadataBuilder::file(stat.len).build()),
            Box::new(Buffer::from(bytes)),
        ))
    }
}

struct VolumeLister {
    source: Arc<dyn VolumeSource>,
    /// The operator's directory path: `/` or `a/b/`.
    dir: String,
    entries: Option<std::vec::IntoIter<VolumeEntry>>,
}

impl oio::List for VolumeLister {
    async fn next(&mut self) -> opendal::Result<Option<oio::Entry>> {
        if self.entries.is_none() {
            let list = self
                .source
                .list(volume_path(&self.dir))
                .await
                .map_err(to_opendal)?;
            self.entries = Some(list.into_iter());
        }
        let Some(e) = self.entries.as_mut().and_then(Iterator::next) else {
            return Ok(None);
        };
        let prefix = if self.dir == "/" {
            String::new()
        } else {
            self.dir.clone()
        };
        Ok(Some(if e.is_dir {
            oio::Entry::new(
                &format!("{prefix}{}/", e.name),
                MetadataBuilder::dir().build(),
            )
        } else {
            oio::Entry::new(
                &format!("{prefix}{}", e.name),
                MetadataBuilder::file(e.len).build(),
            )
        }))
    }
}
