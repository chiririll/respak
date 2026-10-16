//! The live part: PICS, depot keys, manifest request codes, CDN servers, and opening the depots.
//! The Steam connection is opened lazily, only when something is not already in `.respak/steam/`.
//! Tested live only (`RESPAK_STEAM_LIVE`).

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use opendal::Operator;
use respak_core::{CancellationToken, Event, Events};
use steamroom::apps::AppDetails;
use steamroom::cdn::{CdnClient, CdnServerPool};
use steamroom::client::{LoggedIn, SteamClient};
use steamroom::content::CdnAuthToken;
use steamroom::depot::manifest::DepotManifest;
use steamroom::depot::{AppId, CellId, DepotId, DepotKey, ManifestId};
use steamroom::enums::DepotFileFlags;
use tokio::sync::OnceCell;

use crate::account::AccountStore;
use crate::cache::{ChunkCache, DEFAULT_CAP};
use crate::cdn::CdnSource;
use crate::chunks::ChunkRef;
use crate::depot::DepotSource;
use crate::error::{Error, Result};
use crate::store::{ChunkStore, Stats};
use crate::tree::{DepotFile, DepotTree};

/// Parallel chunk fetches (STEAM.md §6).
const PARALLEL_FETCHES: usize = 8;

/// One `--depot ID[=MANIFEST]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepotSpec {
    /// Depot id.
    pub depot: u32,
    /// Pinned manifest; `None` takes the branch's current one from PICS.
    pub manifest: Option<u64>,
}

impl std::str::FromStr for DepotSpec {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        fn number<T: std::str::FromStr>(
            whole: &str,
            text: &str,
            what: &str,
        ) -> std::result::Result<T, String> {
            text.trim()
                .parse()
                .map_err(|_| format!("`{whole}`: {what} must be a number"))
        }
        match s.split_once('=') {
            None => Ok(Self {
                depot: number(s, s, "the depot id")?,
                manifest: None,
            }),
            Some((depot, manifest)) => Ok(Self {
                depot: number(s, depot, "the depot id")?,
                manifest: Some(number(s, manifest, "the manifest id")?),
            }),
        }
    }
}

/// Everything `--steam` needs.
#[derive(Debug, Clone)]
pub struct SteamOptions {
    /// The app id (`--steam`).
    pub app: u32,
    /// Branch (`--branch`, default `public`).
    pub branch: String,
    /// Depots in `--depot` order; later wins collisions.
    pub depots: Vec<DepotSpec>,
    /// The `.respak` directory.
    pub state_dir: PathBuf,
    /// Chunk cache cap in bytes.
    pub cache_cap: u64,
}

impl SteamOptions {
    /// Options with the default branch and cache cap.
    pub fn new(app: u32, depots: Vec<DepotSpec>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            app,
            branch: "public".into(),
            depots,
            state_dir: state_dir.into(),
            cache_cap: DEFAULT_CAP,
        }
    }
}

/// The opened depots: the input `Operator` and the counters for the end-of-run summary.
pub struct SteamInput {
    /// The merged tree, read-only.
    pub operator: Operator,
    store: Arc<ChunkStore>,
}

impl SteamInput {
    /// Counters so far.
    pub fn stats(&self) -> Stats {
        self.store.stats()
    }
}

/// Talks to Steam on demand: the connection and the PICS answers are made at most once.
pub struct Resolver {
    accounts: AccountStore,
    app: u32,
    branch: String,
    client: OnceCell<SteamClient<LoggedIn>>,
    details: Mutex<HashMap<u32, Arc<OnceCell<Arc<AppDetails>>>>>,
}

impl Resolver {
    async fn client(&self) -> Result<&SteamClient<LoggedIn>> {
        self.client
            .get_or_try_init(|| async { Ok(crate::session::logon(&self.accounts).await?.0) })
            .await
    }

    async fn details(&self, app: u32) -> Result<Arc<AppDetails>> {
        let cell = self
            .details
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(app)
            .or_default()
            .clone();
        let details = cell
            .get_or_try_init(|| async {
                tracing::info!("asking Steam (PICS) about app {app}");
                Ok::<_, Error>(Arc::new(
                    self.client().await?.app_details(AppId(app)).await?,
                ))
            })
            .await?;
        Ok(details.clone())
    }

    /// The app that owns `depot`'s key and manifests: `depotfromapp` for a shared depot, else the app itself.
    pub async fn key_app(&self, depot: u32) -> Result<u32> {
        let own = self.details(self.app).await?;
        Ok(own
            .depot(DepotId(depot))
            .and_then(|d| d.depot_from_app)
            .map_or(self.app, |a| a.0))
    }

