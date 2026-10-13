//! The chunk cache on disk: `<dir>/<depot>/<sha1 hex>`, decrypted and decompressed. LRU by mtime
//! under a size cap; the size is scanned once at open and tracked in memory after that.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use steamroom::util::checksum::SteamAdler32;

use crate::chunks::ChunkRef;
use crate::error::{Error, Result};

/// Default cap: 8 GiB.
pub const DEFAULT_CAP: u64 = 8 << 30;

/// Eviction goes down to this share of the cap, so a cache sitting at its cap does not rescan on every write.
const EVICT_TO_PERCENT: u64 = 90;

#[derive(Debug)]
struct Inner {
    dir: PathBuf,
    cap: u64,
    size: AtomicU64,
    /// Serializes eviction passes.
    evicting: std::sync::Mutex<()>,
}

/// The chunk cache.
#[derive(Debug, Clone)]
pub struct ChunkCache {
    inner: Arc<Inner>,
}

fn io_error(path: &Path, e: impl ToString) -> Error {
    Error::Account { path: path.display().to_string(), message: e.to_string() }
}

/// Every file under `dir/<depot>/` with its size and mtime.
fn scan(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut out = Vec::new();
    let Ok(depots) = fs::read_dir(dir) else { return out };
    for depot in depots.flatten() {
        let Ok(files) = fs::read_dir(depot.path()) else { continue };
        for file in files.flatten() {
            if let Ok(meta) = file.metadata() {
                if meta.is_file() {
                    out.push((file.path(), meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
                }
            }
        }
    }
    out
}

impl ChunkCache {
    /// Opens (creating) the cache in `dir` and scans it once for its size.
    pub fn open(dir: impl AsRef<Path>, cap: u64) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        let size = scan(&dir).iter().map(|(_, len, _)| len).sum();
        Ok(Self { inner: Arc::new(Inner { dir, cap, size: AtomicU64::new(size), evicting: Default::default() }) })
    }

    /// Where a chunk lives.
    pub fn path_of(&self, depot: u32, sha: &[u8; 20]) -> PathBuf {
        let hex: String = sha.iter().map(|b| format!("{b:02x}")).collect();
        self.inner.dir.join(depot.to_string()).join(hex)
    }

    /// Bytes currently cached (the in-memory count).
    pub fn size(&self) -> u64 {
        self.inner.size.load(Ordering::Relaxed)
    }

    /// The cached chunk, verified (size + Adler32). A bad file is deleted and reported as a miss.
    /// A hit refreshes the mtime.
    pub async fn get(&self, depot: u32, chunk: &ChunkRef) -> Option<Bytes> {
        let path = self.path_of(depot, &chunk.sha);
        let (inner, chunk) = (self.inner.clone(), chunk.clone());
        tokio::task::spawn_blocking(move || {
            let data = fs::read(&path).ok()?;
            if data.len() != chunk.len as usize || SteamAdler32::compute(&data).0 != chunk.adler {
                tracing::warn!("cached chunk {} is corrupt; deleting it", path.display());
                if fs::remove_file(&path).is_ok() {
                    inner.size.fetch_sub(data.len() as u64, Ordering::Relaxed);
                }
                return None;
            }
            if let Ok(file) = fs::File::options().write(true).open(&path) {
                let _ = file.set_modified(SystemTime::now());
            }
            Some(Bytes::from(data))
        })
        .await
        .ok()
        .flatten()
    }

    /// Stores a chunk, then evicts least-recently-used files until the size is under the cap.
    pub async fn put(&self, depot: u32, chunk: &ChunkRef, data: &Bytes) -> Result<()> {
        let path = self.path_of(depot, &chunk.sha);
        let (inner, data) = (self.inner.clone(), data.clone());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let parent = path.parent().expect("chunk paths have a parent");
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            let old = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, &data).map_err(|e| io_error(&tmp, e))?;
            fs::rename(&tmp, &path).map_err(|e| io_error(&path, e))?;
            let size = inner.size.fetch_add(data.len() as u64, Ordering::Relaxed) + data.len() as u64 - old;
            if old > 0 {
                inner.size.fetch_sub(old, Ordering::Relaxed);
            }
            if size > inner.cap {
                evict(&inner);
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::Protocol(e.to_string()))?
    }
}

