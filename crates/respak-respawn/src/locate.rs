//! `rpak.guid -> rpak.asset`: finding an asset by guid when it lives in another pak
//! (PROJECT.md §6).
//!
//! The search order is fixed, and it never scans paks on its own:
//! 1. the pak the reference came from;
//! 2. `common.rpak` in the same directory;
//! 3. the plugin's search list (globs, `RespawnPlugin::search`);
//! 4. the GUID -> pak index in the plugin cache.
//!
//! A hit found through the search list is written to the index, so a later run can find it
//! without the list. Nothing found is `MissingDependency`.

use std::sync::Arc;

use async_trait::async_trait;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey, Version,
};

/// A reference to an asset by guid and kind, made by an asset in the pak at `origin`.
#[derive(Debug)]
pub struct GuidRef {
    format: FormatId,
    key: ValueKey,
    /// The asset's guid.
    pub guid: u64,
    /// The asset's kind, e.g. `txtr`.
    pub kind: String,
    /// Virtual path of the pak the reference was found in.
    pub origin: String,
}

impl GuidRef {
    /// A reference to `guid` of `kind`, made from inside the pak at `origin`.
    pub fn new(guid: u64, kind: &str, origin: &str) -> Self {
        Self {
            format: FormatId::new("rpak.guid").with_version(Version::new(1)),
            key: ValueKey::file(format!("{origin}/guid:{kind}:0x{guid:016X}")),
            guid,
            kind: kind.to_owned(),
            origin: origin.to_owned(),
        }
    }

    /// The asset's file name in a pak volume: `0x<GUID>.<kind>`.
    pub fn file_name(&self) -> String {
        format!("0x{:016X}.{}", self.guid, self.kind)
    }
}

impl Data for GuidRef {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
}

/// Read edge `rpak.guid -> rpak.asset`.
pub struct RpakLocate {
    search: Arc<Vec<String>>,
}

impl RpakLocate {
    /// A locator that searches `search` (globs on the input) after the pak itself and
    /// `common.rpak`.
    pub fn new(search: Arc<Vec<String>>) -> Self {
        Self { search }
    }
}

/// Tries `pak/file`; `Ok(None)` if the pak or the asset is not there.
async fn try_pak(pak: &str, file: &str, cx: &Cx) -> Result<Option<Value>> {
    match cx.mediator.open(&format!("{pak}/{file}"), cx).await {
        Ok(asset) => Ok(Some(asset)),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. } | ErrorKind::NoMatch { .. }) => Ok(None),
        Err(e) if matches!(e.kind(), ErrorKind::Cancelled) => Err(e),
        Err(e) => {
            cx.warn(format!("cannot look for {file} in {pak}: {e}"));
            Ok(None)
        }
    }
}

/// The virtual path of the pak an asset key points into (the key without its last segment).
fn pak_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(pak, _)| pak)
}

#[async_trait]
impl Detect for RpakLocate {
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(input.source.is::<GuidRef>().then_some(Detected { score: 10, version: None }))
    }
}

#[async_trait]
impl ReadHandler for RpakLocate {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let r = source
            .downcast_ref::<GuidRef>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "rpak.guid expected".into() }))?;
        let file = r.file_name();
        let mut tried = vec![r.origin.clone()];

        // 1. The same pak.
        if let Some(asset) = try_pak(&r.origin, &file, cx).await? {
            return Ok(asset);
        }
        // 2. `common.rpak` next to it.
        let dir = r.origin.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
        let common = format!("{dir}common.rpak");
        if common != r.origin {
            if let Some(asset) = try_pak(&common, &file, cx).await? {
                return Ok(asset);
            }
            tried.push(common);
        }
        // 3. The search list.
        let cache = cx.cache("respawn");
        let index_key = format!("guid/{:016x}.{}", r.guid, r.kind);
        for pattern in self.search.iter() {
            let paks = match cx.mediator.expand(pattern, cx).await {
                Ok(paks) => paks,
                Err(e) if matches!(e.kind(), ErrorKind::NoMatch { .. }) => continue,
                Err(e) => return Err(e),
            };
            for pak in paks {
                cx.check_cancelled()?;
                if tried.contains(&pak) {
                    continue;
                }
                if let Some(asset) = try_pak(&pak, &file, cx).await? {
                    if let Some(cache) = &cache {
                        cache.put_json(&index_key, &pak_of(&asset.key().to_string())).await;
                    }
                    return Ok(asset);
                }
                tried.push(pak);
            }
        }
        // 4. The index.
        if let Some(cache) = &cache {
            if let Some(pak) = cache.get_json::<String>(&index_key).await {
                if !tried.contains(&pak) {
                    if let Some(asset) = try_pak(&pak, &file, cx).await? {
                        return Ok(asset);
                    }
                    cx.warn(format!("the cached location {pak} of {file} is stale"));
                    cache.remove(&index_key).await;
                }
            }
        }
        Err(Error::new(ErrorKind::MissingDependency {
            item: format!("{} {:#018x}", r.kind, r.guid),
            file: format!("not found in {}", tried.join(", ")),
        }))
    }
}