    /// The branch's current manifest of `depot` (falling back to `public`).
    async fn current_manifest(&self, depot: u32) -> Result<u64> {
        for app in [self.app, self.key_app(depot).await?] {
            let details = self.details(app).await?;
            if let Some(d) = details.depot(DepotId(depot)) {
                if let Some(m) = d.manifest(&self.branch).or_else(|| d.manifest("public")) {
                    return Ok(m.manifest_id.0);
                }
            }
        }
        Err(Error::Fetch(format!("depot {depot} has no manifest on branch `{}` (or `public`); pin one with --depot {depot}=MANIFEST", self.branch)))
    }

    async fn depot_key(&self, depot: u32) -> Result<DepotKey> {
        let app = self.key_app(depot).await?;
        self.client()
            .await?
            .get_depot_decryption_key(DepotId(depot), AppId(app))
            .await
            .map_err(|e| {
                Error::Fetch(format!(
                    "no key for depot {depot} (does the account own the game?): {e}"
                ))
            })
    }

    /// The CDN server list as a pool.
    pub(crate) async fn cdn_pool(&self) -> Result<CdnServerPool> {
        let servers = self
            .client()
            .await?
            .get_cdn_servers(CellId(0), Some(20))
            .await?;
        if servers.is_empty() {
            return Err(Error::Fetch("Steam returned no CDN servers".into()));
        }
        Ok(CdnServerPool::new(servers))
    }

    /// A CDN auth token for `host`.
    pub(crate) async fn cdn_token(&self, depot: u32, host: &str) -> Result<CdnAuthToken> {
        let app = self.key_app(depot).await?;
        Ok(self
            .client()
            .await?
            .get_cdn_auth_token(crate::cdn::app_id(app), DepotId(depot), host)
            .await?)
    }

    /// The manifest file as Steam serves it (a zip around a protobuf).
    async fn download_manifest(&self, depot: u32, manifest: u64) -> Result<Vec<u8>> {
        let app = self.key_app(depot).await?;
        let branch = (self.branch != "public").then_some(self.branch.as_str());
        let refused = || {
            Error::Fetch(format!(
                "Steam refused a request code for depot {depot} manifest {manifest} (code 0); the account may not own it, or the manifest is too old"
            ))
        };
        let code = self
            .client()
            .await?
            .get_manifest_request_code(
                AppId(app),
                DepotId(depot),
                ManifestId(manifest),
                branch,
                None,
            )
            .await
            .map_err(|e| {
                Error::Fetch(format!(
                    "request code for depot {depot} manifest {manifest}: {e}"
                ))
            })?
            .filter(|c| *c != 0)
            .ok_or_else(refused)?;
        let pool = self.cdn_pool().await?;
        let token = match self.cdn_token(depot, &pool.pick().0.host).await {
            Ok(t) => t.token.filter(|t| !t.is_empty()).map(|t| {
                if t.starts_with('?') {
                    t
                } else {
                    format!("?{t}")
                }
            }),
            Err(_) => None,
        };
        let bytes = CdnClient::new()?
            .download_manifest_pooled(
                &pool,
                DepotId(depot),
                ManifestId(manifest),
                code,
                token.as_deref(),
            )
            .await
            .map_err(|e| Error::Fetch(format!("manifest {manifest} of depot {depot}: {e}")))?;
        Ok(bytes.to_vec())
    }
}

/// `future`, unless `cancel` is triggered first.
async fn cancellable<T>(
    cancel: &CancellationToken,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        result = future => result,
        () = cancel.cancelled() => Err(Error::Cancelled),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |e: std::io::Error| Error::Account {
        path: path.display().to_string(),
        message: e.to_string(),
    };
    std::fs::create_dir_all(path.parent().expect("state files have a parent")).map_err(io)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn read_cached(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok().filter(|b| !b.is_empty())
}

/// Unwraps the zip around a downloaded manifest (if it is one), parses it and decrypts the filenames.
fn parse_manifest(bytes: &[u8], key: &DepotKey) -> Result<DepotManifest> {
    let fail = |e: &dyn std::fmt::Display| Error::Fetch(format!("manifest: {e}"));
    let payload = if bytes.starts_with(b"PK") {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| fail(&e))?;
        let mut file = archive.by_index(0).map_err(|e| fail(&e))?;
        let mut out = Vec::new();
        file.read_to_end(&mut out).map_err(|e| fail(&e))?;
        out
    } else {
        bytes.to_vec()
    };
    let mut manifest = DepotManifest::parse(&payload).map_err(|e| fail(&e))?;
    manifest.decrypt_filenames(key).map_err(|e| fail(&e))?;
    Ok(manifest)
}

