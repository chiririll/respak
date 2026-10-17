//! `asrc`: one Miles audio source. Handed out by the `mbnk` volume, never read from a file.
//!
//! Opening a source opens the `.mstr` next to the bank by file-name convention, checks its
//! header, reads the codec header block and picks the codec from its first four bytes. Each
//! channel is a mono track: track `n` is channel `n`.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use opendal::Operator;
use respak_core::capability::{Audio, AudioChunk, AudioTrackInfo, Dependency, Metadata};
use respak_core::stream::blocking_stream;
use respak_core::{Cx, Data, Error, ErrorKind, FormatId, Result, ValueKey, Version};
use tokio::sync::OnceCell;

use crate::bcf1::{self, Bcf1Header};
use crate::mbnk::{Bank, SourceEntry};
use crate::mstr::{MstrHeader, MSTR_HEADER_LEN};
use crate::rada::RadaHeader;

/// Codec header of a source, from the first four bytes of its stream header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Bink Audio (`BCF1`).
    Bcf1(Bcf1Header),
    /// RAD Audio (`RADA`): header only.
    Rada(RadaHeader),
}

impl Codec {
    /// Picks the codec by the magic of the codec header block.
    pub fn parse(block: &[u8]) -> Result<Self> {
        if Bcf1Header::has_magic(block) {
            Ok(Self::Bcf1(Bcf1Header::parse(block)?))
        } else if RadaHeader::has_magic(block) {
            Ok(Self::Rada(RadaHeader::parse(block)?))
        } else {
            let id = block
                .get(..4)
                .map_or_else(|| "short".to_owned(), |b| format!("{b:02X?}"));
            Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "asrc".into(),
                version: format!("codec {id}"),
            }))
        }
    }

    /// Channels of the stream.
    pub fn channels(&self) -> usize {
        match self {
            Self::Bcf1(h) => usize::from(h.channels),
            Self::Rada(h) => usize::from(h.channels),
        }
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::Bcf1(h) => u32::from(h.sample_rate),
            Self::Rada(h) => h.sample_rate,
        }
    }
}

/// File name of the `.mstr` a source lives in (`general_stream_patch_2.mstr`).
pub fn stream_file_name(stem: &str, source: &SourceEntry) -> String {
    let kind = source.language.as_deref().unwrap_or("stream");
    if source.patch_idx == 0 {
        format!("{stem}_{kind}.mstr")
    } else {
        format!("{stem}_{kind}_patch_{}.mstr", source.patch_idx)
    }
}

/// A source with the place of its bytes.
#[derive(Debug)]
pub struct AsrcData {
    format: FormatId,
    key: ValueKey,
    /// Operator the bank (and so its `.mstr` files) lives on.
    host: Operator,
    /// Path of the `.mstr` on `host`.
    stream_path: String,
    entry: SourceEntry,
    stream: Stream,
}

/// What opening the `.mstr` gave.
#[derive(Debug)]
enum Stream {
    Ready {
        codec: Codec,
        /// Encoded data in the `.mstr`, absolute offsets.
        data: Range<u64>,
    },
    /// The `.mstr` is not there. Describing the source works; reading it is a
    /// `MissingDependency`.
    Missing,
}

/// `.mstr` headers read so far, by path (`None`: the file is not there). A bank has thousands of
/// sources in a few streams, so each header is read once per bank.
pub(crate) type StreamHeads = Mutex<HashMap<String, Arc<OnceCell<Option<MstrHeader>>>>>;

/// Reads `range` of the sibling file `path`; a missing file is a `MissingDependency` naming
/// `item`.
async fn read_sibling(host: &Operator, path: &str, range: Range<u64>, item: &str) -> Result<Bytes> {
    let read = async { Ok::<_, Error>(host.read_with(path).range(range).await?.to_bytes()) };
    read.await.map_err(|e| match e.kind() {
        ErrorKind::NotFound { .. } => Error::new(ErrorKind::MissingDependency {
            item: item.to_owned(),
            file: path.to_owned(),
        }),
        _ => e,
    })
}

impl AsrcData {
    /// Opens `source` of `bank` (whose file is in `dir` on `host`): reads the `.mstr` header and
    /// the codec header block. A missing `.mstr` is not an error here (the source has no tracks
    /// and reading it gives `MissingDependency`); a header that does not match the source
    /// (`buildTag`, language, patch) or a codec block of the wrong size → `Corrupt`; an unknown
    /// codec → `UnsupportedVersion`.
    pub(crate) async fn open(
        host: &Operator,
        dir: &str,
        bank: &Bank,
        source: &SourceEntry,
        key: ValueKey,
        version: Version,
        heads: &StreamHeads,
    ) -> Result<Self> {
        let stream_path = format!("{dir}{}", stream_file_name(bank.stem(), source));
        let missing = |key, stream_path| Self {
            format: FormatId::new("asrc").with_version(version),
            key,
            host: host.clone(),
            stream_path,
            entry: source.clone(),
            stream: Stream::Missing,
        };
        let cell = heads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(stream_path.clone())
            .or_default()
            .clone();
        let mstr = cell
            .get_or_try_init(|| async {
                match read_sibling(host, &stream_path, 0..MSTR_HEADER_LEN as u64, &source.name)
                    .await
                {
                    Ok(head) => MstrHeader::parse(&head).map(Some),
                    Err(e) if matches!(e.kind(), ErrorKind::MissingDependency { .. }) => Ok(None),
                    Err(e) => Err(e),
                }
            })
            .await?;
        let Some(mstr) = *mstr else {
            return Ok(missing(key, stream_path));
        };
        if mstr.build_tag != bank.build_tag()
            || mstr.language_idx != source.language_idx
            || mstr.patch_idx != i32::from(source.patch_idx)
        {
            return Err(Error::corrupt(format!(
                "{stream_path} does not belong to this bank or source (build tag {:#x} vs {:#x}, language {} vs {}, patch {} vs {})",
                mstr.build_tag,
                bank.build_tag(),
                mstr.language_idx,
                source.language_idx,
                mstr.patch_idx,
                source.patch_idx
            )));
        }
        let block = read_sibling(
            host,
            &stream_path,
            source.header_range.clone(),
            &source.name,
        )
        .await?;
        let codec = Codec::parse(&block)?;
        let total = match codec {
            Codec::Bcf1(h) => u64::from(h.file_size),
            Codec::Rada(h) => h.file_size,
        };
        let data_len = total.checked_sub(block.len() as u64).ok_or_else(|| {
            Error::corrupt(format!(
                "{}: codec stream of {total} bytes is shorter than its header",
                source.name
            ))
        })?;
        let start = u64::from(mstr.stream_data_offset) + source.data_offset;
        Ok(Self {
            format: FormatId::new("asrc").with_version(version),
            key,
            host: host.clone(),
            stream_path,
            entry: source.clone(),
            stream: Stream::Ready {
                codec,
                data: start..start + data_len,
            },
        })
    }

