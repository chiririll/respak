//! The virtual tree and name search (docs/design/webui.md section 6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::{extension, Listing, Session};

/// What a row of the tree is.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// A directory of the input.
    Dir,
    /// A file of the input.
    File,
    /// A file that opens into entries.
    Container,
    /// A folder inside a container.
    Folder,
    /// An entry of a container.
    Asset,
    /// All the entries of one kind (kind view).
    Group,
}

/// One row.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// What is shown.
    pub name: String,
    /// The virtual path (for a group: the container's).
    pub path: String,
    /// What it is.
    pub kind: NodeKind,
    /// Size in bytes of a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Extension, lower case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
    /// For a folder or group: how many entries are below.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// A page of rows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// The path asked for.
    pub path: String,
    /// The container it is in, if it is in one.
    pub container: Option<String>,
    /// All rows there are.
    pub total: usize,
    /// Where this page starts.
    pub offset: usize,
    /// The rows.
    pub nodes: Vec<Node>,
    /// Whether the names of a container were read.
    pub named: bool,
}

/// The query of `GET /api/tree`.
#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    /// Virtual path; empty is the root.
    #[serde(default)]
    pub path: String,
    /// `path` (default) or `kind`, inside a container.
    #[serde(default)]
    pub view: String,
    /// In kind view: the kind to list.
    pub group: Option<String>,
    /// Read the names of a container's entries (slow).
    #[serde(default)]
    pub names: bool,
    /// First row.
    #[serde(default)]
    pub offset: usize,
    /// Rows to return, at most [`MAX_LIMIT`].
    pub limit: Option<usize>,
}

/// Rows per page when the page does not say.
pub const DEFAULT_LIMIT: usize = 500;
/// The most rows a page can ask for.
pub const MAX_LIMIT: usize = 2000;

/// Cleans a virtual path the page sent: no empty, `.` or `..` parts.
pub fn clean(path: &str) -> ApiResult<String> {
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(ApiError::bad_request("`..` is not allowed in a path")),
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Splits a virtual path at its first container: `(container, rest)`.
pub fn split_container<'a>(session: &Session, path: &'a str) -> Option<(&'a str, &'a str)> {
    let mut end = 0;
    for part in path.split('/') {
        end += part.len();
        if session.is_container_name(part) {
            let rest = path.get(end + 1..).unwrap_or("");
            return Some((&path[..end], rest));
        }
        end += 1;
    }
    None
}

/// One page of the tree.
pub async fn page(session: &Session, query: TreeQuery) -> ApiResult<Page> {
    let path = clean(&query.path)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let (nodes, total, container, named) = match split_container(session, &path) {
        Some((container, inner)) => {
            let listing = session.listing(container, query.names).await?;
            let nodes = if query.view == "kind" {
                kind_view(container, &listing, query.group.as_deref())
            } else {
                folder_view(container, inner, &listing)
            };
            let total = nodes.len();
            (nodes, total, Some(container.to_owned()), listing.named)
        }
        None => {
            let nodes = dir_view(session, &path).await?;
            let total = nodes.len();
            (nodes, total, None, false)
        }
    };
    let nodes = nodes.into_iter().skip(query.offset).take(limit).collect();
    Ok(Page {
        path,
        container,
        total,
        offset: query.offset,
        nodes,
        named,
    })
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_owned()
    } else {
        format!("{base}/{name}")
    }
}

