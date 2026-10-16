//! Bink Audio (`BCF1`): the container side. The header, the seek table and the frame framing
//! live here; the DCT decoder is the `chiririll/nihav` fork's `binkaud.rs` (Bink Audio 2,
//! more than 2 channels and a raw-block entry point are added there, added when the decoder
//! is wired in). See `todo/formats/miles-codecs.md`, "BCF1".

use nihav_rad::BinkAudioBlocks;
use respak_core::capability::AudioChunk;
use respak_core::detect::{magic, u16le, u32le};
use respak_core::{Error, ErrorKind, Result};

use crate::corrupt;

/// Most channels accepted. rsx sizes its per-stream arrays for 8 sub-streams but its decoder
/// holds 4, so more than 8 channels cannot be verified.
const MAX_CHANNELS: u8 = 8;

/// Sync word that starts every frame.
const SYNC: u16 = 0x9999;

/// Bytes of the fixed header.
pub const BCF1_HEADER_LEN: usize = 24;

/// The 24-byte `BinkFileHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bcf1Header {
    /// Header version (1 or 2).
    pub version: u8,
    /// Total channels.
    pub channels: u8,
    /// Sample rate in Hz.
    pub sample_rate: u16,
    /// Frames per channel.
    pub sample_count: u32,
    /// Low 16 bits: the largest frame payload; high 16 bits non-zero: Bink Audio 2 bitstream.
    pub max_comp_space: u32,
    /// The whole codec stream in bytes.
    pub file_size: u32,
    /// Entries of the seek table (v2: the low 16 bits only).
    pub seek_entries: u16,
}

impl Bcf1Header {
    /// Whether `header` starts with the `BCF1` magic (`"1FCB"` on disk).
    pub fn has_magic(header: &[u8]) -> bool {
        magic(header, 0, b"1FCB")
    }

    /// Parses the header. Wrong magic, no channels, more than 8 or a short header → `Corrupt`;
    /// version 0 or above 2 → `UnsupportedVersion`.
    pub fn parse(header: &[u8]) -> Result<Self> {
        if header.len() < BCF1_HEADER_LEN || !Self::has_magic(header) {
            return Err(corrupt("bcf1: not a BCF1 header"));
        }
        let version = header[4];
        if !(1..=2).contains(&version) {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "bcf1".into(),
                version: version.to_string(),
            }));
        }
        let channels = header[5];
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(corrupt(format!("bcf1: {channels} channels")));
        }
        let entries = u32le(header, 0x14).expect("length checked");
        // v1: the whole word is the count; v2: the low half (the high half is unknown).
        let seek_entries = if version == 1 {
            u16::try_from(entries).map_err(|_| corrupt(format!("bcf1: {entries} seek entries")))?
        } else {
            entries as u16
        };
        Ok(Self {
            version,
            channels,
            sample_rate: u16le(header, 6).expect("length checked"),
            sample_count: u32le(header, 8).expect("length checked"),
            max_comp_space: u32le(header, 0xC).expect("length checked"),
            file_size: u32le(header, 0x10).expect("length checked"),
            seek_entries,
        })
    }

    /// Whether the bitstream is Bink Audio 2 (7-bit quantiser indices, magnitudes then signs).
    pub fn is_bink_audio_2(&self) -> bool {
        self.max_comp_space >> 16 != 0
    }

    /// Bytes from the start of the stream to the first frame: `24 + 2 * seek_entries`.
    pub fn frames_offset(&self) -> usize {
        BCF1_HEADER_LEN + 2 * usize::from(self.seek_entries)
    }

    /// Sub-streams of two channels (the last holds one when the count is odd).
    pub fn stream_count(&self) -> usize {
        usize::from(self.channels).div_ceil(2)
    }

    /// Samples per channel one block emits: 480, 960 or 1920 by sample rate.
    pub fn samples_per_block(&self) -> usize {
        match self.sample_rate {
            44100.. => 1920,
            22050..=44099 => 960,
            _ => 480,
        }
    }

    /// The largest frame payload in bytes (the low 16 bits of `max_comp_space`).
    pub fn max_payload(&self) -> usize {
        (self.max_comp_space & 0xFFFF) as usize
    }
}