    /// The codec header, `None` when the `.mstr` is missing.
    pub fn codec(&self) -> Option<Codec> {
        match self.stream {
            Stream::Ready { codec, .. } => Some(codec),
            Stream::Missing => None,
        }
    }

    /// Path of the `.mstr` the source lives in.
    pub fn stream_path(&self) -> &str {
        &self.stream_path
    }
}

#[async_trait]
impl Audio for AsrcData {
    /// One mono track per channel. With the `.mstr` missing (the channel count is in it) one
    /// placeholder track at the bank's sample rate, so a conversion reaches `samples` and fails
    /// with `MissingDependency` instead of "has no audio".
    fn tracks(&self) -> Vec<AudioTrackInfo> {
        match &self.stream {
            Stream::Ready { codec, .. } => vec![
                AudioTrackInfo {
                    channels: 1,
                    sample_rate: codec.sample_rate()
                };
                codec.channels()
            ],
            Stream::Missing => vec![AudioTrackInfo {
                channels: 1,
                sample_rate: u32::from(self.entry.sample_rate),
            }],
        }
    }

    /// `BCF1`: the decoded channel, streamed block by block. `RADA`: `UnsupportedVersion`.
    async fn samples(&self, track: u32, cx: &Cx) -> Result<BoxStream<'static, Result<AudioChunk>>> {
        let Stream::Ready { codec, data } = &self.stream else {
            return Err(Error::new(ErrorKind::MissingDependency {
                item: self.entry.name.clone(),
                file: self.stream_path.clone(),
            }));
        };
        let channel = track as usize;
        if channel >= codec.channels() {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!(
                    "track {track} does not exist, the source has {} channel(s)",
                    codec.channels()
                ),
            }));
        }
        let header = match *codec {
            Codec::Bcf1(h) => h,
            Codec::Rada(h) => {
                return Err(Error::new(ErrorKind::UnsupportedVersion {
                    format: "rada".into(),
                    version: format!("RAD Audio v{}", h.version),
                }))
            }
        };
        let data = read_sibling(
            &self.host,
            &self.stream_path,
            data.clone(),
            &self.entry.name,
        )
        .await?;
        Ok(blocking_stream(cx, move |sink| {
            bcf1::decode_channel(&header, &data, channel, sink)
        }))
    }
}

impl Metadata for AsrcData {
    fn name(&self) -> Option<&str> {
        Some(&self.entry.name)
    }

    fn kind(&self) -> Option<&str> {
        Some("asrc")
    }

    fn guid(&self) -> Option<u64> {
        None
    }

    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for AsrcData {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn key(&self) -> &ValueKey {
        &self.key
    }

    fn as_audio(&self) -> Option<&dyn Audio> {
        Some(self)
    }

    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mbnk::fixtures::Fx;

    fn entry(language: Option<&str>, patch: u16) -> SourceEntry {
        let mut f = Fx::new("x");
        f.patch = patch;
        SourceEntry {
            name: f.name.into(),
            language: language.map(str::to_owned),
            language_idx: if language.is_some() { 0 } else { 0xFFFF },
            patch_idx: patch,
            sample_rate: 48000,
            sample_count: 1,
            header_range: 0..24,
            data_offset: 0,
        }
    }

    #[test]
    fn stream_file_names_follow_the_convention() {
        assert_eq!(
            stream_file_name("general", &entry(None, 0)),
            "general_stream.mstr"
        );
        assert_eq!(
            stream_file_name("general", &entry(None, 2)),
            "general_stream_patch_2.mstr"
        );
        assert_eq!(
            stream_file_name("general", &entry(Some("english"), 0)),
            "general_english.mstr"
        );
        assert_eq!(
            stream_file_name("general", &entry(Some("korean"), 1)),
            "general_korean_patch_1.mstr"
        );
    }

    #[test]
    fn the_codec_is_picked_by_its_magic() {
        let mut bcf1 = b"1FCB".to_vec();
        bcf1.extend([2, 2, 0x80, 0xBB]);
        bcf1.extend([0u8; 16]);
        assert!(matches!(Codec::parse(&bcf1).unwrap(), Codec::Bcf1(_)));
        let e = Codec::parse(b"OGGS....").unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "{e}"
        );
        assert!(Codec::parse(b"ab").is_err());
    }
}
