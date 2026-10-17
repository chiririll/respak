use respak_core::{Error, Result, Version};

use crate::anim_codec::FlagWidth;
use crate::rmdl::reader;

use super::{unsupported, CREATED_12_1, HEADER_LEN};

/// The variant of a sequence. 12.0 (before the pak creation time cut-off), 7 to 11, 9 and 15+ are
/// `UnsupportedVersion`.
pub fn variant(asset_version: u32, created_time: u64) -> Result<Version> {
    match asset_version {
        12 if created_time >= CREATED_12_1 => Ok(Version::with_minor(12, 1)),
        12 => Err(unsupported("12.0")),
        13 | 14 => Ok(Version::new(asset_version)),
        other => Err(unsupported(other.to_string())),
    }
}

/// The bone flag width of a variant (4 bits for 12.1, 6 for 13 and 14).
pub(crate) fn flag_width(version: Version) -> FlagWidth {
    if version.major >= 13 {
        FlagWidth::Six
    } else {
        FlagWidth::Four
    }
}

/// The decoded 64-byte asset header (`AnimSeqAssetHeader_v8_t`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AseqHeader {
    /// The variant.
    pub version: Version,
    /// Where the seqdesc, animdescs and section tables are in `.mem`.
    pub data_offset: u64,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
    /// Model guids (`.mem` offset, count).
    pub models: (Option<u64>, u32),
    /// Effect guids (`.mem` offset). The header stores no count for them (open, see `aseq.md`), so
    /// they are not listed as dependencies until a real file shows the layout.
    pub effects: Option<u64>,
    /// Settings guids (`.mem` offset, count).
    pub settings: (Option<u64>, u32),
    /// The external sections (`dataExtra`): `.mem` offset (`None` if null) and size in bytes.
    pub extra: (Option<u64>, u32),
}

impl AseqHeader {
    /// Parses a relocated 64-byte header of a supported variant.
    pub fn parse(version: Version, header: &[u8]) -> Result<Self> {
        if !(12..=14).contains(&version.major) {
            return Err(unsupported(version.to_string()));
        }
        if header.len() != HEADER_LEN {
            return Err(Error::corrupt(format!(
                "aseq header is {} bytes, expected {HEADER_LEN}",
                header.len()
            )));
        }
        let h = reader(header);
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = h.u64(at)?;
            Ok((v != u64::MAX && v != 0).then_some(v))
        };
        Ok(Self {
            version,
            data_offset: ptr(0x00)?.ok_or_else(|| Error::corrupt("aseq has no data pointer"))?,
            name_offset: ptr(0x08)?,
            models: (ptr(0x20)?, u32::from(h.u16(0x18)?)),
            effects: ptr(0x28)?,
            settings: (ptr(0x30)?, u32::from(h.u16(0x1A)?)),
            extra: (ptr(0x38)?, h.u32(0x1C)?),
        })
    }
}
