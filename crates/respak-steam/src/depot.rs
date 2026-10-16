//! The merged tree as a read-only OpenDAL service.

use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{Error, ErrorKind};

use crate::store::ChunkStore;
use crate::tree::DepotTree;

/// `stat` and `list` from the tree, `read` through the chunk store. Errors name the depot,
/// manifest and file and reach core as `Storage`.
pub struct DepotSource {
    tree: DepotTree,
    store: Arc<ChunkStore>,
}

impl DepotSource {
    /// A source over `tree`, reading through `store`.
    pub fn new(tree: DepotTree, store: Arc<ChunkStore>) -> Self {
        Self { tree, store }
    }

    /// The source as an [`Operator`] (`volume_operator`).
    pub fn into_operator(self) -> Operator {
        volume_operator(self, "steam")
    }
}

fn not_found(path: &str) -> Error {
    Error::new(ErrorKind::NotFound {
        path: path.to_string(),
    })
}

#[async_trait]
impl VolumeSource for DepotSource {
    async fn stat(&self, path: &str) -> respak_core::Result<VolumeStat> {
        if let Some(f) = self.tree.file(path) {
            return Ok(VolumeStat {
                is_dir: false,
                len: f.file.size,
            });
        }
        if self.tree.is_dir(path) {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        Err(not_found(path))
    }

    async fn read(&self, path: &str, range: Range<u64>) -> respak_core::Result<Bytes> {
        let f = self.tree.file(path).ok_or_else(|| not_found(path))?;
        self.store
            .read(f.depot, &f.file.chunks, range)
            .await
            .map_err(|e| {
                if matches!(e, crate::Error::Cancelled) {
                    return Error::new(ErrorKind::Cancelled);
                }
                let message = format!(
                    "steam depot {} manifest {} file {}: {e}",
                    f.depot, f.manifest, f.file.path
                );
                Error::new(ErrorKind::Storage(opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    message,
                )))
            })
    }

    async fn list(&self, dir: &str) -> respak_core::Result<Vec<VolumeEntry>> {
        let entries = self.tree.list(dir).ok_or_else(|| not_found(dir))?;
        Ok(entries
            .into_iter()
            .map(|e| VolumeEntry {
                name: e.name,
                is_dir: e.is_dir,
                len: e.size,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::cache::ChunkCache;
    use crate::store::tests::Fake;
    use crate::tree::DepotFile;

    /// One 1000-byte file `Paks/Win64/Common.rpak` in 100-byte chunks, one more file next to it.
    fn operator(fake: &Arc<Fake>, dir: &std::path::Path) -> Operator {
        let mut tree = DepotTree::default();
        let chunks: Arc<[_]> = fake.chunks.clone().into();
        tree.add(
            1,
            99,
            vec![
                DepotFile {
                    path: "Paks\\Win64\\Common.rpak".into(),
                    size: 1000,
                    chunks,
                },
                DepotFile {
                    path: "readme.txt".into(),
                    size: 0,
                    chunks: Arc::from(vec![]),
                },
            ],
        );
        let store = ChunkStore::new(ChunkCache::open(dir, 1 << 30).unwrap(), fake.clone(), 8);
        DepotSource::new(tree, Arc::new(store)).into_operator()
    }

    #[tokio::test]
    async fn stat_list_and_case_insensitive_ranged_read() {
        let fake = Fake::new(&[100; 10]);
        let dir = tempfile::tempdir().unwrap();
        let op = operator(&fake, dir.path());
        assert_eq!(
            op.stat("paks/win64/common.rpak")
                .await
                .unwrap()
                .content_length(),
            1000
        );
        assert!(op.stat("Paks/").await.unwrap().is_dir());
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            0,
            "stat and list never fetch"
        );
        let names: Vec<_> = op
            .list("Paks/Win64/")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name().to_string())
            .collect();
        assert!(
            names
                .iter()
                .any(|n| n.trim_end_matches('/') == "Common.rpak"),
            "{names:?}"
        );

        let got = op
            .read_with("PAKS/WIN64/COMMON.RPAK")
            .range(150..260)
            .await
            .unwrap()
            .to_vec();
        let all: Vec<u8> = fake.data.iter().flat_map(|d| d.to_vec()).collect();
        assert_eq!(got, &all[150..260]);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "only chunks 1 and 2");
        assert!(op.read("readme.txt").await.unwrap().to_vec().is_empty());
    }

    #[tokio::test]
    async fn a_missing_path_is_not_found_and_a_failed_chunk_names_depot_manifest_and_file() {
        let mut fake = Fake::new(&[100; 10]);
        Arc::get_mut(&mut fake).unwrap().fail = true;
        let dir = tempfile::tempdir().unwrap();
        let op = operator(&fake, dir.path());
        assert_eq!(
            op.stat("nope.bin").await.unwrap_err().kind(),
            opendal::ErrorKind::NotFound
        );
        let message = op
            .read_with("Paks/Win64/Common.rpak")
            .range(0..10)
            .await
            .unwrap_err()
            .to_string();
        for needle in [
            "depot 1",
            "manifest 99",
            "Paks/Win64/Common.rpak",
            "CDN said no",
        ] {
            assert!(message.contains(needle), "{needle} not in {message}");
        }
    }
}
