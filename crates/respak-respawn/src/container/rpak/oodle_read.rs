//! The progressive reader of an Oodle-compressed body.

use std::collections::HashMap;
use std::ops::Range;

use bytes::Bytes;
use respak_core::{Error, Result};

use super::body::{check, Body, MIN_INPUT};
use crate::oodle::{self, OodleError, Scanner};

/// How far an Oodle body has been read and scanned, and the segments decoded so far.
#[derive(Default)]
pub(super) struct OodleProgress {
    /// The start of the compressed stream (after the pak header) read so far.
    pub(super) input: Vec<u8>,
    /// Where the segments start, as far as the input has been walked.
    pub(super) scan: Scanner,
    /// Decoded segments by index.
    pub(super) segments: HashMap<usize, Bytes>,
}

impl Body {
    /// The decoded bytes of an Oodle body in `range` (inside the body).
    pub(super) async fn oodle_read(&self, range: Range<u64>) -> Result<Bytes> {
        let Self::Oodle {
            op,
            path,
            start,
            len,
            stream_len,
            state,
            cancel,
        } = self
        else {
            unreachable!("only Oodle bodies are read this way")
        };
        if range.start > range.end || range.end > *len {
            return Err(Error::corrupt("read past the end of the decompressed pak"));
        }
        let (from, to) = (usize::try_from(range.start), usize::try_from(range.end));
        let (Ok(from), Ok(to), Ok(total)) = (from, to, usize::try_from(*len)) else {
            return Err(Error::corrupt("rpak: body too large"));
        };
        if from == to {
            return Ok(Bytes::new());
        }
        let mut progress = state.lock().await;
        let p = &mut *progress;
        // Walk the stream until the last segment needed is known, and where it ends.
        let segment_of =
            |p: &OodleProgress, at: usize| p.scan.segments.iter().rposition(|s| s.output <= at);
        let (first, last) = loop {
            check(cancel)?;
            if p.scan.scanned() >= to {
                let last = segment_of(p, to - 1)
                    .ok_or_else(|| Error::corrupt("rpak: no Oodle segment"))?;
                if p.scan.scanned() >= total || p.scan.segments.len() > last + 1 {
                    let first = segment_of(p, from)
                        .ok_or_else(|| Error::corrupt("rpak: no Oodle segment"))?;
                    break (first, last);
                }
            }
            // One block further once the range is covered, else up to its end.
            let upto = if p.scan.scanned() >= to {
                p.scan.scanned() + 1
            } else {
                to
            };
            match p.scan.advance(&p.input, total, upto) {
                Ok(()) => {}
                Err(OodleError::NeedInput { at_least }) => {
                    let have = p.input.len() as u64;
                    let target = (at_least as u64)
                        .max(2 * have)
                        .max(MIN_INPUT)
                        .min(*stream_len);
                    if target <= have {
                        return Err(Error::corrupt(
                            "rpak: the compressed stream runs past the end of the file",
                        ));
                    }
                    let more = op
                        .read_with(path)
                        .range(start + have..start + target)
                        .await?;
                    p.input.extend_from_slice(&more.to_bytes());
                }
                Err(e) => return Err(Error::custom("oodle", e)),
            }
        };
        for k in first..=last {
            if p.segments.contains_key(&k) {
                continue;
            }
            check(cancel)?;
            let seg = p.scan.segments[k];
            let (out_end, in_end) = match p.scan.segments.get(k + 1) {
                Some(next) => (next.output, next.input),
                None => (total, p.scan.position()),
            };
            let input = p.input[seg.input..in_end].to_vec();
            let decoded = tokio::task::spawn_blocking(move || {
                let mut out = vec![0u8; out_end - seg.output];
                oodle::decode_segment(&input, &mut out).map(|()| out)
            })
            .await
            .map_err(|e| Error::custom("decompression task", e))?
            .map_err(|e| Error::custom("oodle", e))?;
            p.segments.insert(k, Bytes::from(decoded));
        }
        let at = |k: usize| p.scan.segments[k].output;
        if first == last {
            let base = at(first);
            return Ok(p.segments[&first].slice(from - base..to - base));
        }
        let mut out = Vec::with_capacity(to - from);
        for k in first..=last {
            let seg = &p.segments[&k];
            let base = at(k);
            out.extend_from_slice(&seg[from.max(base) - base..(to - base).min(seg.len())]);
        }
        Ok(Bytes::from(out))
    }
}
