//! `file -> mbnk`: detection, reading and the opened container.

use std::sync::Arc;

use async_trait::async_trait;
use opendal::Operator;
use respak_core::capability::Container;
use respak_core::detect::{magic, u32le};
use respak_core::value::FileData;
use respak_core::volume::volume_operator;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use super::bank::Bank;
use super::layout::{BankLayout, MAGIC, MBNK_HEADER_LEN};
use super::not_found;
use super::source::MbnkSource;
use crate::asrc::{AsrcData, StreamHeads};

/// An opened bank: `Data` + `Container`.
#[derive(Debug)]
pub struct MbnkData {
    format: FormatId,
    version: Version,
    key: ValueKey,
    mount: String,
    volume: Operator,
    host: Operator,
    dir: String,
    bank: Arc<Bank>,
    heads: StreamHeads,
}

impl MbnkData {
    /// The parsed bank.
    pub fn bank(&self) -> &Bank {
        &self.bank
    }
}

#[async_trait]
impl Container for MbnkData {
    fn volume(&self) -> Operator {
        self.volume.clone()
    }

    fn mount_path(&self) -> &str {
        &self.mount
    }

    /// Hands out an `asrc` ([`AsrcData`]) for a source path. Opening it reads the `.mstr`
    /// header and the source's codec header, so a missing stream is reported here.
    async fn entry(&self, path: &str) -> Result<Value> {
        let key = ValueKey::file(format!("{}/{path}", self.mount));
        if let Some(source) = self.bank.find(path) {
            let asrc = AsrcData::open(
                &self.host,
                &self.dir,
                &self.bank,
                source,
                key,
                self.version,
                &self.heads,
            )
            .await?;
            return Ok(Arc::new(asrc));
        }
        if path.is_empty() || self.bank.is_dir(path) {
            return Ok(Arc::new(FileData::new(
                self.volume.clone(),
                path,
                format!("{}/{path}", self.mount),
                None,
            )));
        }
        Err(not_found(path))
    }
}

impl Data for MbnkData {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn key(&self) -> &ValueKey {
        &self.key
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

/// Read edge `file -> mbnk`.
pub struct MbnkRead;

#[async_trait]
impl Detect for MbnkRead {
    /// Magic `CBNK` = 10; a known version = +2; an unknown one is `UnsupportedVersion`; a
    /// `fileSize` field equal to the length = +2. `Version { major: bank version, minor: 0 }`.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, MAGIC) {
            return Ok(None);
        }
        let Some(version) = u32le(input.header, 4) else {
            return Ok(None);
        };
        let version = version as i32;
        if BankLayout::for_version(version).is_none() {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "mbnk".into(),
                version: version.to_string(),
            }));
        }
        let mut score = 12;
        if input
            .len
            .is_some_and(|len| u32le(input.header, 8).map(u64::from) == Some(len))
        {
            score += 2;
        }
        Ok(Some(Detected {
            score,
            version: Some(Version::new(version as u32)),
        }))
    }

    fn header_len(&self) -> usize {
        MBNK_HEADER_LEN
    }
}

#[async_trait]
impl ReadHandler for MbnkRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "mbnk must be read from a file".into(),
            })
        })?;
        let read = file.op.read_with(&file.path);
        let bytes = match &file.range {
            Some(r) => read.range(r.clone()).await?,
            None => read.await?,
        }
        .to_bytes();
        let bank = Arc::new(Bank::parse(&bytes)?);
        let mount = source.key().to_string();
        let volume = volume_operator(MbnkSource(bank.clone()), &mount);
        let dir = file
            .path
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        let version = Version::new(bank.layout().version as u32);
        let format = FormatId::new("mbnk").with_version(version);
        Ok(Arc::new(MbnkData {
            format,
            version,
            key: source.key().clone(),
            mount,
            volume,
            host: file.op.clone(),
            dir,
            bank,
            heads: StreamHeads::default(),
        }))
    }
}
