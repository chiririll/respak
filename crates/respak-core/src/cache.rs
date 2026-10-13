//! The plugin cache (PROJECT.md §7): an optional `Operator` that plugins use for small,
//! expensive-to-recompute data. Core never interprets the contents.

use bytes::Bytes;
use opendal::Operator;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::event::Event;
use crate::mediator::Mediator;

/// One plugin's slice of the cache, under `<plugin id>/v<cache_version>/`.
///
/// Plugins must work without a cache, so no method fails: a read error or an undecodable entry
/// is a miss and a write error is dropped, each published as [`Event::Warning`].
#[derive(Clone)]
pub struct PluginCache {
    op: Operator,
    prefix: String,
    mediator: Mediator,
}

impl PluginCache {
    pub(crate) fn new(op: Operator, plugin: &str, version: u32, mediator: Mediator) -> Self {
        Self { op, prefix: format!("{plugin}/v{version}/"), mediator }
    }

    fn path(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key.trim_start_matches('/'))
    }

    fn warn(&self, what: &str, key: &str, error: impl std::fmt::Display) {
        self.mediator.publish(Event::Warning { message: format!("cache: cannot {what} `{}`: {error}", self.path(key)) });
    }

    /// The bytes stored under `key`, if any.
    pub async fn get(&self, key: &str) -> Option<Bytes> {
        match self.op.read(&self.path(key)).await {
            Ok(buf) => Some(buf.to_bytes()),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => None,
            Err(e) => {
                self.warn("read", key, e);
                None
            }
        }
    }

    /// Stores `bytes` under `key`.
    pub async fn put(&self, key: &str, bytes: Bytes) {
        if let Err(e) = self.op.write(&self.path(key), bytes).await {
            self.warn("write", key, e);
        }
    }

    /// Deletes `key`; a missing key is not an error.
    pub async fn remove(&self, key: &str) {
        if let Err(e) = self.op.delete(&self.path(key)).await {
            self.warn("remove", key, e);
        }
    }

    /// The JSON value stored under `key`; an entry that does not decode is a miss.
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let bytes = self.get(key).await?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(e) => {
                self.warn("decode", key, e);
                None
            }
        }
    }

    /// Stores `value` as JSON under `key`.
    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) {
        match serde_json::to_vec(value) {
            Ok(bytes) => self.put(key, bytes.into()).await,
            Err(e) => self.warn("encode", key, e),
        }
    }
}