/// One frame of the stream: `0x9999`, a size, the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bcf1Frame<'a> {
    /// Encoded payload.
    pub payload: &'a [u8],
    /// Frames per channel to keep (the end trim; set only in the `0xFFFF` form).
    pub valid_samples: Option<u16>,
}

impl<'a> Bcf1Frame<'a> {
    /// Splits the next frame off `buf`, returning it and the bytes consumed. A wrong sync word,
    /// a truncated frame or a size over `max_comp_space & 0xFFFF` → `Corrupt`; `None` at the end
    /// of `buf`.
    pub fn parse(buf: &'a [u8], header: &Bcf1Header) -> Result<Option<(Self, usize)>> {
        if buf.is_empty() {
            return Ok(None);
        }
        if u16le(buf, 0) != Some(SYNC) {
            return Err(corrupt("bcf1: frame without sync word"));
        }
        let truncated = || corrupt("bcf1: truncated frame header");
        let short = u16le(buf, 2).ok_or_else(truncated)?;
        let (size, valid_samples, start) = if short == 0xFFFF {
            let size = u16le(buf, 4).ok_or_else(truncated)?;
            let valid = u16le(buf, 6).ok_or_else(truncated)?;
            (usize::from(size), Some(valid), 8)
        } else {
            (usize::from(short), None, 4)
        };
        if size > header.max_payload() {
            return Err(corrupt(format!(
                "bcf1: frame of {size} bytes, the header allows {}",
                header.max_payload()
            )));
        }
        let payload = buf
            .get(start..start + size)
            .ok_or_else(|| corrupt("bcf1: truncated frame"))?;
        Ok(Some((
            Self {
                payload,
                valid_samples,
            },
            start + size,
        )))
    }
}

