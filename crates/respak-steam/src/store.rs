//! Fetching chunks: cache hit, else the CDN, through one place that shares in-flight downloads
//! and counts what happened.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::future::{try_join_all, BoxFuture, Shared};
use futures::FutureExt;
use respak_core::{CancellationToken, Event, Events};
use tokio::sync::Semaphore;

use crate::cache::ChunkCache;
use crate::chunks::{assemble, covering, ChunkRef};
use crate::error::{Error, Result};

/// Where chunks come from on a cache miss. The real one downloads from the CDN, decrypts,
/// decompresses and checks size + Adler32 (`process_chunk`); tests use a fake.
#[async_trait]
pub trait ChunkSource: Send + Sync + 'static {
    /// The decompressed chunk of `depot`, and how many bytes were downloaded for it.
    async fn fetch(&self, depot: u32, chunk: &ChunkRef) -> Result<(Bytes, u64)>;
}

/// What a run did, printed as a summary at the end.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Chunks downloaded.
    pub chunks_downloaded: u64,
    /// Bytes downloaded (as stored on the CDN, i.e. compressed).
    pub bytes_downloaded: u64,
    /// Chunks served from the cache.
    pub cache_hits: u64,
}

type Key = (u32, [u8; 20]);
type Fetched = Shared<BoxFuture<'static, std::result::Result<Bytes, String>>>;

struct Inner {
    cache: ChunkCache,
    source: Arc<dyn ChunkSource>,
    cancel: CancellationToken,
    events: Events,
    permits: Semaphore,
    inflight: Mutex<HashMap<Key, Fetched>>,
    downloaded: AtomicU64,
    bytes: AtomicU64,
    hits: AtomicU64,
    requested: AtomicU64,
}

/// Cache + source. At most `parallel` fetches at a time; concurrent requests for the same
/// `(depot, sha)` share one download.
pub struct ChunkStore {
    inner: Arc<Inner>,
}

/// What `load` fails with when the token is triggered; `get_one` turns it into [`Error::Cancelled`].
const CANCELLED: &str = "cancelled";

async fn load(
    inner: Arc<Inner>,
    depot: u32,
    chunk: ChunkRef,
) -> std::result::Result<Bytes, String> {
    if inner.cancel.is_cancelled() {
        return Err(CANCELLED.into());
    }
    if let Some(data) = inner.cache.get(depot, &chunk).await {
        inner.hits.fetch_add(1, Ordering::Relaxed);
        return Ok(data);
    }
    let _permit = tokio::select! {
        permit = inner.permits.acquire() => permit.map_err(|e| e.to_string())?,
        () = inner.cancel.cancelled() => return Err(CANCELLED.into()),
    };
    let fetched = tokio::select! {
        fetched = inner.source.fetch(depot, &chunk) => fetched,
        () = inner.cancel.cancelled() => return Err(CANCELLED.into()),
    };
    let (data, downloaded) =
        fetched.map_err(|e| format!("depot {depot} chunk {}: {e}", chunk.hex()))?;
    let count = inner.downloaded.fetch_add(1, Ordering::Relaxed) + 1;
    let bytes = inner.bytes.fetch_add(downloaded, Ordering::Relaxed) + downloaded;
    inner.events.publish(Event::Custom {
        source: "steam".into(),
        name: "chunk".into(),
        data: serde_json::json!({ "depot": depot, "chunk": chunk.hex(), "downloaded": downloaded, "chunks": count, "bytes": bytes }),
    });
    if let Err(e) = inner.cache.put(depot, &chunk, &data).await {
        inner.events.publish(Event::Warning {
            message: format!("cannot cache chunk {}: {e}", chunk.hex()),
        });
    }
    Ok(data)
}

