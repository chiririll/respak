//! The [`Container`] capability.

use async_trait::async_trait;
use opendal::Operator;

use crate::error::Result;
use crate::value::{FileData, Value};

/// A value that exposes its contents as a read-only OpenDAL [`Operator`], its **volume**, and
/// resolves paths inside it to typed entries.
///
/// # Typed entries
/// The default [`entry`](Container::entry) returns a plain `file { volume, path }`. A container
/// overrides it when an entry needs data that is not in the entry's own bytes. Example: the
/// rpak volume returns `rpak.asset { kind, version, header_size, guid, header: file,
/// mem: file, stream: Option<file>, opt: Option<file> }`, because an asset's version and header
/// size live in the pak's asset entry, not in the asset bytes. Core knows nothing about such
/// types; it only calls `entry` and scores the returned value like any other.
#[async_trait]
pub trait Container: Send + Sync {
    /// The container's contents. Cloning an `Operator` is cheap.
    fn volume(&self) -> Operator;

    /// The container's own virtual path (e.g. `paks/Win64/common.rpak`). Entry keys are built
    /// from it as `<mount_path>/<path>`.
    fn mount_path(&self) -> &str;

    /// Resolves `path` (relative to the volume) to a value. The default checks that the path
    /// exists (`NotFound` otherwise).
    async fn entry(&self, path: &str) -> Result<Value> {
        self.volume().stat(path).await?;
        let virtual_path = format!("{}/{}", self.mount_path().trim_end_matches('/'), path.trim_start_matches('/'));
        Ok(std::sync::Arc::new(FileData::new(self.volume(), path, virtual_path, None)))
    }
}
