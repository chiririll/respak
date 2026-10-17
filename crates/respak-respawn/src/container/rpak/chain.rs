//! A patched pak's `.mem`: the page map and the files of the patch chain it points into.

use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use respak_core::{Error, ErrorKind, Result};

use super::body::{open_body, Body};
use super::names::{pak_stem, patch_file_name};
use super::volume::Inner;
use crate::patch::{PageMap, Source};

/// One file of a patch chain: its body, and where it starts in `F` (the bodies back to back).
pub(super) struct Link {
    pub(super) start: u64,
    pub(super) len: u64,
    pub(super) body: Arc<Body>,
}

/// A patched pak's `.mem`: the map, the replacement data and the files it points into.
pub(super) struct Resolved {
    pub(super) map: PageMap,
    /// The edit stream; the replacement data starts at `map.replacement_at`.
    pub(super) stream: Bytes,
    pub(super) chain: Vec<Link>,
}

impl Resolved {
    async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        let mut out = Vec::with_capacity((range.end - range.start) as usize);
        for piece in self.map.locate(range)? {
            match piece.source {
                Source::Replacement(at) => {
                    let from = self.map.replacement_at as u64 + at;
                    let bytes = self.stream.get(from as usize..(from + piece.len) as usize);
                    out.extend_from_slice(bytes.ok_or_else(|| {
                        Error::corrupt("rpak patch: replacement data past the edit stream")
                    })?);
                }
                Source::File(mut at) => {
                    let mut left = piece.len;
                    while left > 0 {
                        let link = self
                            .chain
                            .iter()
                            .find(|l| (l.start..l.start + l.len).contains(&at))
                            .ok_or_else(|| {
                                Error::corrupt(
                                    "rpak patch: the script reads past the end of the pak files",
                                )
                            })?;
                        let n = left.min(link.start + link.len - at);
                        out.extend_from_slice(
                            &link.body.read(at - link.start..at - link.start + n).await?,
                        );
                        at += n;
                        left -= n;
                    }
                }
            }
        }
        Ok(Bytes::from(out))
    }
}

impl Inner {
    /// The bytes of `.mem` (the pages back to back) in `range`.
    pub(super) async fn read_mem(&self, range: Range<u64>) -> Result<Bytes> {
        let Some(patched) = &self.patched else {
            let base = self.tables.page_data_offset;
            return self.body.read(base + range.start..base + range.end).await;
        };
        patched
            .get_or_try_init(|| self.resolve())
            .await?
            .read(range)
            .await
    }

    /// Reads the edit stream, builds the page map and opens the lower files of the chain.
    async fn resolve(&self) -> Result<Resolved> {
        let patch = self
            .tables
            .patch
            .as_ref()
            .expect("a patched pak has a patch block");
        let tables_end = self.header.tables_len();
        let stream = self
            .body
            .read(tables_end..tables_end + patch.stream_size)
            .await?;
        let stored: u64 = self.tables.pages[patch.patch_pages as usize..]
            .iter()
            .map(|p| u64::from(p.size))
            .sum();
        let map = PageMap::build(
            &stream,
            self.tables.mem_len(),
            stored,
            self.tables.page_data_offset,
        )?;

        let top_len = self.header.body_len();
        let mut chain = vec![Link {
            start: 0,
            len: top_len,
            body: self.body.clone(),
        }];
        let stem = pak_stem(&self.file);
        let mut start = top_len;
        for &index in &patch.indices {
            let name = patch_file_name(stem, u32::from(index));
            let path = format!("{}{name}", self.dir);
            let (header, body) = match open_body(&self.host, &path, 0, None, &self.cancel).await {
                Ok(opened) => opened,
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    return Err(Error::new(ErrorKind::MissingDependency {
                        item: self.file.clone(),
                        file: path,
                    }));
                }
                Err(e) => return Err(e),
            };
            let len = header.body_len();
            chain.push(Link {
                start,
                len,
                body: Arc::new(body),
            });
            start += len;
        }
        Ok(Resolved { map, stream, chain })
    }
}
