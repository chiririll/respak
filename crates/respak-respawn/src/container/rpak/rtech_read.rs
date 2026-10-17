//! The progressive reader of an RTech-compressed body.

use bytes::Bytes;
use respak_core::{Error, Result};

use super::body::{check, Body, MIN_INPUT};
use crate::rtech;

/// How far an RTech body has been read and decoded.
#[derive(Default)]
pub(super) struct Progress {
    /// The start of the file (header included) read so far.
    pub(super) input: Vec<u8>,
    /// The decoded start of the body.
    pub(super) body: Bytes,
    /// `body` is the whole body.
    pub(super) done: bool,
}

impl Body {
    /// The decoded body, at least `upto` bytes of it (fewer only if the body is shorter).
    pub(super) async fn decoded(&self, upto: u64) -> Result<Bytes> {
        let Self::Rtech {
            op,
            path,
            start,
            header_len,
            len,
            file_len,
            state,
            cancel,
        } = self
        else {
            unreachable!("only compressed bodies are decoded")
        };
        let mut progress = state.lock().await;
        while (progress.body.len() as u64) < upto && !progress.done {
            check(cancel)?;
            // Decode a bit ahead, so a run of small reads does not decode again each time.
            let want = upto
                .max(2 * progress.body.len() as u64)
                .max(MIN_INPUT)
                .min(*len);
            let complete = progress.input.len() as u64 >= *file_len;
            let (input, header_len) = (std::mem::take(&mut progress.input), *header_len);
            let expected = *len;
            let (input, result) = tokio::task::spawn_blocking(move || {
                let result =
                    rtech::decompress_prefix(&input, complete, header_len, Some(expected), want);
                (input, result)
            })
            .await
            .map_err(|e| Error::custom("decompression task", e))?;
            progress.input = input;
            match result {
                Ok(prefix) => {
                    progress.done = prefix.complete;
                    progress.body = Bytes::from(prefix.body);
                }
                Err(rtech::RtechError::NeedInput { at_least }) => {
                    let have = progress.input.len() as u64;
                    let target = at_least.max(2 * have).max(MIN_INPUT).min(*file_len);
                    if target <= have {
                        return Err(Error::corrupt(
                            "rpak: the compressed stream runs past the end of the file",
                        ));
                    }
                    let more = op
                        .read_with(path)
                        .range(start + have..start + target)
                        .await?;
                    progress.input.extend_from_slice(&more.to_bytes());
                }
                Err(e) => return Err(Error::custom("rtech", e)),
            }
        }
        Ok(progress.body.clone())
    }
}
