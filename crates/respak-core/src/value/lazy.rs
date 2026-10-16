//! `LazyBlob`: a [`Blob`] whose bytes are computed when someone first asks for them.

use std::fs::File;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::Mutex;

use super::{Data, FormatId, ValueKey};
use crate::capability::Blob;
use crate::error::{Error, ErrorKind, Result};
use crate::Cx;

/// Produced bytes kept in memory before they move to a temporary file.
const MEMORY_LIMIT: u64 = 64 << 20;

/// Marks a length that is not known yet.
const UNKNOWN: u64 = u64::MAX;

/// What a [`LazyBlob`] computes its bytes from.
pub enum LazySource {
    /// Bytes in order, produced as the stream is polled. The conversion advances only as far as
    /// the readers ask.
    Stream(BoxStream<'static, Result<Bytes>>),
    /// A finished file, for a producer that cannot go in order (a muxer that seeks back to patch
    /// its header). The blob owns the file from here on; an anonymous temporary file is deleted
    /// when it is closed.
    File(File),
}

/// Starts the computation. Called once, by the first read, with that read's context.
pub type LazyInit = Box<dyn FnOnce(Cx) -> BoxFuture<'static, Result<LazySource>> + Send>;

/// A blob computed on demand, with the computed ranges kept.
///
/// Nothing runs until the first [`Blob::read`]. A [`LazySource::Stream`] is then advanced just far
/// enough to cover each read. What it produced is kept, so reading a range again (or an earlier
/// one) costs nothing and never recomputes: up to 64 MiB in memory, past that in a temporary file,
/// so a result of any size passes through with bounded memory.
///
/// Readers are served one at a time. A failure is final: the read that hit it gets the error and
/// later reads get a description of it.
pub struct LazyBlob {
    format: FormatId,
    key: ValueKey,
    len: AtomicU64,
    state: Mutex<State>,
}

struct State {
    init: Option<LazyInit>,
    stream: Option<BoxStream<'static, Result<Bytes>>>,
    spool: Spool,
    failed: Option<String>,
}

impl LazyBlob {
    /// A blob of `format` that `init` computes when it is first read.
    pub fn new(format: FormatId, key: ValueKey, init: LazyInit) -> Self {
        let state = State {
            init: Some(init),
            stream: None,
            spool: Spool::memory(),
            failed: None,
        };
        Self {
            format,
            key,
            len: AtomicU64::new(UNKNOWN),
            state: Mutex::new(state),
        }
    }
}

impl std::fmt::Debug for LazyBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyBlob")
            .field("format", &self.format)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl Data for LazyBlob {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_blob(&self) -> Option<&dyn Blob> {
        Some(self)
    }
}

fn failed(message: &str) -> Error {
    Error::new(ErrorKind::Custom {
        source: None,
        message: format!("the conversion failed earlier: {message}"),
    })
}

#[async_trait]
impl Blob for LazyBlob {
    fn len(&self) -> Option<u64> {
        match self.len.load(Ordering::Acquire) {
            UNKNOWN => None,
            len => Some(len),
        }
    }

