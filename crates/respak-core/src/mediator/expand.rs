//! Globs over virtual paths (docs/design/project.md §6).
//!
//! A pattern is split into `/` segments and matched segment by segment. A glob descends into a
//! container only where a pattern segment matches the container's file and more segments
//! follow (`paks/Win64/*.rpak/**/*.txtr`). `**` alone never enters a container: it only walks
//! real directories (`paks/**/*.txtr` opens no rpak).

use globset::Glob;
use opendal::Operator;

use super::{Cx, Mediator};
use crate::error::{Error, ErrorKind, Result};
use crate::value::FileData;

/// Whether `path` has glob syntax in it (otherwise it names one thing).
pub fn is_glob(path: &str) -> bool {
    path.contains(['*', '?', '[', '{'])
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// The children of `dir` on `op` as `(name, is_dir)`, sorted by name.
async fn children(op: &Operator, dir: &str) -> Result<Vec<(String, bool)>> {
    let listed = if dir.is_empty() {
        "/".to_owned()
    } else {
        format!("{dir}/")
    };
    let mut out = Vec::new();
    for e in op.list(&listed).await? {
        let path = e.path().trim_matches('/');
        if e.path() == listed || path == dir.trim_matches('/') {
            continue;
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        out.push((name.to_owned(), e.metadata().is_dir()));
    }
    out.sort();
    Ok(out)
}

impl Mediator {
    /// Expands `pattern` to the virtual paths of the files it matches, sorted, without
    /// duplicates. A pattern without glob syntax is returned as is (not checked). Nothing
    /// matched → `NoMatch`.
    pub async fn expand(&self, pattern: &str, cx: &Cx) -> Result<Vec<String>> {
        let pattern = pattern.trim_matches('/');
        if !is_glob(pattern) {
            return Ok(vec![pattern.to_owned()]);
        }
        let segments: Vec<&str> = pattern.split('/').collect();
        let mut out = Vec::new();
        self.expand_in(&cx.input, "", "", &segments, cx, &mut out)
            .await?;
        out.sort();
        out.dedup();
        if out.is_empty() {
            return Err(Error::new(ErrorKind::NoMatch {
                pattern: pattern.to_owned(),
            }));
        }
        Ok(out)
    }

    /// Matches `segments` below `dir` on `op`; `mount` is the virtual path of `op`'s root.
    async fn expand_in(
        &self,
        op: &Operator,
        mount: &str,
        dir: &str,
        segments: &[&str],
        cx: &Cx,
        out: &mut Vec<String>,
    ) -> Result<()> {
        let (segment, rest) = (segments[0], &segments[1..]);
        let invalid = |e: globset::Error| {
            Error::new(ErrorKind::InvalidRequest {
                message: format!("bad glob `{segment}`: {e}"),
            })
        };

        if segment == "**" {
            if rest.is_empty() {
                // Everything below, in real directories.
                let mut dirs = vec![dir.to_owned()];
                while let Some(d) = dirs.pop() {
                    for (name, is_dir) in children(op, &d).await? {
                        let child = join(&d, &name);
                        if is_dir {
                            dirs.push(child);
                        } else {
                            out.push(join(mount, &child));
                        }
                    }
                }
                return Ok(());
            }
            // Zero directories, then one more.
            Box::pin(self.expand_in(op, mount, dir, rest, cx, out)).await?;
            for (name, is_dir) in children(op, dir).await? {
                if is_dir {
                    Box::pin(self.expand_in(op, mount, &join(dir, &name), segments, cx, out))
                        .await?;
                }
            }
            return Ok(());
        }

        // A plain name needs no listing.
        let candidates: Vec<(String, bool)> = if is_glob(segment) {
            let matcher = Glob::new(segment).map_err(invalid)?.compile_matcher();
            children(op, dir)
                .await?
                .into_iter()
                .filter(|(name, _)| matcher.is_match(name))
                .collect()
        } else {
            let path = join(dir, segment);
            // Some services only know a directory by its trailing slash.
            let found = match op.stat(&path).await {
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                    op.stat(&format!("{path}/")).await
                }
                other => other,
            };
            match found {
                Ok(m) => vec![(segment.to_owned(), m.is_dir())],
                Err(e) if e.kind() == opendal::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(e.into()),
            }
        };
        for (name, is_dir) in candidates {
            let child = join(dir, &name);
            match (is_dir, rest.is_empty()) {
                (false, true) => out.push(join(mount, &child)),
                (true, false) => Box::pin(self.expand_in(op, mount, &child, rest, cx, out)).await?,
                (false, false) => {
                    // More pattern follows a file: it must be a container.
                    let virtual_path = join(mount, &child);
                    let file: crate::value::Value = std::sync::Arc::new(FileData::new(
                        op.clone(),
                        child.clone(),
                        virtual_path.clone(),
                        None,
                    ));
                    match self.mount(file, cx).await {
                        Ok(container) => {
                            let c = container.as_container().expect("mount checks this");
                            let (volume, mount_path) = (c.volume(), c.mount_path().to_owned());
                            Box::pin(self.expand_in(&volume, &mount_path, "", rest, cx, out))
                                .await?;
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                ErrorKind::NotFound { .. } | ErrorKind::UnknownFormat
                            ) => {}
                        Err(e) => cx.warn(format!("cannot look inside {virtual_path}: {e}")),
                    }
                }
                // A directory matched the last segment: only files are outputs.
                (true, true) => {}
            }
        }
        Ok(())
    }
}
