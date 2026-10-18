//! The `rpak.asset -> subt` read edge.

use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use respak_core::reader::Reader;
use respak_core::{
    Cx, Detect, Detected, Error, ErrorKind, Probe, ReadHandler, Result, Value, ValueKey,
};

use super::{parse_color_tag, SubtAsset, SubtEntry, SubtHeader, ENTRY_LEN, WHITE};
use crate::guid::string_to_guid;
use crate::rpak::{detect_kind, rpak_asset, MEM};

/// Longest line taken, in bytes.
const LINE_MAX: u64 = 1 << 16;

/// Read edge `rpak.asset -> subt`.
pub struct SubtRead;

#[async_trait]
impl Detect for SubtRead {
    /// The asset kind is `subt`; any asset version other than 0 and 1 is `UnsupportedVersion`
    /// when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"subt"))
    }
}

/// The language of a `subtitles_<language>.rpak` pak at `key`, if the name hashes to `guid`.
fn language_of(key: &ValueKey, guid: u64) -> Option<String> {
    let key = key.to_string();
    let (pak, _) = key.rsplit_once('/')?;
    let file = pak.rsplit('/').next()?;
    let language = file.strip_prefix("subtitles_")?.strip_suffix(".rpak")?;
    (string_to_guid(&format!("subtitles/subtitles_{language}.rpak")) == guid)
        .then(|| language.to_owned())
}

#[async_trait]
impl ReadHandler for SubtRead {
    /// Parses the header, bounds-checks the slot array and every string offset against `.mem`,
    /// and splits the colour tags. Malformed tags and invalid UTF-8 are warnings, not errors.
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"subt")?;
        if asset.entry.version > 1 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "subt".into(),
                version: asset.entry.version.to_string(),
            }));
        }
        let header = SubtHeader::parse(&asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();

        let count = header.entry_count as usize;
        let slots = match header.entries {
            Some(at) if count > 0 => {
                let len = (count * ENTRY_LEN) as u64;
                if at.saturating_add(len) > mem_len {
                    return Err(Error::corrupt("subt: the slots are outside the pak's data"));
                }
                asset.read_mem(at, len).await?
            }
            _ => bytes::Bytes::new(),
        };
        let slots = Reader::new("subt slots", &slots);
        let mut lines = Vec::new();
        let mut farthest = 0u64;
        for i in 0..count {
            let hash = slots.u32(i * ENTRY_LEN)?;
            if hash == 0 {
                continue;
            }
            let offset = slots.i32(i * ENTRY_LEN + 4)?;
            let offset = u64::try_from(offset)
                .map_err(|_| Error::corrupt(format!("subt: line {hash:#x} has offset {offset}")))?;
            farthest = farthest.max(offset);
            lines.push((hash, offset));
        }

        let block = match header.strings {
            Some(at) if !lines.is_empty() => {
                if at.saturating_add(farthest) >= mem_len {
                    return Err(Error::corrupt("subt: a string is outside the pak's data"));
                }
                let end = (at + farthest + LINE_MAX).min(mem_len);
                asset.read_mem(at, end - at).await?
            }
            None if !lines.is_empty() => {
                return Err(Error::corrupt(
                    "subt: lines are listed but the string pointer is null",
                ));
            }
            _ => bytes::Bytes::new(),
        };

        let mut entries = Vec::with_capacity(lines.len());
        for (hash, offset) in lines {
            let tail = &block[offset as usize..];
            let end = tail.iter().position(|&b| b == 0).ok_or_else(|| {
                Error::corrupt(format!(
                    "subt: the string of line {hash:#x} is not terminated"
                ))
            })?;
            let raw = match std::str::from_utf8(&tail[..end]) {
                Ok(s) => Cow::Borrowed(s),
                Err(_) => {
                    cx.warn(format!("subt: line {hash:#x} is not valid UTF-8"));
                    String::from_utf8_lossy(&tail[..end])
                }
            };
            let (color, text) = parse_color_tag(&raw);
            if color.is_none() && raw.starts_with("<clr:") {
                cx.warn(format!("subt: line {hash:#x} has a malformed colour tag"));
            }
            entries.push(SubtEntry {
                hash,
                color: color.unwrap_or(WHITE),
                text: text.to_owned(),
            });
        }

        let guid = asset.entry.guid;
        Ok(Arc::new(SubtAsset::new(
            source.key().clone(),
            asset.entry.version,
            language_of(source.key(), guid),
            entries,
            guid,
        )))
    }
}
