//! Where the body of a pak comes from, and how a range of it is read.

use std::ops::Range;

use bytes::Bytes;
use opendal::Operator;
use respak_core::CancellationToken;
use respak_core::{Error, ErrorKind, Result};
use tokio::sync::Mutex;

use super::oodle_read::OodleProgress;
use super::rtech_read::Progress;
use crate::pak::PakHeader;

/// Where the body of a pak comes from.
pub(super) enum Body {
    /// Stored as is: ranged reads of the file.
    Raw {
        op: Operator,
        path: String,
        start: u64,
    },
    /// RTech-compressed: the body is decoded front to back, only as far as reads need.
    Rtech {
        op: Operator,
        path: String,
        start: u64,
        header_len: usize,
        len: u64,
        file_len: u64,
        state: Mutex<Progress>,
        cancel: CancellationToken,
    },
    /// Oodle-compressed: cut into segments that decode on their own, each only when a read needs it.
    Oodle {
        op: Operator,
        path: String,
        start: u64,
        len: u64,
        stream_len: u64,
        state: Mutex<OodleProgress>,
        cancel: CancellationToken,
    },
}

/// `Cancelled` once the host has triggered the call's token.
pub(super) fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(Error::new(ErrorKind::Cancelled));
    }
    Ok(())
}

/// First read of a compressed pak, and the least growth of the input between decoding attempts.
pub(super) const MIN_INPUT: u64 = 1 << 20;

impl Body {
    /// `range` is relative to the start of the body (the byte after the header).
    pub(super) async fn read(&self, range: Range<u64>) -> Result<Bytes> {
        match self {
            Self::Oodle { .. } => self.oodle_read(range).await,
            Self::Raw { op, path, start } => Ok(op
                .read_with(path)
                .range(start + range.start..start + range.end)
                .await?
                .to_bytes()),
            Self::Rtech { .. } => {
                let body = self.decoded(range.end).await?;
                let (s, e) = (usize::try_from(range.start), usize::try_from(range.end));
                match (s, e) {
                    (Ok(s), Ok(e)) if s <= e && e <= body.len() => Ok(body.slice(s..e)),
                    _ => Err(Error::corrupt("read past the end of the decompressed pak")),
                }
            }
        }
    }
}

/// Opens the pak at `path` (the whole file, or `len` bytes from `start`): reads and checks its
/// header and returns it with the body reader. `Corrupt` if the file is shorter than the header
/// says.
pub(super) async fn open_body(
    op: &Operator,
    path: &str,
    start: u64,
    len: Option<u64>,
    cancel: &CancellationToken,
) -> Result<(PakHeader, Body)> {
    let file_len = match len {
        Some(len) => len,
        None => op.stat(path).await?.content_length(),
    };
    let head_len = file_len.min(PakHeader::MAX_LEN as u64);
    let head = op
        .read_with(path)
        .range(start..start + head_len)
        .await?
        .to_vec();
    let header = PakHeader::parse(&head)?;

    let compressed = header.is_rtech();
    let needed = if compressed {
        header.compressed_size
    } else if header.is_oodle() {
        // The compressed size leaves out the header.
        header
            .compressed_size
            .saturating_add(header.header_len as u64)
    } else {
        header.decompressed_size
    };
    if file_len < needed {
        return Err(Error::corrupt(format!(
            "rpak: file has {file_len} bytes, its header says {needed}"
        )));
    }
    let header_len = header.header_len;
    let body = if header.is_oodle() {
        Body::Oodle {
            op: op.clone(),
            path: path.to_owned(),
            start: start + header_len as u64,
            len: header.body_len(),
            stream_len: header.compressed_size,
            state: Mutex::default(),
            cancel: cancel.clone(),
        }
    } else if compressed {
        Body::Rtech {
            op: op.clone(),
            path: path.to_owned(),
            start,
            header_len,
            len: header.body_len(),
            file_len: header.compressed_size,
            state: Mutex::default(),
            cancel: cancel.clone(),
        }
    } else {
        Body::Raw {
            op: op.clone(),
            path: path.to_owned(),
            start: start + header_len as u64,
        }
    };
    Ok((header, body))
}