impl ChunkStore {
    /// A store over `cache` and `source`.
    pub fn new(cache: ChunkCache, source: Arc<dyn ChunkSource>, parallel: usize) -> Self {
        let inner = Inner {
            cache,
            source,
            cancel: CancellationToken::new(),
            events: Events::new(respak_core::event::DEFAULT_EVENT_CAPACITY),
            permits: Semaphore::new(parallel.max(1)),
            inflight: Mutex::default(),
            downloaded: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            requested: AtomicU64::new(0),
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Makes the store stop when `cancel` is triggered and publish what it does to `events`.
    /// Called before the store is shared.
    pub fn with_cancel_events(mut self, cancel: CancellationToken, events: Events) -> Self {
        let inner = Arc::get_mut(&mut self.inner).expect("the store is not shared yet");
        inner.cancel = cancel;
        inner.events = events;
        self
    }

    async fn get_one(&self, depot: u32, chunk: &ChunkRef) -> Result<Bytes> {
        let key = (depot, chunk.sha);
        let shared = {
            let mut inflight = self
                .inner
                .inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            inflight
                .entry(key)
                .or_insert_with(|| {
                    load(self.inner.clone(), depot, chunk.clone())
                        .boxed()
                        .shared()
                })
                .clone()
        };
        let result = shared.await;
        self.inner
            .inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key);
        result.map_err(|e| {
            if e == CANCELLED {
                Error::Cancelled
            } else {
                Error::Fetch(e)
            }
        })
    }

    /// The decompressed chunks `chunks`, in order.
    pub async fn get_many(&self, depot: u32, chunks: &[ChunkRef]) -> Result<Vec<Bytes>> {
        try_join_all(chunks.iter().map(|c| self.get_one(depot, c))).await
    }

    /// Reads `range` of a file made of `chunks`: `covering`, `get_many`, `assemble`.
    pub async fn read(&self, depot: u32, chunks: &[ChunkRef], range: Range<u64>) -> Result<Bytes> {
        let need = covering(chunks, range.clone());
        if !need.is_empty() {
            let count = need.len() as u64;
            let total = self.inner.requested.fetch_add(count, Ordering::Relaxed) + count;
            self.inner.events.publish(Event::Custom {
                source: "steam".into(),
                name: "request".into(),
                data: serde_json::json!({ "depot": depot, "chunks": count, "total": total }),
            });
        }
        let data = self.get_many(depot, &chunks[need.clone()]).await?;
        Ok(assemble(chunks, need.start, &data, range))
    }

    /// Counters so far.
    pub fn stats(&self) -> Stats {
        Stats {
            chunks_downloaded: self.inner.downloaded.load(Ordering::Relaxed),
            bytes_downloaded: self.inner.bytes.load(Ordering::Relaxed),
            cache_hits: self.inner.hits.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use steamroom::util::checksum::SteamAdler32;

    use super::*;
    use crate::chunks::tests::file;

    /// Serves the chunks of a synthetic file; counts calls and the highest concurrency.
    pub(crate) struct Fake {
        pub(crate) chunks: Vec<ChunkRef>,
        pub(crate) data: Vec<Bytes>,
        pub(crate) calls: AtomicUsize,
        active: AtomicUsize,
        pub(crate) peak: AtomicUsize,
        pub(crate) fail: bool,
    }

    impl Fake {
        pub(crate) fn new(sizes: &[u32]) -> Arc<Self> {
            let (mut chunks, data, _) = file(sizes);
            for (c, d) in chunks.iter_mut().zip(&data) {
                c.adler = SteamAdler32::compute(d).0;
            }
            Arc::new(Self {
                chunks,
                data,
                calls: 0.into(),
                active: 0.into(),
                peak: 0.into(),
                fail: false,
            })
        }
    }

    #[async_trait]
    impl ChunkSource for Fake {
        async fn fetch(&self, _depot: u32, chunk: &ChunkRef) -> Result<(Bytes, u64)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            if self.fail {
                return Err(Error::Fetch("CDN said no".into()));
            }
            let i = self.chunks.iter().position(|c| c.sha == chunk.sha).unwrap();
            Ok((self.data[i].clone(), u64::from(chunk.len) / 2))
        }
    }

    fn store(fake: &Arc<Fake>, dir: &std::path::Path, parallel: usize) -> ChunkStore {
        ChunkStore::new(
            ChunkCache::open(dir, 1 << 30).unwrap(),
            fake.clone(),
            parallel,
        )
    }

    #[tokio::test]
    async fn concurrent_reads_of_one_chunk_fetch_it_once() {
        let fake = Fake::new(&[100]);
        let dir = tempfile::tempdir().unwrap();
        let store = store(&fake, dir.path(), 8);
        let reads = (0..10).map(|_| store.read(1, &fake.chunks, 0..100));
        for r in futures::future::join_all(reads).await {
            assert_eq!(r.unwrap(), fake.data[0]);
        }
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.stats(),
            Stats {
                chunks_downloaded: 1,
                bytes_downloaded: 50,
                cache_hits: 0
            }
        );
    }

    #[tokio::test]
    async fn at_most_eight_fetches_run_at_once() {
        let fake = Fake::new(&[10; 32]);
        let dir = tempfile::tempdir().unwrap();
        let store = store(&fake, dir.path(), 8);
        let data = store.get_many(1, &fake.chunks).await.unwrap();
        assert_eq!(data, fake.data);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 32);
        let peak = fake.peak.load(Ordering::SeqCst);
        assert!(peak > 1 && peak <= 8, "peak {peak}");
    }