/// A manifest's files as tree entries (directories and symlinks left out).
fn files_of(manifest: &DepotManifest) -> Vec<DepotFile> {
    manifest
        .files
        .iter()
        .filter(|f| {
            let flags = DepotFileFlags::from_bits_truncate(f.flags);
            !flags.is_directory()
                && !flags.contains(DepotFileFlags::SYMLINK)
                && f.link_target.is_none()
        })
        .map(|f| {
            let mut offset = 0u64;
            let chunks: Vec<ChunkRef> = f
                .chunks
                .iter()
                .map(|c| {
                    let at = c.offset.unwrap_or(offset);
                    offset = at + u64::from(c.uncompressed_size);
                    ChunkRef {
                        sha: c.id.0,
                        offset: at,
                        len: c.uncompressed_size,
                        adler: c.checksum,
                    }
                })
                .collect();
            DepotFile {
                path: f.normalized_path(),
                size: f.size,
                chunks: chunks.into(),
            }
        })
        .collect()
}

/// Opens the depots: keys and manifests from `.respak/steam/`, else from Steam (logging on with the
/// stored token; failing with `NeedLogin` rather than prompting).
pub async fn open(
    options: SteamOptions,
    cancel: &CancellationToken,
    events: &Events,
) -> Result<SteamInput> {
    let steam_dir = options.state_dir.join("steam");
    let resolver = Arc::new(Resolver {
        accounts: AccountStore::new(&options.state_dir),
        app: options.app,
        branch: options.branch.clone(),
        client: OnceCell::new(),
        details: Mutex::default(),
    });
    let mut tree = DepotTree::default();
    let mut keys = HashMap::new();
    for spec in &options.depots {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let depot = spec.depot;
        let key_path = steam_dir.join("keys").join(format!("{depot}.key"));
        let key = match read_cached(&key_path).and_then(|b| <[u8; 32]>::try_from(b).ok()) {
            Some(raw) => DepotKey(raw),
            None => {
                let key = cancellable(cancel, resolver.depot_key(depot)).await?;
                write_atomic(&key_path, &key.0)?;
                key
            }
        };
        let manifest_id = match spec.manifest {
            Some(id) => id,
            None => cancellable(cancel, resolver.current_manifest(depot)).await?,
        };
        let manifest_path = steam_dir
            .join("manifests")
            .join(format!("{depot}_{manifest_id}.manifest"));
        let cached = read_cached(&manifest_path).and_then(|b| parse_manifest(&b, &key).ok());
        let manifest = match cached {
            Some(m) => m,
            None => {
                let bytes =
                    cancellable(cancel, resolver.download_manifest(depot, manifest_id)).await?;
                let manifest = parse_manifest(&bytes, &key)?;
                write_atomic(&manifest_path, &bytes)?;
                manifest
            }
        };
        tracing::info!(
            "depot {depot} manifest {manifest_id}: {} files",
            manifest.files.len()
        );
        events.publish(Event::Custom {
            source: "steam".into(),
            name: "depot".into(),
            data: serde_json::json!({ "depot": depot, "manifest": manifest_id, "files": manifest.files.len() }),
        });
        tree.add(depot, manifest_id, files_of(&manifest));
        keys.insert(depot, key);
    }
    let cache = ChunkCache::open(steam_dir.join("chunks"), options.cache_cap)?;
    let store = ChunkStore::new(
        cache,
        Arc::new(CdnSource::new(resolver, keys)?),
        PARALLEL_FETCHES,
    )
    .with_cancel_events(cancel.clone(), events.clone());
    let store = Arc::new(store);
    let operator = DepotSource::new(tree, store.clone()).into_operator();
    Ok(SteamInput { operator, store })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depot_spec_parses() {
        assert_eq!(
            "1172471".parse(),
            Ok(DepotSpec {
                depot: 1172471,
                manifest: None
            })
        );
        assert_eq!(
            "1172471=7843921000000000001".parse(),
            Ok(DepotSpec {
                depot: 1172471,
                manifest: Some(7843921000000000001)
            })
        );
        for bad in ["", "abc", "1=", "=2", "1=x", "1=2=3"] {
            assert!(bad.parse::<DepotSpec>().is_err(), "{bad}");
        }
    }
}
