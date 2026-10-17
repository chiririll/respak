//! The asset header, the CPU data and the studiohdr layout of a model variant.

use respak_core::{Error, Result, Version};

use super::block::reader;
use super::unsupported;

pub(super) const ASSET_HEADER_LEN: usize = 96;
pub(super) const CPU_LEN: usize = 16;
/// A studio block bigger than this is not a model.
pub(super) const MAX_STUDIO_LEN: u64 = 64 << 20;
/// pak `created_time` (FILETIME) from which rsx calls a 19.x model 19.3 ("07/29/2026").
pub(super) const CREATED_19_3: u64 = 0x01DD_1EED_32D6_C000;
/// pak `created_time` from which rsx calls a 19.x model 19.1 ("09/05/2025").
pub(super) const CREATED_19_1: u64 = 0x01DC_1DF8_05C2_8000;

/// Which studiohdr layout a model uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioLayout {
    /// `studiohdr_v16_t`, 224 bytes (asset version 16).
    V16,
    /// `studiohdr_v17_t`, 228 bytes (versions 17, 18, 19.0, 19.1).
    V17,
    /// `studiohdr_v19_2_t`, 244 bytes (versions 19.2, 19.3, 20).
    V19_2,
}

impl StudioLayout {
    /// The layout of a variant, `UnsupportedVersion` outside 16 to 20.
    pub fn for_version(v: Version) -> Result<Self> {
        match (v.major, v.minor) {
            (16, 0) => Ok(Self::V16),
            (17 | 18, 0) | (19, 0 | 1) => Ok(Self::V17),
            (19, 2 | 3) | (20, 0) => Ok(Self::V19_2),
            _ => Err(unsupported(v.to_string())),
        }
    }

    /// How far the fields after `boneStateCount` moved relative to `studiohdr_v17_t`.
    pub(super) fn shift(self) -> usize {
        if self == Self::V19_2 {
            16
        } else {
            0
        }
    }
}

/// The variant of a model: the minor of 19.x comes from the pak's creation time and the studiohdr's
/// `sourceFilenameOffset` (the u16 at studiohdr + 216, `None` if the block is too short).
pub fn variant(
    asset_version: u32,
    created_time: u64,
    source_filename_offset: Option<u16>,
) -> Result<Version> {
    match asset_version {
        16..=18 | 20 => Ok(Version::new(asset_version)),
        19 => {
            let minor = if created_time >= CREATED_19_3 {
                3
            } else if source_filename_offset == Some(244) {
                2
            } else if created_time >= CREATED_19_1 {
                1
            } else {
                0
            };
            Ok(Version::with_minor(19, minor))
        }
        other => Err(unsupported(other.to_string())),
    }
}
/// The decoded asset header and CPU data.
#[derive(Debug, Clone, PartialEq)]
pub struct RmdlHeader {
    /// The variant.
    pub version: Version,
    /// The studiohdr layout it uses.
    pub layout: StudioLayout,
    /// Where the studiohdr sits in `.mem`.
    pub studio_offset: u64,
    /// Bytes of the studio blob (`ModelAssetCPU_v16_t::dataSizeModel`).
    pub studio_len: u64,
    /// Bytes of the VG after baking (may be rounded up).
    pub streaming_size: u32,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
    /// The inline VG (`staticStreamingData`) as a `.mem` offset, `None` if null.
    pub static_data: Option<u64>,
    /// Bounding box (min, max).
    pub bounds: ([f32; 3], [f32; 3]),
    /// Where the animation rig guids are (`.mem` offset) and how many.
    pub anim_rigs: (Option<u64>, u32),
    /// Where the animation sequence guids are (`.mem` offset) and how many.
    pub anim_seqs: (Option<u64>, u32),
}

impl RmdlHeader {
    /// Parses a relocated 96-byte asset header (`asset_version` 16 to 20) and the 16-byte CPU
    /// data. `created_time` is the pak header's creation time and `source_filename_offset` the
    /// studiohdr's (both only matter for 19.x, see [`variant`]).
    pub fn parse(
        asset_version: u32,
        header: &[u8],
        cpu: &[u8],
        created_time: u64,
        source_filename_offset: Option<u16>,
    ) -> Result<Self> {
        let version = variant(asset_version, created_time, source_filename_offset)?;
        let layout = StudioLayout::for_version(version)?;
        if header.len() != ASSET_HEADER_LEN {
            return Err(Error::corrupt(format!(
                "rmdl header is {} bytes, expected {ASSET_HEADER_LEN}",
                header.len()
            )));
        }
        if cpu.len() != CPU_LEN {
            return Err(Error::corrupt(format!(
                "rmdl CPU data is {} bytes, expected {CPU_LEN}",
                cpu.len()
            )));
        }
        let (h, c) = (reader(header), reader(cpu));
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = h.u64(at)?;
            Ok((v != u64::MAX && v != 0).then_some(v))
        };
        let studio_offset =
            ptr(0x00)?.ok_or_else(|| Error::corrupt("rmdl has no studio data pointer"))?;
        let studio_len = u64::from(
            u32::try_from(c.i32(12)?).map_err(|_| Error::corrupt("rmdl: negative studio size"))?,
        );
        if studio_len == 0 || studio_len > MAX_STUDIO_LEN {
            return Err(Error::corrupt(format!(
                "rmdl studio block of {studio_len} bytes is not usable"
            )));
        }
        Ok(Self {
            version,
            layout,
            studio_offset,
            studio_len,
            streaming_size: h.u32(0x2C)?,
            name_offset: ptr(0x08)?,
            static_data: ptr(0x18)?,
            bounds: (h.floats(0x30)?, h.floats(0x3C)?),
            anim_rigs: (ptr(0x20)?, h.u32(0x28)?),
            anim_seqs: (ptr(0x50)?, u32::from(h.u16(0x4A)?)),
        })
    }
}