/// The children of a directory of the input: directories first, then files, by name.
async fn dir_view(session: &Session, dir: &str) -> ApiResult<Vec<Node>> {
    let listed = if dir.is_empty() {
        "/".to_owned()
    } else {
        format!("{dir}/")
    };
    let entries = session
        .input
        .list(&listed)
        .await
        .map_err(|e| match e.kind() {
            opendal::ErrorKind::NotFound => {
                ApiError::not_found(format!("{dir} is not a directory"))
            }
            _ => ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Storage",
                e.to_string(),
            ),
        })?;
    let mut nodes = Vec::new();
    for entry in entries {
        let full = entry.path().trim_matches('/');
        if entry.path() == listed || full == dir {
            continue;
        }
        let name = full.rsplit('/').next().unwrap_or(full).to_owned();
        let is_dir = entry.metadata().is_dir();
        let ext = extension(&name);
        let kind = if is_dir {
            NodeKind::Dir
        } else if session.is_container_name(&name) {
            NodeKind::Container
        } else {
            NodeKind::File
        };
        nodes.push(Node {
            path: full.to_owned(),
            name,
            kind,
            size: (!is_dir).then(|| entry.metadata().content_length()),
            ext,
            count: None,
        });
    }
    nodes.sort_by(|a, b| {
        let dir = |n: &Node| n.kind != NodeKind::Dir;
        dir(a)
            .cmp(&dir(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(nodes)
}

/// The children of the folder `inner` of a container, by display path.
fn folder_view(container: &str, inner: &str, listing: &Listing) -> Vec<Node> {
    let prefix = if inner.is_empty() {
        String::new()
    } else {
        format!("{inner}/")
    };
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    let mut assets = Vec::new();
    for item in &listing.items {
        let Some(rest) = item.display.strip_prefix(&prefix) else {
            continue;
        };
        match rest.split_once('/') {
            Some((folder, _)) => *folders.entry(folder.to_owned()).or_default() += 1,
            None => assets.push(Node {
                name: rest.to_owned(),
                path: join(container, &item.path),
                kind: NodeKind::Asset,
                size: None,
                ext: extension(rest),
                count: None,
            }),
        }
    }
    let mut nodes: Vec<Node> = folders
        .into_iter()
        .map(|(name, count)| Node {
            path: join(container, &join(inner, &name)),
            name,
            kind: NodeKind::Folder,
            size: None,
            ext: None,
            count: Some(count),
        })
        .collect();
    nodes.extend(assets);
    nodes
}

/// The kinds of a container, or the entries of one kind.
fn kind_view(container: &str, listing: &Listing, group: Option<&str>) -> Vec<Node> {
    match group {
        None => {
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for item in &listing.items {
                let kind = extension(&item.display)
                    .or_else(|| extension(&item.path))
                    .unwrap_or_default();
                *kinds.entry(kind).or_default() += 1;
            }
            kinds
                .into_iter()
                .map(|(kind, count)| Node {
                    name: if kind.is_empty() {
                        "(none)".into()
                    } else {
                        kind.clone()
                    },
                    path: container.to_owned(),
                    kind: NodeKind::Group,
                    size: None,
                    ext: Some(kind),
                    count: Some(count),
                })
                .collect()
        }
        Some(group) => listing
            .items
            .iter()
            .filter(|item| {
                extension(&item.display)
                    .or_else(|| extension(&item.path))
                    .unwrap_or_default()
                    == group
            })
            .map(|item| Node {
                name: item.display.clone(),
                path: join(container, &item.path),
                kind: NodeKind::Asset,
                size: None,
                ext: extension(&item.display),
                count: None,
            })
            .collect(),
    }
}

/// A search hit.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// Virtual path.
    pub path: String,
    /// Shown name.
    pub name: String,
    /// `file` or `asset`.
    pub kind: &'static str,
    /// The container of an asset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

/// The query of `GET /api/search`.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Words that must all appear in the name, in any case.
    #[serde(default)]
    pub q: String,
    /// `files`, `loaded` or `all` (default).
    #[serde(default)]
    pub scope: String,
    /// At most this many hits (default 200).
    pub limit: Option<usize>,
}

/// The answer of a search.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// The hits, at most `limit`.
    pub hits: Vec<Hit>,
    /// Whether there were more.
    pub truncated: bool,
    /// How many containers were looked into.
    pub containers: usize,
}

/// Searches names: the files of the input, and the entries of the containers opened so far.
pub async fn search(session: &Session, query: SearchQuery) -> ApiResult<SearchResult> {
    let words: Vec<String> = query.q.split_whitespace().map(str::to_lowercase).collect();
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let mut result = SearchResult {
        hits: Vec::new(),
        truncated: false,
        containers: 0,
    };
    if words.is_empty() {
        return Ok(result);
    }
    let matches = |text: &str| {
        let lower = text.to_lowercase();
        words.iter().all(|w| lower.contains(w))
    };
    if query.scope != "loaded" {
        for file in session.file_index().await?.iter() {
            if words.iter().all(|w| file.lower.contains(w)) {
                if result.hits.len() == limit {
                    result.truncated = true;
                    break;
                }
                result.hits.push(Hit {
                    name: file
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file.path)
                        .to_owned(),
                    path: file.path.clone(),
                    kind: "file",
                    container: None,
                });
            }
        }
    }
    if query.scope != "files" {
        'containers: for (container, listing) in session.loaded() {
            result.containers += 1;
            for item in &listing.items {
                if matches(&item.display) || matches(&item.path) {
                    if result.hits.len() == limit {
                        result.truncated = true;
                        break 'containers;
                    }
                    result.hits.push(Hit {
                        name: item.display.clone(),
                        path: join(&container, &item.path),
                        kind: "asset",
                        container: Some(container.clone()),
                    });
                }
            }
        }
    }
    Ok(result)
}
