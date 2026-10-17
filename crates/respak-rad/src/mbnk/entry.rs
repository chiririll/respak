//! One source of a bank, and the raw entry layouts it is read from.

use std::ops::Range;

use respak_core::reader::Reader;
use respak_core::{Error, Result};

/// One source of a bank, with what is needed to find its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceEntry {
    /// Name from the string table (the v48 name block for bank v48).
    pub name: String,
    /// Language name for a localised source.
    pub language: Option<String>,
    /// Language table index (`0xFFFF` = not localised).
    pub language_idx: u16,
    /// Patch index (`0` = the base `.mstr`).
    pub patch_idx: u16,
    /// Sample rate recorded in the bank (the codec header wins for decoding).
    pub sample_rate: u16,
    /// Frames recorded in the bank.
    pub sample_count: u64,
    /// Codec header block, absolute offsets in the `.mstr`.
    pub header_range: Range<u64>,
    /// Offset of the encoded data, relative to the `.mstr` header's `streamDataOffset`.
    pub data_offset: u64,
}

impl SourceEntry {
    /// The bytes of the `.asrc` entry in the volume: a few `key=value` lines. Audio is never in
    /// them.
    pub(super) fn descriptor(&self) -> String {
        format!(
            "name={}\nlanguage={}\npatch={}\nsample_rate={}\nsample_count={}\n",
            self.name,
            self.language.as_deref().unwrap_or(""),
            self.patch_idx,
            self.sample_rate,
            self.sample_count
        )
    }
}

/// The fields of one entry, whatever its layout.
pub(super) struct RawEntry {
    pub(super) name_offset: u64,
    pub(super) language: u16,
    pub(super) patch: u16,
    pub(super) sample_rate: u16,
    pub(super) sample_count: u64,
    pub(super) header_size: u32,
    pub(super) header_offset: u64,
    pub(super) data_offset: u64,
}

pub(super) fn read_entry(source: u8, e: Reader) -> Result<RawEntry> {
    Ok(match source {
        13 => RawEntry {
            name_offset: u64::from(e.u32(0x10)?),
            language: e.u16(0x50)?,
            patch: e.u16(0x52)?,
            sample_rate: e.u16(0x14)?,
            sample_count: u64::from(e.u32(0x34)?),
            header_size: e.u32(0x30)?,
            header_offset: e.u64(0x38)?,
            data_offset: e.u64(0x40)?,
        },
        28 => RawEntry {
            name_offset: u64::from(e.u32(0x10)?),
            language: e.u16(0x0C)?,
            patch: e.u16(0x0E)?,
            sample_rate: e.u16(0x14)?,
            sample_count: u64::from(e.u32(0x34)?),
            header_size: e.u32(0x30)?,
            header_offset: e.u64(0x38)?,
            data_offset: e.u64(0x40)?,
        },
        39 => RawEntry {
            name_offset: u64::from(e.u32(0)?),
            language: e.u16(4)?,
            patch: e.u16(6)?,
            sample_rate: e.u16(0x0C)?,
            sample_count: u64::from(e.u32(0x24)?),
            header_size: e.u32(0x20)?,
            header_offset: e.u64(0x28)?,
            data_offset: e.u64(0x30)?,
        },
        _ => RawEntry {
            name_offset: e.u64(0)?,
            language: e.u16(8)?,
            patch: e.u16(0x0A)?,
            sample_rate: e.u16(0x10)?,
            sample_count: e.u64(0x28)?,
            header_size: e.u32(0x24)?,
            header_offset: e.u64(0x30)?,
            data_offset: e.u64(0x38)?,
        },
    })
}

/// The NUL-terminated string at `off`.
pub(super) fn cstring(bytes: &[u8], off: u64) -> Result<String> {
    let tail = usize::try_from(off)
        .ok()
        .and_then(|o| bytes.get(o..))
        .ok_or_else(|| Error::corrupt(format!("mbnk: string offset {off} is past the end")))?;
    let end = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::corrupt("mbnk: unterminated string"))?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}
