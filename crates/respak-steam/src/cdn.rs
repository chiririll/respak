//! The real [`ChunkSource`]: a chunk from a CDN server, decrypted, decompressed and checked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use steamroom::cdn::{CdnClient, CdnServerPool};
use steamroom::depot::{chunk::process_chunk, AppId, ChunkId, DepotId, DepotKey};
use steamroom::util::checksum::Sha1Hash;
use tokio::sync::OnceCell;

use crate::chunks::ChunkRef;
use crate::error::{Error, Result};
use crate::link::Resolver;
use crate::store::ChunkSource;

/// A CDN auth token is renewed a little before Steam says it expires.
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

/// What the CDN needs per `(depot, host)`.
#[derive(Clone)]
struct Token {
    value: Option<String>,
    valid_until: Option<Instant>,
}

/// Downloads chunks. The CDN server list is fetched on the first miss and tokens per host as they are needed.
pub struct CdnSource {
    resolver: Arc<Resolver>,
    keys: HashMap<u32, DepotKey>,
    cdn: CdnClient,
    pool: OnceCell<CdnServerPool>,
    tokens: Mutex<HashMap<(u32, String), Token>>,
}

/// `?token` as the URL needs it (steamroom appends the token as it is).
fn as_query(token: String) -> String {
    if token.starts_with('?') {
        token
    } else {
        format!("?{token}")
    }
}

impl CdnSource {
    /// A source for the depots whose keys are in `keys`.
    pub fn new(resolver: Arc<Resolver>, keys: HashMap<u32, DepotKey>) -> Result<Self> {
        Ok(Self { resolver, keys, cdn: CdnClient::new()?, pool: OnceCell::new(), tokens: Mutex::default() })
    }

    async fn pool(&self) -> Result<&CdnServerPool> {
        self.pool.get_or_try_init(|| async { self.resolver.cdn_pool().await }).await
    }

    async fn token(&self, depot: u32, host: &str) -> Option<String> {
        let key = (depot, host.to_string());
        let cached = self.tokens.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
        if let Some(t) = cached.filter(|t| t.valid_until.is_none_or(|v| v > Instant::now())) {
            return t.value;
        }
        let fetched = self.resolver.cdn_token(depot, host).await;
        let token = match fetched {
            Ok(t) => Token {
                value: t.token.filter(|v| !v.is_empty()).map(as_query),
                valid_until: t.expiration_time.and_then(|exp| {
                    let left = u64::from(exp).checked_sub(crate::account::now_secs())?;
                    Some(Instant::now() + Duration::from_secs(left).saturating_sub(TOKEN_MARGIN))
                }),
            },
            Err(e) => {
                tracing::debug!("no CDN auth token for {host}: {e}");
                Token { value: None, valid_until: Some(Instant::now() + TOKEN_MARGIN) }
            }
        };
        self.tokens.lock().unwrap_or_else(|e| e.into_inner()).insert(key, token.clone());
        token.value
    }
}

#[async_trait]
impl ChunkSource for CdnSource {
    async fn fetch(&self, depot: u32, chunk: &ChunkRef) -> Result<(Bytes, u64)> {
        let key = self.keys.get(&depot).cloned().ok_or_else(|| Error::Fetch(format!("no key for depot {depot}")))?;
        let pool = self.pool().await?;
        let mut last = None;
        for _ in 0..pool.len().min(5) {
            let (server, wait) = pool.pick();
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            let token = self.token(depot, &server.host).await;
            let raw = match self.cdn.download_chunk(server, DepotId(depot), &ChunkId(chunk.sha), token.as_deref()).await {
                Ok(raw) => raw,
                Err(e) => {
                    let retry = match &e {
                        steamroom::Error::CdnStatus { retry_after, .. } => retry_after.map(Duration::from_secs),
                        _ => None,
                    };
                    tracing::warn!("CDN {} failed for chunk {}: {e}", server.host, chunk.hex());
                    pool.report_failure(server, retry);
                    last = Some(e.to_string());
                    continue;
                }
            };
            let downloaded = raw.len() as u64;
            let (expected_len, expected_adler, sha) = (chunk.len, chunk.adler, chunk.sha);
            let key = key.clone();
            let decoded = tokio::task::spawn_blocking(move || {
                let data = process_chunk(&raw, &key, expected_len, expected_adler).map_err(|e| e.to_string())?;
                if sha != [0u8; 20] && Sha1Hash::compute(&data).0 != sha {
                    return Err("SHA-1 does not match the manifest".to_string());
                }
                Ok(Bytes::from(data))
            })
            .await
            .map_err(|e| Error::Fetch(e.to_string()))?;
            match decoded {
                Ok(data) => {
                    pool.report_success(server);
                    return Ok((data, downloaded));
                }
                Err(e) => {
                    tracing::warn!("chunk {} from {} is bad: {e}", chunk.hex(), server.host);
                    pool.report_failure(server, None);
                    last = Some(e);
                }
            }
        }
        Err(Error::Fetch(last.unwrap_or_else(|| "no CDN servers".into())))
    }
}

/// The app whose CDN token and keys a depot needs, as steamroom wants it.
pub(crate) fn app_id(app: u32) -> AppId {
    AppId(app)
}
