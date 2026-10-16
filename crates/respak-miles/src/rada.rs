//! RAD Audio (`RADA`, Apex Season 24+): header only. Decoding is not possible without RAD's
//! closed library, so a source with this codec reports its shape and `samples()` gives
//! `UnsupportedVersion` (`todo/formats/miles-codecs.md`, "RADA").

use respak_core::detect::{magic, u32le};
use respak_core::Result;

use crate::corrupt;

/// Bytes of the `RadAFileHeader`.
pub const RADA_HEADER_LEN: usize = 40;

/// Most channels a RADA stream can have (`RADA_MAX_CHANNELS`).
const MAX_CHANNELS: u8 = 32;

/// The fields of the 40-byte `RadAFileHeader` this slice uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadaHeader {
    /// Header `version` byte.
    pub version: u8,
    /// Channel count (at most 32).
    pub channels: u8,
    /// Sample rate in Hz (24000, 32000, 44100 or 48000).
    pub sample_rate: u32,
    /// Frames per channel.
    pub frame_count: u64,
    /// Size of the whole codec stream.
    pub file_size: u64,
}

impl RadaHeader {
    /// Whether `header` starts with the `RADA` tag (`"ADAR"` on disk).
    pub fn has_magic(header: &[u8]) -> bool {
        magic(header, 0, b"ADAR")
    }

    /// Parses the header. A sample-rate enum of 4 or more, no channels or more than 32, or a
    /// short header → `Corrupt`.
    pub fn parse(header: &[u8]) -> Result<Self> {
        if header.len() < RADA_HEADER_LEN || !Self::has_magic(header) {
            return Err(corrupt("rada: not a RADA header"));
        }
        let channels = header[9];
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(corrupt(format!("rada: {channels} channels")));
        }
        let sample_rate = match header[0x0F] {
            0 => 24000,
            1 => 32000,
            2 => 44100,
            3 => 48000,
            other => return Err(corrupt(format!("rada: sample rate code {other}"))),
        };
        let u64le = |at: usize| {
            u64::from(u32le(header, at).expect("length checked"))
                | u64::from(u32le(header, at + 4).expect("length checked")) << 32
        };
        Ok(Self {
            version: header[8],
            channels,
            sample_rate,
            frame_count: u64le(0x10),
            file_size: u64le(0x18),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(rate_code: u8, channels: u8, frames: u64, size: u64) -> Vec<u8> {
        let mut h = vec![0u8; RADA_HEADER_LEN];
        h[..4].copy_from_slice(b"ADAR");
        h[8] = 1;
        h[9] = channels;
        h[0x0F] = rate_code;
        h[0x10..0x18].copy_from_slice(&frames.to_le_bytes());
        h[0x18..0x20].copy_from_slice(&size.to_le_bytes());
        h
    }

    #[test]
    fn parses_the_shape() {
        let h = RadaHeader::parse(&header(3, 2, 96_000, 5000)).unwrap();
        assert_eq!(
            h,
            RadaHeader {
                version: 1,
                channels: 2,
                sample_rate: 48000,
                frame_count: 96_000,
                file_size: 5000
            }
        );
        assert_eq!(
            RadaHeader::parse(&header(0, 1, 1, 1)).unwrap().sample_rate,
            24000
        );
    }

    #[test]
    fn bad_rate_or_channels_are_corrupt() {
        assert!(RadaHeader::parse(&header(4, 2, 1, 1)).is_err());
        assert!(RadaHeader::parse(&header(3, 0, 1, 1)).is_err());
        assert!(RadaHeader::parse(&header(3, 33, 1, 1)).is_err());
        assert!(RadaHeader::parse(&header(3, 32, 1, 1)).is_ok());
        assert!(RadaHeader::parse(&header(3, 2, 1, 1)[..39]).is_err());
    }
}
