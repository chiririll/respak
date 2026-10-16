//! Virtual paths: `paks/Win64/common.rpak/texture/foo.txtr` (PROJECT.md §6).

use std::collections::HashMap;
use std::sync::Mutex;

use opendal::Operator;

use crate::error::{Error, ErrorKind, Result};
use crate::mediator::{Cx, Mediator};
use crate::value::{FileData, Value};

/// Containers mounted during one `Engine` call, by the virtual path of their file. A mounted
/// volume lives until the call ends (PROJECT.md §5.6, §7).
#[derive(Default)]
pub struct Mounts {
    containers: Mutex<HashMap<String, Value>>,
}

impl Mounts {
    fn get(&self, key: &str) -> Option<Value> {
        self.containers.lock().unwrap().get(key).cloned()
    }

    fn insert(&self, key: String, value: Value) {
        self.containers.lock().unwrap().insert(key, value);
    }

    /// The path of `path` inside the innermost mounted container that contains it
    /// (`paks/Win64/common.rpak/texture/foo.txtr` -> `texture/foo.txtr`), or `path` itself if it
    /// is not inside any.
    pub fn path_in_container<'a>(&self, path: &'a str) -> &'a str {
        let containers = self.containers.lock().unwrap();
        containers
            .keys()
            .filter_map(|k| {
                path.strip_prefix(k.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
            })
            .min_by_key(|rest| rest.len())
            .unwrap_or(path)
    }

    /// How many containers are mounted.
    pub fn len(&self) -> usize {
        self.containers.lock().unwrap().len()
    }

    /// Whether nothing is mounted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn join(mount: &str, rel: &str) -> String {
    if mount.is_empty() {
        rel.to_owned()
    } else {
        format!("{mount}/{rel}")
    }
}

fn not_found(path: &str) -> Error {
    Error::new(ErrorKind::NotFound {
        path: path.to_owned(),
    })
}

impl Mediator {
    /// Resolves a virtual path to a value.
    ///
    /// Resolution goes left to right: the longest prefix that is a real file is identified; if it
    /// is a container, the rest is resolved against its volume (`Container::entry`); repeat.
    /// Real files win over container entries. A path that is not there is `NotFound`.
    pub async fn open(&self, path: &str, cx: &Cx) -> Result<Value> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return Err(not_found(path));
        }
        self.resolve(cx.input.clone(), "", path, None, cx).await
    }

    /// The container behind `value`: the value itself if it is one, or the identified `file`.
    /// Mounted containers are kept in `cx.mounts`.
    pub async fn mount(&self, file: Value, cx: &Cx) -> Result<Value> {
        let key = file.key().to_string();
        if let Some(mounted) = cx.mounts.get(&key) {
            return Ok(mounted);
        }
        let value = if file.as_container().is_some() {
            file
        } else {
            match self.identify(file, cx).await {
                Ok(v) => v,
                // Something that is not any known format is not a container either.
                Err(e) if matches!(e.kind(), ErrorKind::UnknownFormat) => {
                    return Err(not_found(&key))
                }
                Err(e) => return Err(e),
            }
        };
        if value.as_container().is_none() {
            return Err(not_found(&key));
        }
        cx.mounts.insert(key, value.clone());
        let c = value.as_container().expect("checked above");
        let entries = crate::engine::list_volume(&c.volume())
            .await
            .map_or(0, |files| files.len());
        cx.mediator.publish(crate::event::Event::InputOpened {
            path: c.mount_path().to_owned(),
            entries,
        });
        Ok(value)
    }

    async fn resolve(
        &self,
        op: Operator,
        mount: &str,
        rel: &str,
        container: Option<Value>,
        cx: &Cx,
    ) -> Result<Value> {
        let segments: Vec<&str> = rel.split('/').collect();
        // Inside a container the whole path is the container's own business (`entry`), so only
        // proper prefixes are looked for there (a nested container file).
        let longest = if container.is_some() {
            segments.len() - 1
        } else {
            segments.len()
        };
        for n in (1..=longest).rev() {
            let prefix = segments[..n].join("/");
            match op.stat(&prefix).await {
                Ok(m) if m.is_file() => {
                    if n == segments.len() {
                        return Ok(std::sync::Arc::new(FileData::new(
                            op,
                            prefix.clone(),
                            join(mount, &prefix),
                            None,
                        )));
                    }
                    let file: Value = std::sync::Arc::new(FileData::new(
                        op.clone(),
                        prefix.clone(),
                        join(mount, &prefix),
                        None,
                    ));
                    let inner = self.mount(file, cx).await?;
                    let c = inner.as_container().expect("mount checks this");
                    let rest = segments[n..].join("/");
                    let (volume, mount_path) = (c.volume(), c.mount_path().to_owned());
                    return Box::pin(self.resolve(
                        volume,
                        &mount_path,
                        &rest,
                        Some(inner.clone()),
                        cx,
                    ))
                    .await;
                }
                Ok(_) => {}
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        match container {
            Some(c) => {
                c.as_container()
                    .expect("a mounted container")
                    .entry(rel)
                    .await
            }
            None => Err(not_found(&join(mount, rel))),
        }
    }
}
