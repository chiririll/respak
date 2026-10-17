//! The bank as a read-only volume of `.asrc` descriptors.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::volume::{VolumeEntry, VolumeSource, VolumeStat};
use respak_core::Result;

use super::bank::Bank;
use super::not_found;

pub(super) struct MbnkSource(pub(super) Arc<Bank>);

#[async_trait]
impl VolumeSource for MbnkSource {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() || self.0.is_dir(path) {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        let s = self.0.find(path).ok_or_else(|| not_found(path))?;
        Ok(VolumeStat {
            is_dir: false,
            len: s.descriptor().len() as u64,
        })
    }

    /// The bytes of an `.asrc` entry are its descriptor, never audio data.
    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        let s = self.0.find(path).ok_or_else(|| not_found(path))?;
        let all = Bytes::from(s.descriptor());
        let end = (range.end as usize).min(all.len());
        Ok(all.slice((range.start as usize).min(end)..end))
    }

    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        Ok(self.0.list(dir))
    }
}
