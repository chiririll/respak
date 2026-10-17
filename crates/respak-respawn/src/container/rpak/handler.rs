//! `file -> rpak`: detection and opening.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use respak_core::value::FileData;
use respak_core::volume::volume_operator;
use respak_core::{
    magic, u16le, Cx, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result,
    Value, Version,
};
use tokio::sync::OnceCell;

use super::body::open_body;
use super::data::RpakData;
use super::master::newest_patch;
use super::volume::{Inner, RpakSource};
use crate::pak::{PakHeader, Tables};

/// Read edge `file -> rpak`.
#[derive(Debug, Clone, Copy)]
pub struct RpakRead {
    patches: bool,
}

impl Default for RpakRead {
    fn default() -> Self {
        Self::new(true)
    }
}

impl RpakRead {
    /// With `patches` off, a patch master is not consulted and a patch file is refused as
    /// `UnsupportedVersion`.
    pub fn new(patches: bool) -> Self {
        Self { patches }
    }
}

#[async_trait]
impl Detect for RpakRead {
    /// Magic `RPak` = 10, a supported version (6, 7, 8) = +2, extension `rpak` = +1.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, b"RPak") {
            return Ok(None);
        }
        let version = u16le(input.header, 4);
        let known = version.is_some_and(|v| PakHeader::len_for(v).is_some());
        let score = 10 + 2 * u32::from(known) + u32::from(input.extension == Some("rpak"));
        Ok(Some(Detected {
            score,
            version: version.map(|v| Version::new(u32::from(v))),
        }))
    }

    fn header_len(&self) -> usize {
        8
    }
}

#[async_trait]
impl ReadHandler for RpakRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "rpak must be read from a file".into(),
            })
        })?;
        let start = file.range.as_ref().map_or(0, |r| r.start);
        let len = file.range.as_ref().map(|r| r.end - r.start);
        // `common.rpak` next to a `patch_master.rpak` is opened as its newest patch.
        let patch = match (self.patches, &file.range) {
            (true, None) => newest_patch(&file.op, &file.path, &cx.cancel).await,
            _ => None,
        };
        let path = patch.as_ref().unwrap_or(&file.path).clone();
        let (header, body) = open_body(
            &file.op,
            &path,
            start,
            if patch.is_some() { None } else { len },
            &cx.cancel,
        )
        .await?;
        if !self.patches && header.patch_count > 0 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "rpak".into(),
                version: format!(
                    "{} (patch file, patchCount {}; patches are ignored)",
                    header.version, header.patch_count
                ),
            }));
        }
        let table_bytes = body.read(0..header.tables_len()).await?;
        let tables = Tables::parse(&header, &table_bytes)?;

        let mut by_guid = HashMap::new();
        for (i, a) in tables.assets.iter().enumerate() {
            by_guid.entry(a.guid).or_insert(i);
        }
        let dir = path
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        let opened = (0..tables.starpaks.len() + tables.opt_starpaks.len())
            .map(|_| OnceCell::new())
            .collect();
        let patched = tables.patch.is_some().then(OnceCell::new);
        let inner = Arc::new(Inner {
            header,
            tables,
            body: Arc::new(body),
            file: path,
            patched,
            by_guid,
            host: file.op.clone(),
            cancel: cx.cancel.clone(),
            dir,
            opened,
        });
        let mount = source.key().to_string();
        let volume = volume_operator(RpakSource(inner.clone()), &mount);
        let format =
            FormatId::new("rpak").with_version(Version::new(u32::from(inner.header.version)));
        Ok(Arc::new(RpakData {
            format,
            key: source.key().clone(),
            mount,
            volume,
            inner,
        }))
    }
}
