//! `mstr`: a Miles stream file. Only the 20-byte header is read; the audio bytes are addressed
//! by the bank (`docs/formats/miles.md`, "MSTR header").

use async_trait::async_trait;
use respak_core::detect::{magic, u16le, u32le};
use respak_core::value::FileData;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

/// Bytes of the header.
pub const MSTR_HEADER_LEN: usize = 0x14;

/// `'CSTR'` as it lies on disk (the MSVC literal is stored reversed).
const MAGIC: &[u8] = b"RTSC";

/// `languageIdx` of the non-localised `_stream` file.
pub const NOT_LOCALISED: u16 = 0xFFFF;

/// The 20-byte header of a `.mstr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MstrHeader {
    /// `0xFFFF` for the non-localised `_stream` file, else the language table index.
    pub language_idx: u16,
    /// Base of every source's data in this file.
    pub stream_data_offset: u32,
    /// Patch index (`0` = the base file).
    pub patch_idx: i32,
    /// Must equal the bank's build tag.
    pub build_tag: u32,
}

impl MstrHeader {
    /// Whether `header` starts with the `CSTR` magic.
    pub fn has_magic(header: &[u8]) -> bool {
        magic(header, 0, MAGIC)
    }

    /// Parses the header. Wrong magic or too short → `Corrupt`; a version other than 2 →
    /// `UnsupportedVersion`.
    pub fn parse(header: &[u8]) -> Result<Self> {
        if header.len() < MSTR_HEADER_LEN || !Self::has_magic(header) {
            return Err(Error::corrupt("mstr: not a CSTR header"));
        }
        let version = u16le(header, 4).expect("length checked");
        if version != 2 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "mstr".into(),
                version: version.to_string(),
            }));
        }
        Ok(Self {
            language_idx: u16le(header, 6).expect("length checked"),
            stream_data_offset: u32le(header, 8).expect("length checked"),
            patch_idx: u32le(header, 0xC).expect("length checked") as i32,
            build_tag: u32le(header, 0x10).expect("length checked"),
        })
    }
}

/// An `mstr` with its header.
#[derive(Debug)]
pub struct MstrData {
    format: FormatId,
    key: ValueKey,
    /// The parsed header.
    pub header: MstrHeader,
}

impl Data for MstrData {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn key(&self) -> &ValueKey {
        &self.key
    }
}

/// Read edge `file -> mstr`.
pub struct MstrRead;

#[async_trait]
impl Detect for MstrRead {
    /// Magic `CSTR` = 10, version 2 = +2; another version is `UnsupportedVersion`.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !MstrHeader::has_magic(input.header) {
            return Ok(None);
        }
        MstrHeader::parse(input.header)?;
        Ok(Some(Detected {
            score: 12,
            version: Some(Version::new(2)),
        }))
    }

    fn header_len(&self) -> usize {
        MSTR_HEADER_LEN
    }
}

#[async_trait]
impl ReadHandler for MstrRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "mstr must be read from a file".into(),
            })
        })?;
        let start = file.range.as_ref().map_or(0, |r| r.start);
        let header = file
            .op
            .read_with(&file.path)
            .range(start..start + MSTR_HEADER_LEN as u64)
            .await?
            .to_bytes();
        let header = MstrHeader::parse(&header)?;
        Ok(std::sync::Arc::new(MstrData {
            format: FormatId::new("mstr").with_version(Version::new(2)),
            key: source.key().clone(),
            header,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header: `CSTR`, version, language, data offset, patch, build tag.
    pub(crate) fn header_bytes(
        version: u16,
        language: u16,
        data_offset: u32,
        patch: i32,
        build_tag: u32,
    ) -> Vec<u8> {
        let mut b = b"RTSC".to_vec();
        b.extend(version.to_le_bytes());
        b.extend(language.to_le_bytes());
        b.extend(data_offset.to_le_bytes());
        b.extend(patch.to_le_bytes());
        b.extend(build_tag.to_le_bytes());
        b
    }

    #[test]
    fn parses_the_header() {
        let h = MstrHeader::parse(&header_bytes(2, NOT_LOCALISED, 0x1000, 3, 0xABCD)).unwrap();
        assert_eq!(
            h,
            MstrHeader {
                language_idx: NOT_LOCALISED,
                stream_data_offset: 0x1000,
                patch_idx: 3,
                build_tag: 0xABCD
            }
        );
        assert_eq!(
            MstrHeader::parse(&header_bytes(2, 0, 0, 0, 0))
                .unwrap()
                .language_idx,
            0
        );
    }

    #[test]
    fn another_version_is_unsupported() {
        let e = MstrHeader::parse(&header_bytes(1, 0, 0, 0, 0)).unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "{e}"
        );
    }

    #[test]
    fn wrong_magic_or_short_is_corrupt_and_not_detected() {
        let mut b = header_bytes(2, 0, 0, 0, 0);
        b[0] = b'X';
        assert!(matches!(
            MstrHeader::parse(&b).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
        assert!(!MstrHeader::has_magic(&b));
        assert!(MstrHeader::parse(&b"RTSC\x02\x00"[..]).is_err());
    }
}
