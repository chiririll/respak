//! Writing outputs: path claims, bytes and streamed blobs.

use crate::error::{Error, ErrorKind, Result};
use crate::event::Event;

use super::Cx;

impl Cx {
    /// Claims `path` for the value with key `key`. `false`: the same key already wrote it.
    fn claim(&self, path: &str, key: &crate::value::ValueKey) -> Result<bool> {
        let mut written = self.written.lock().unwrap();
        match written.get(path) {
            Some(k) if *k == key.to_string() => Ok(false),
            Some(_) => Err(Error::new(ErrorKind::OutputConflict {
                path: path.to_owned(),
            })),
            None => {
                written.insert(path.to_owned(), key.to_string());
                Ok(true)
            }
        }
    }

    /// Writes `bytes` to `path` on the output operator on behalf of the value with key `key`.
    ///
    /// The first writer of a path wins. Writing the same key to the same path again is skipped
    /// (`Ok`, nothing written); a different key is `OutputConflict`. Returns whether the file
    /// was written.
    pub async fn write(
        &self,
        path: &str,
        key: &crate::value::ValueKey,
        bytes: bytes::Bytes,
    ) -> Result<bool> {
        if !self.claim(path, key)? {
            return Ok(false);
        }
        if let Err(e) = self.output.write(path, bytes).await {
            // Leave the path free for a retry.
            self.written.lock().unwrap().remove(path);
            return Err(e.into());
        }
        self.mediator.publish(Event::OutputWritten {
            path: path.to_owned(),
        });
        Ok(true)
    }

    /// Like [`Cx::write`], for a blob: reads it in chunks and streams them to the output, so a
    /// large (or computed, see [`LazyBlob`](crate::value::LazyBlob)) blob is never held whole.
    /// A skipped path (same key) reads nothing, so a lazy blob is not even computed. A failure
    /// part-way aborts the file and frees the path.
    pub async fn write_blob(
        &self,
        path: &str,
        key: &crate::value::ValueKey,
        blob: &dyn crate::capability::Blob,
    ) -> Result<bool> {
        if !self.claim(path, key)? {
            return Ok(false);
        }
        if let Err(e) = self.copy_blob(path, blob).await {
            self.written.lock().unwrap().remove(path);
            return Err(e);
        }
        self.mediator.publish(Event::OutputWritten {
            path: path.to_owned(),
        });
        Ok(true)
    }

    async fn copy_blob(&self, path: &str, blob: &dyn crate::capability::Blob) -> Result<()> {
        let mut writer = self.output.writer(path).await?;
        let copied = async {
            let mut at = 0;
            // No cancellation check here: a finished conversion is still written, as `write` does.
            // A computed blob stops by itself when the token fires while it is being produced.
            loop {
                let chunk = blob.read(at..at + crate::capability::CHUNK, self).await?;
                if chunk.is_empty() {
                    return Ok(());
                }
                at += chunk.len() as u64;
                writer.write(chunk).await?;
            }
        }
        .await;
        match copied {
            Ok(()) => {
                writer.close().await?;
                Ok(())
            }
            Err(e) => {
                let _ = writer.abort().await;
                Err(e)
            }
        }
    }
}