    #[tokio::test]
    async fn a_range_fetches_only_the_covering_chunks_and_a_second_read_hits_the_cache() {
        let fake = Fake::new(&[100; 10]);
        let dir = tempfile::tempdir().unwrap();
        let store = store(&fake, dir.path(), 8);
        let got = store.read(1, &fake.chunks, 250..420).await.unwrap();
        assert_eq!(got.len(), 170);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 3, "chunks 2, 3 and 4");
        store.read(1, &fake.chunks, 250..420).await.unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
        assert_eq!(store.stats().cache_hits, 3);
    }

    #[tokio::test]
    async fn everything_cached_makes_no_source_call_even_in_a_new_process() {
        let fake = Fake::new(&[100; 4]);
        let dir = tempfile::tempdir().unwrap();
        store(&fake, dir.path(), 8)
            .get_many(1, &fake.chunks)
            .await
            .unwrap();
        let again = Fake::new(&[100; 4]);
        let second = store(&again, dir.path(), 8);
        assert_eq!(second.get_many(1, &again.chunks).await.unwrap(), again.data);
        assert_eq!(again.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            second.stats(),
            Stats {
                chunks_downloaded: 0,
                bytes_downloaded: 0,
                cache_hits: 4
            }
        );
    }

    #[tokio::test]
    async fn a_failed_fetch_names_the_depot_and_chunk_and_is_not_cached() {
        let mut fake = Fake::new(&[100]);
        Arc::get_mut(&mut fake).unwrap().fail = true;
        let dir = tempfile::tempdir().unwrap();
        let store = store(&fake, dir.path(), 8);
        let message = store
            .read(7, &fake.chunks, 0..10)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("depot 7")
                && message.contains(&fake.chunks[0].hex())
                && message.contains("CDN said no"),
            "{message}"
        );
        assert!(store.read(7, &fake.chunks, 0..10).await.is_err());
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            2,
            "a failure is not remembered"
        );
    }

    #[tokio::test]
    async fn a_triggered_token_stops_the_store_before_any_fetch() {
        let fake = Fake::new(&[100; 4]);
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let store = store(&fake, dir.path(), 8).with_cancel_events(cancel, Events::new(16));
        assert!(matches!(
            store.get_many(1, &fake.chunks).await,
            Err(Error::Cancelled)
        ));
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn cancelling_during_a_download_returns_cancelled_and_caches_nothing() {
        let fake = Fake::new(&[100; 16]);
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        let store = store(&fake, dir.path(), 2).with_cancel_events(cancel.clone(), Events::new(16));
        let trigger = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                cancel.cancel();
            }
        });
        assert!(matches!(
            store.get_many(1, &fake.chunks).await,
            Err(Error::Cancelled)
        ));
        trigger.await.unwrap();
        assert!(
            fake.calls.load(Ordering::SeqCst) <= 2,
            "only the running fetches started"
        );
        assert_eq!(store.stats().chunks_downloaded, 0);
    }

    #[tokio::test]
    async fn every_download_publishes_a_chunk_event() {
        let fake = Fake::new(&[100; 3]);
        let dir = tempfile::tempdir().unwrap();
        let events = Events::new(16);
        let mut rx = events.subscribe();
        let store =
            store(&fake, dir.path(), 1).with_cancel_events(CancellationToken::new(), events);
        store.get_many(1, &fake.chunks).await.unwrap();
        for n in 1..=3u64 {
            match rx.try_recv().unwrap() {
                Event::Custom { source, name, data } => {
                    assert_eq!((source.as_str(), name.as_str()), ("steam", "chunk"));
                    assert_eq!(data["chunks"], n);
                    assert_eq!(data["bytes"], n * 50);
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(rx.try_recv().is_err());
    }
}