    async fn read(&self, range: Range<u64>, cx: &Cx) -> Result<Bytes> {
        let mut state = self.state.lock().await;
        if let Some(message) = &state.failed {
            return Err(failed(message));
        }
        if let Err(e) = self.produce(&mut state, range.end, cx).await {
            // A cancelled read says nothing about the data: a later read may start over.
            if !matches!(e.kind(), ErrorKind::Cancelled) {
                state.failed = Some(e.to_string());
            }
            return Err(e);
        }
        state.spool.read(range, cx).await
    }
}

impl LazyBlob {
    /// Runs the computation until `end` bytes exist, or it is finished.
    async fn produce(&self, state: &mut State, end: u64, cx: &Cx) -> Result<()> {
        if let Some(init) = state.init.take() {
            match init(cx.clone()).await? {
                LazySource::Stream(stream) => state.stream = Some(stream),
                LazySource::File(file) => {
                    let len = file.metadata().map_err(io_error)?.len();
                    state.spool = Spool::Disk {
                        file: Arc::new(file),
                        len,
                    };
                    self.len.store(len, Ordering::Release);
                }
            }
        }
        while let Some(stream) = state.stream.as_mut() {
            if state.spool.len() >= end {
                break;
            }
            cx.check_cancelled()?;
            match stream.next().await {
                Some(chunk) => state.spool.push(chunk?, cx).await?,
                None => {
                    state.stream = None;
                    self.len.store(state.spool.len(), Ordering::Release);
                }
            }
        }
        Ok(())
    }
}

fn io_error(e: std::io::Error) -> Error {
    Error::new(ErrorKind::Custom {
        source: Some(Box::new(e)),
        message: "lazy blob storage".into(),
    })
}

/// The produced bytes: in memory while they are few, in a temporary file after.
enum Spool {
    Memory { chunks: Vec<(u64, Bytes)>, len: u64 },
    Disk { file: Arc<File>, len: u64 },
}

impl Spool {
    fn memory() -> Self {
        Self::Memory {
            chunks: Vec::new(),
            len: 0,
        }
    }

    fn len(&self) -> u64 {
        match self {
            Self::Memory { len, .. } | Self::Disk { len, .. } => *len,
        }
    }

    async fn push(&mut self, bytes: Bytes, cx: &Cx) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if let Self::Memory { chunks, len } = self {
            if *len + bytes.len() as u64 <= MEMORY_LIMIT {
                chunks.push((*len, bytes.clone()));
                *len += bytes.len() as u64;
                return Ok(());
            }
            // Over the limit: everything so far moves to a file.
            let file = Arc::new(tempfile::tempfile().map_err(io_error)?);
            let held = std::mem::take(chunks);
            let target = file.clone();
            cx.blocking(move || -> std::io::Result<()> {
                for (at, chunk) in held {
                    write_at(&target, &chunk, at)?;
                }
                Ok(())
            })
            .await?
            .map_err(io_error)?;
            *self = Self::Disk { file, len: *len };
        }
        let Self::Disk { file, len } = self else {
            unreachable!("a memory spool was just replaced")
        };
        let (file, at) = (file.clone(), *len);
        *len += bytes.len() as u64;
        cx.blocking(move || write_at(&file, &bytes, at))
            .await?
            .map_err(io_error)
    }

    async fn read(&self, range: Range<u64>, cx: &Cx) -> Result<Bytes> {
        let end = range.end.min(self.len());
        let start = range.start.min(end);
        match self {
            Self::Memory { chunks, .. } => {
                let first = chunks.partition_point(|(at, chunk)| at + chunk.len() as u64 <= start);
                let mut out = BytesMut::with_capacity((end - start) as usize);
                for (at, chunk) in &chunks[first..] {
                    if *at >= end {
                        break;
                    }
                    let from = start.saturating_sub(*at) as usize;
                    let to = (end - at).min(chunk.len() as u64) as usize;
                    out.extend_from_slice(&chunk[from..to]);
                }
                Ok(out.freeze())
            }
            Self::Disk { file, .. } => {
                let file = file.clone();
                cx.blocking(move || read_at(&file, start, (end - start) as usize))
                    .await?
                    .map_err(io_error)
            }
        }
    }
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], at: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, bytes, at)
}

#[cfg(windows)]
fn write_at(file: &File, mut bytes: &[u8], mut at: u64) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(file, bytes, at)?;
        bytes = &bytes[n..];
        at += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, at: u64, len: usize) -> std::io::Result<Bytes> {
    let mut buf = vec![0; len];
    std::os::unix::fs::FileExt::read_exact_at(file, &mut buf, at)?;
    Ok(buf.into())
}

#[cfg(windows)]
fn read_at(file: &File, at: u64, len: usize) -> std::io::Result<Bytes> {
    let mut buf = vec![0; len];
    let mut done = 0;
    while done < len {
        let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[done..], at + done as u64)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        done += n;
    }
    Ok(buf.into())
}