/// Deletes the least recently used files until the cache is at 90% of its cap.
fn evict(inner: &Inner) {
    let _guard = inner.evicting.lock().unwrap_or_else(|e| e.into_inner());
    if inner.size.load(Ordering::Relaxed) <= inner.cap {
        return;
    }
    let target = inner.cap / 100 * EVICT_TO_PERCENT;
    let mut files = scan(&inner.dir);
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if inner.size.load(Ordering::Relaxed) <= target {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            inner.size.fetch_sub(len, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn chunk(n: u8, body: &[u8]) -> (ChunkRef, Bytes) {
        let mut sha = [0u8; 20];
        sha[0] = n;
        (ChunkRef { sha, offset: 0, len: body.len() as u32, adler: SteamAdler32::compute(body).0 }, Bytes::copy_from_slice(body))
    }

    fn age(cache: &ChunkCache, depot: u32, c: &ChunkRef, secs: u64) {
        let file = fs::File::options().write(true).open(cache.path_of(depot, &c.sha)).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(secs)).unwrap();
    }

    #[tokio::test]
    async fn put_then_get_hits() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ChunkCache::open(dir.path(), 1 << 20).unwrap();
        let (c, data) = chunk(1, b"hello chunk");
        assert!(cache.get(7, &c).await.is_none());
        cache.put(7, &c, &data).await.unwrap();
        assert_eq!(cache.get(7, &c).await.unwrap(), data);
        assert!(cache.get(8, &c).await.is_none(), "chunks are not shared across depots");
        assert_eq!(cache.size(), data.len() as u64);
        cache.put(7, &c, &data).await.unwrap();
        assert_eq!(cache.size(), data.len() as u64, "storing the same chunk twice counts once");
    }

    #[tokio::test]
    async fn corrupt_chunk_is_deleted_and_missed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ChunkCache::open(dir.path(), 1 << 20).unwrap();
        let (c, data) = chunk(1, b"hello chunk");
        cache.put(7, &c, &data).await.unwrap();
        fs::write(cache.path_of(7, &c.sha), b"hellO chunk").unwrap();
        assert!(cache.get(7, &c).await.is_none(), "bad Adler32");
        assert!(!cache.path_of(7, &c.sha).exists());
        assert_eq!(cache.size(), 0);
        cache.put(7, &c, &data).await.unwrap();
        fs::write(cache.path_of(7, &c.sha), b"short").unwrap();
        assert!(cache.get(7, &c).await.is_none(), "bad size");
        assert!(!cache.path_of(7, &c.sha).exists());
    }

    #[tokio::test]
    async fn eviction_keeps_the_size_under_the_cap_and_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ChunkCache::open(dir.path(), 300).unwrap();
        let chunks: Vec<_> = (0..3).map(|i| chunk(i, &[i; 100])).collect();
        for (i, (c, d)) in chunks.iter().enumerate() {
            cache.put(1, c, d).await.unwrap();
            age(&cache, 1, c, 100 - i as u64 * 10);
        }
        assert_eq!(cache.size(), 300);
        let (c4, d4) = chunk(9, &[9; 100]);
        cache.put(1, &c4, &d4).await.unwrap();
        assert!(cache.size() <= 300);
        assert!(cache.get(1, &chunks[0].0).await.is_none(), "the oldest went first");
        assert!(cache.get(1, &c4).await.is_some(), "the new chunk stays");
        assert!(cache.get(1, &chunks[2].0).await.is_some());
    }

    #[tokio::test]
    async fn a_hit_refreshes_the_order() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ChunkCache::open(dir.path(), 300).unwrap();
        let [(a, da), (b, db), (c, dc), (d, dd)] = [1u8, 2, 3, 4].map(|i| chunk(i, &[i; 100]));
        for (x, data) in [(&a, &da), (&b, &db), (&c, &dc)] {
            cache.put(1, x, data).await.unwrap();
        }
        age(&cache, 1, &a, 100);
        age(&cache, 1, &b, 50);
        age(&cache, 1, &c, 10);
        assert!(cache.get(1, &a).await.is_some(), "the hit makes a the newest");
        cache.put(1, &d, &dd).await.unwrap();
        assert!(cache.get(1, &b).await.is_none() && cache.get(1, &c).await.is_none(), "b and c were the oldest");
        assert!(cache.get(1, &a).await.is_some() && cache.get(1, &d).await.is_some());
    }

    #[tokio::test]
    async fn the_startup_scan_matches_the_tracked_size() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ChunkCache::open(dir.path(), 1 << 20).unwrap();
        for i in 0..4u8 {
            let (c, d) = chunk(i, &vec![i; 10 + i as usize]);
            cache.put(u32::from(i % 2), &c, &d).await.unwrap();
        }
        let size = cache.size();
        assert_eq!(size, 10 + 11 + 12 + 13);
        assert_eq!(ChunkCache::open(dir.path(), 1 << 20).unwrap().size(), size);
    }
}