/// Decodes the frames in `data` (the data block of a source: frames only, the header block and
/// seek table are elsewhere) and hands the samples of `channel` to `sink`, one chunk per frame,
/// trimmed to the header's frame count. The sink returns `false` to stop. A damaged frame ends
/// the stream with one `Err`, after the chunks before it.
pub(crate) fn decode_channel(
    header: &Bcf1Header,
    data: &[u8],
    channel: usize,
    sink: &mut dyn FnMut(Result<AudioChunk>) -> bool,
) {
    let backend = |e: &dyn std::fmt::Debug| corrupt(format!("bcf1: {e:?}"));
    let mut decoder = match BinkAudioBlocks::new(
        u32::from(header.sample_rate),
        usize::from(header.channels),
        header.is_bink_audio_2(),
    ) {
        Ok(d) => d,
        Err(e) => {
            sink(Err(backend(&e)));
            return;
        }
    };
    let (mut pos, mut emitted, total) = (0, 0u64, u64::from(header.sample_count));
    while emitted < total {
        let (frame, used) = match Bcf1Frame::parse(&data[pos..], header) {
            Ok(Some(f)) => f,
            Ok(None) => return,
            Err(e) => {
                sink(Err(e));
                return;
            }
        };
        pos += used;
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decoder.decode(frame.payload)
        }));
        let planes = match decoded {
            Ok(Ok(planes)) => planes,
            Ok(Err(e)) => {
                sink(Err(backend(&e)));
                return;
            }
            Err(_) => {
                sink(Err(corrupt("bcf1: the decoder panicked")));
                return;
            }
        };
        let keep = frame.valid_samples.map_or(usize::MAX, usize::from);
        let n = planes[channel]
            .len()
            .min(keep)
            .min((total - emitted) as usize);
        emitted += n as u64;
        if !sink(Ok(AudioChunk {
            samples: planes[channel][..n].to_vec(),
        })) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(version: u8, channels: u8, rate: u16, max_comp: u32, entries: u32) -> Vec<u8> {
        let mut b = b"1FCB".to_vec();
        b.push(version);
        b.push(channels);
        b.extend(rate.to_le_bytes());
        b.extend(1000u32.to_le_bytes());
        b.extend(max_comp.to_le_bytes());
        b.extend(9999u32.to_le_bytes());
        b.extend(entries.to_le_bytes());
        b
    }

    #[test]
    fn parses_the_header_and_derives_sizes() {
        let h = Bcf1Header::parse(&header_bytes(2, 2, 48000, 0x400, 3)).unwrap();
        assert_eq!(
            (
                h.version,
                h.channels,
                h.sample_rate,
                h.sample_count,
                h.file_size,
                h.seek_entries
            ),
            (2, 2, 48000, 1000, 9999, 3)
        );
        assert_eq!(
            (
                h.max_payload(),
                h.frames_offset(),
                h.stream_count(),
                h.samples_per_block()
            ),
            (0x400, 24 + 6, 1, 1920)
        );
        assert!(!h.is_bink_audio_2());
        let rate = |r| {
            Bcf1Header::parse(&header_bytes(1, 1, r, 1, 0))
                .unwrap()
                .samples_per_block()
        };
        assert_eq!((rate(44100), rate(22050), rate(16000)), (1920, 960, 480));
        assert_eq!(
            Bcf1Header::parse(&header_bytes(1, 3, 48000, 1, 0))
                .unwrap()
                .stream_count(),
            2
        );
    }

    #[test]
    fn the_v2_high_word_is_not_a_count_and_flags_bink_audio_2() {
        let h = Bcf1Header::parse(&header_bytes(2, 2, 48000, 0x0001_0400, 0x0007_0002)).unwrap();
        assert_eq!(h.seek_entries, 2);
        assert!(h.is_bink_audio_2());
        assert_eq!(h.max_payload(), 0x400);
    }

    #[test]
    fn bad_headers() {
        let e = Bcf1Header::parse(&header_bytes(3, 2, 48000, 1, 0)).unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "{e}"
        );
        assert!(matches!(
            Bcf1Header::parse(&header_bytes(0, 2, 48000, 1, 0))
                .unwrap_err()
                .kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        assert!(Bcf1Header::parse(&header_bytes(1, 0, 48000, 1, 0)).is_err());
        assert!(Bcf1Header::parse(&header_bytes(1, 9, 48000, 1, 0)).is_err());
        assert!(Bcf1Header::parse(&header_bytes(1, 2, 48000, 1, 70_000)).is_err());
        assert!(Bcf1Header::parse(&header_bytes(1, 2, 48000, 1, 0)[..23]).is_err());
        let mut b = header_bytes(1, 2, 48000, 1, 0);
        b[0] = b'X';
        assert!(!Bcf1Header::has_magic(&b));
        assert!(Bcf1Header::parse(&b).is_err());
    }

    fn header() -> Bcf1Header {
        Bcf1Header::parse(&header_bytes(2, 2, 48000, 8, 0)).unwrap()
    }

    #[test]
    fn frames_in_both_forms() {
        let mut buf = vec![0x99, 0x99, 3, 0, 1, 2, 3];
        buf.extend([0x99, 0x99, 0xFF, 0xFF, 2, 0, 100, 0, 7, 8]);
        let (a, used) = Bcf1Frame::parse(&buf, &header()).unwrap().unwrap();
        assert_eq!(
            (a.payload, a.valid_samples, used),
            (&[1u8, 2, 3][..], None, 7)
        );
        let (b, used2) = Bcf1Frame::parse(&buf[used..], &header()).unwrap().unwrap();
        assert_eq!(
            (b.payload, b.valid_samples, used2),
            (&[7u8, 8][..], Some(100), 10)
        );
        assert!(Bcf1Frame::parse(&[], &header()).unwrap().is_none());
    }

    #[test]
    fn damaged_frames_are_corrupt() {
        let h = header();
        assert!(
            Bcf1Frame::parse(&[0x12, 0x34, 1, 0, 0], &h).is_err(),
            "no sync"
        );
        assert!(
            Bcf1Frame::parse(&[0x99, 0x99, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &h).is_err(),
            "over max_comp"
        );
        assert!(
            Bcf1Frame::parse(&[0x99, 0x99, 5, 0, 1, 2], &h).is_err(),
            "truncated payload"
        );
        assert!(
            Bcf1Frame::parse(&[0x99, 0x99, 0xFF, 0xFF, 1], &h).is_err(),
            "truncated header"
        );
        assert!(Bcf1Frame::parse(&[0x99], &h).is_err());
    }
}
