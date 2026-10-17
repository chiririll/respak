//! The per-bone animation encodings of `aseq` / `asqd`: bone flag arrays, RLE bone headers, value
//! tracks (`mstudioanimvalue_t` chains), `Quaternion64` and half-float `Vector48`. Pure functions
//! over byte slices; every read is bounded (`Corrupt`).
//!
//! Ported from rsx `src/game/rtech/utils/studio/studio_r5.{h,cpp}` (`ANIM_BONEFLAG_*`,
//! `mstudio_rle_anim_t`, `mstudioanim_valueptr_t`, `s_AnimSeekLUT`, `ExtractAnimValue`,
//! `CalcBonePosition/Quaternion/Scale`) and `src/core/math/compressedvector.h`. Layouts and the
//! lookup table are in `docs/formats/aseq.md`. Datapoint compression is not decoded (`aseq` rejects it).

use respak_core::{Error, Result};

use crate::rmdl::reader;

/// `FIXED_ROT_SCALE`: radians per unit of a rotation track.
pub(crate) const ROT_SCALE: f32 = 0.000_191_753_45;
/// `FIXED_SCALE_SCALE`: scale per unit of a scale track.
pub(crate) const SCALE_SCALE: f32 = 0.003_051_851;

/// Width of one entry of the bone flag array: 4 bits up to model 19.2, 6 bits from 19.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlagWidth {
    /// Nibbles (aseq 12.1).
    Four,
    /// Six-bit fields (aseq 13, 14).
    Six,
}

/// Which channels of a bone have data (`ANIM_BONEFLAG_*`, the low three bits of a flag entry).
pub(crate) const BONE_POS: u8 = 0x1;
/// See [`BONE_POS`].
pub(crate) const BONE_ROT: u8 = 0x2;
/// See [`BONE_POS`].
pub(crate) const BONE_SCALE: u8 = 0x4;

/// Header bits of [`RleHeader::tracked`]: note the order is the reverse of the bone flags.
const TRACK_SCALE: u8 = 0x1;
const TRACK_ROT: u8 = 0x2;
const TRACK_POS: u8 = 0x4;

/// Bytes of the flag array for `bones` bones: `IALIGN2((width * bones + 7) / 8)`.
pub(crate) fn flag_array_size(bones: usize, width: FlagWidth) -> usize {
    let bits = match width {
        FlagWidth::Four => 4,
        FlagWidth::Six => 6,
    };
    (bits * bones).div_ceil(8).next_multiple_of(2)
}

/// The flag entry of `bone` in the array `flags` (little-endian; masked `0x0F` or `0x3F`).
pub(crate) fn bone_flags(flags: &[u8], bone: usize, width: FlagWidth) -> Result<u8> {
    let b = reader(flags);
    match width {
        FlagWidth::Four => Ok((b.u8(bone / 2)? >> (4 * (bone % 2))) & 0x0F),
        FlagWidth::Six => {
            // The field may straddle two bytes; the second one is absent only when the field
            // ends exactly at the end of the array.
            let bit = 6 * bone;
            let lo = u16::from(b.u8(bit >> 3)?);
            let hi = u16::from(flags.get((bit >> 3) + 1).copied().unwrap_or(0));
            Ok((((lo | (hi << 8)) >> (bit & 7)) & 0x3F) as u8)
        }
    }
}

/// The 2-byte header in front of a bone's data (`mstudio_rle_anim_t`): the distance to the next
/// bone's header and which channels are tracks (the others are constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RleHeader {
    /// Bytes from this header to the next one (header and data).
    pub size: u16,
    /// `0x1` scale, `0x2` rotation, `0x4` position is a track.
    pub tracked: u8,
}

impl RleHeader {
    /// Parses the header at `at`.
    pub(crate) fn parse(block: &[u8], at: usize) -> Result<Self> {
        let v = reader(block).u16(at)?;
        let size = v & 0x1FFF;
        if size < 2 {
            return Err(Error::corrupt(format!(
                "animation bone header at {at:#x} has size {size}"
            )));
        }
        Ok(Self {
            size,
            tracked: (v >> 13) as u8,
        })
    }
}

/// A pointer to up to three value tracks (`mstudioanim_valueptr_t`), one per axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValuePtr {
    /// Absolute block offset of the first track.
    pub base: usize,
    /// Which axes have a track: bit 2 X, bit 1 Y, bit 0 Z.
    pub axes: u8,
    /// Track offsets, in 2-byte units, of the second and third present axes.
    pub axis_offsets: [u8; 2],
}

impl ValuePtr {
    /// Parses the pointer at `at` (its offset is relative to `at`).
    pub(crate) fn parse(block: &[u8], at: usize) -> Result<Self> {
        let b = reader(block);
        let v = b.u16(at)?;
        Ok(Self {
            base: at + usize::from(v & 0x1FFF),
            axes: (v >> 13) as u8,
            axis_offsets: [b.u8(at + 2)?, b.u8(at + 3)?],
        })
    }

    /// Block offset of the track of axis `axis` (0 X, 1 Y, 2 Z), `None` if the axis has none.
    pub(crate) fn track(&self, axis: usize) -> Option<usize> {
        if axis > 2 || self.axes & (0x4 >> axis) == 0 {
            return None;
        }
        Some(match axis {
            0 => self.base,
            n => self.base + 2 * usize::from(self.axis_offsets[n - 1]),
        })
    }
}

/// `s_AnimSeekLUT`: `(base, add, per frame)` per `valid` value 0 to 19.
const SEEK_LUT: [u8; 60] = [
    1, 15, 16, 2, 7, 8, 2, 15, 0, 3, 15, 0, 4, 15, 0, 5, 15, 0, 6, 15, 0, 7, 15, 0, 2, 15, 2, 3,
    15, 2, 4, 15, 2, 5, 15, 2, 6, 15, 2, 7, 15, 2, 2, 15, 4, 3, 15, 4, 4, 15, 4, 5, 15, 4, 6, 15,
    4, 7, 15, 4,
];

/// Size of one block of a track, in 2-byte units (`GetAnimValueOffset`, `s_AnimSeekLUT`).
pub(crate) fn block_units(valid: u8, total: u8) -> Result<usize> {
    if valid > 19 {
        return Err(Error::corrupt(format!(
            "animation track block with valid = {valid}"
        )));
    }
    let [base, add, per] = [0, 1, 2].map(|i| usize::from(SEEK_LUT[usize::from(valid) * 3 + i]));
    Ok(base + ((add + usize::from(total) * per) >> 4))
}

/// The value of the track at block offset `at` on `frame`, times `scale`: walks the block chain,
/// then decodes by `valid` (raw `i16`, base plus signed bytes, polynomial plus 2/4-bit residuals;
/// `ExtractAnimValue`). A chain that leaves the block or never reaches the frame is `Corrupt`.
pub(crate) fn decode_track(block: &[u8], at: usize, frame: u32, scale: f32) -> Result<f32> {
    let b = reader(block);
    let (mut at, mut f) = (at, frame as usize);
    let (valid, total) = loop {
        let valid = b.u8(at)?;
        let total = b.u8(at + 1)?;
        if usize::from(total) > f {
            break (valid, total);
        }
        f -= usize::from(total);
        at += 2 * block_units(valid, total)?;
    };
    let base = || -> Result<f32> { Ok(f32::from(b.i16(at + 2)?)) };
    let value = match valid {
        0 => f32::from(b.i16(at + 2 + 2 * f)?),
        1 => {
            let mut v = base()?;
            if f > 0 {
                v += f32::from(b.u8(at + 4 + (f - 1))? as i8);
            }
            v
        }
        2..=19 => {
            let bit_index = usize::from(valid - 2) / 6;
            let polys = usize::from(valid - 2) % 6;
            let mut v = base()?;
            let cycle = if total > 1 {
                f as f32 / f32::from(total - 1)
            } else {
                0.0
            };
            let mut power = 1.0;
            for i in 0..polys {
                power *= cycle;
                v += f32::from(b.i16(at + 4 + 2 * i)?) * power;
            }
            if bit_index != 0 {
                let (bits, bias) = [(0, 0.0), (2, 3.0), (4, 15.0)][bit_index];
                v -= bias;
                let bit_offset = f * bits;
                let word = b.u16(at + 2 * (polys + 2) + 2 * (bit_offset >> 4))?;
                v += 2.0 * f32::from((word >> (bit_offset & 15)) & ((1 << bits) - 1));
            }
            v
        }
        _ => {
            return Err(Error::corrupt(format!(
                "animation track block with valid = {valid}"
            )))
        }
    };
    Ok(value * scale)
}

/// Unpacks a `Quaternion64` (x, y, z: 21 bits each, then the sign of w) into `[x, y, z, w]`.
pub(crate) fn unpack_quaternion64(packed: u64) -> [f32; 4] {
    let axis = |shift: u32| ((packed >> shift) & 0x1F_FFFF) as f32;
    let unit = |v: f32| (v - 1_048_576.0) * (1.0 / 1_048_576.5);
    let (x, y, z) = (unit(axis(0)), unit(axis(21)), unit(axis(42)));
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    [x, y, z, if packed >> 63 != 0 { -w } else { w }]
}

/// An IEEE half float.
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1F);
    let fraction = f32::from(bits & 0x3FF);
    match exponent {
        0 => sign * fraction * 2f32.powi(-24),
        31 if fraction == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + fraction / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Three IEEE half floats (`Vector48`), 6 bytes.
pub(crate) fn half_vec3(bytes: &[u8; 6]) -> [f32; 3] {
    [0, 1, 2].map(|i| half(u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])))
}

/// A quaternion `[x, y, z, w]` from Euler angles in radians (`AngleQuaternion`, Source order:
/// `[roll, pitch, yaw]`).
pub(crate) fn euler_to_quat(angles: [f32; 3]) -> [f32; 4] {
    let (sr, cr) = (angles[0] * 0.5).sin_cos();
    let (sp, cp) = (angles[1] * 0.5).sin_cos();
    let (sy, cy) = (angles[2] * 0.5).sin_cos();
    let (sr_cp, cr_sp) = (sr * cp, cr * sp);
    let (cr_cp, sr_sp) = (cr * cp, sr * sp);
    [
        sr_cp * cy - cr_sp * sy,
        cr_sp * cy + sr_cp * sy,
        cr_cp * sy - sr_sp * cy,
        cr_cp * cy + sr_sp * sy,
    ]
}

/// Euler angles `[roll, pitch, yaw]` in radians of a quaternion `[x, y, z, w]` (`QuaternionAngles`,
/// the inverse of [`euler_to_quat`]).
pub(crate) fn quat_to_euler(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let forward = [
        1.0 - 2.0 * y * y - 2.0 * z * z,
        2.0 * x * y + 2.0 * w * z,
        2.0 * x * z - 2.0 * w * y,
    ];
    let left = [
        2.0 * x * y - 2.0 * w * z,
        1.0 - 2.0 * x * x - 2.0 * z * z,
        2.0 * y * z + 2.0 * w * x,
    ];
    let up_z = 1.0 - 2.0 * x * x - 2.0 * y * y;
    let xy = (forward[0] * forward[0] + forward[1] * forward[1]).sqrt();
    let pitch = (-forward[2]).atan2(xy);
    if xy > 0.001 {
        [left[2].atan2(up_z), pitch, forward[1].atan2(forward[0])]
    } else {
        [0.0, pitch, (-left[0]).atan2(left[1])]
    }
}

/// The Hamilton product `a * b` of two quaternions `[x, y, z, w]`.
pub(crate) fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// One bone's decoded channels on one frame; `None` means the bone has no data for the channel
/// (the caller then uses the bind pose, or the identity for additive animations).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct BoneFrame {
    /// Position: a track adds to the start value, a constant replaces it, so it is returned as
    /// `(value, is_constant)`.
    pub translation: Option<([f32; 3], bool)>,
    /// Rotation `[x, y, z, w]` and whether it is a constant (not a track).
    pub rotation: Option<([f32; 4], bool)>,
    /// Scale, same `(value, is_constant)` rule as the position.
    pub scale: Option<([f32; 3], bool)>,
}

/// Where the bones of one section are: the flag entry of every bone and the offset of the header
/// of each bone that has data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectionIndex {
    /// The flag entry (`BONE_*`) of every bone.
    pub flags: Vec<u8>,
    /// Block offset of the [`RleHeader`] of every bone whose flags have a channel.
    pub headers: Vec<Option<usize>>,
    /// Bytes of the section: the flag array and all bone headers with their data.
    pub extent: usize,
}

/// Walks the flag array and the bone headers of a section of `bones` bones. `None` means `block`
/// ends before the section does (read more and try again); a structure that cannot be right is
/// `Corrupt`.
pub(crate) fn index_section(
    block: &[u8],
    bones: usize,
    width: FlagWidth,
) -> Result<Option<SectionIndex>> {
    let flag_len = flag_array_size(bones, width);
    let Some(array) = block.get(..flag_len) else {
        return Ok(None);
    };
    let (mut flags, mut headers, mut cursor) = (
        Vec::with_capacity(bones),
        Vec::with_capacity(bones),
        flag_len,
    );
    for bone in 0..bones {
        let f = bone_flags(array, bone, width)?;
        flags.push(f);
        if f & (BONE_POS | BONE_ROT | BONE_SCALE) == 0 {
            headers.push(None);
            continue;
        }
        if cursor + 2 > block.len() {
            return Ok(None);
        }
        let header = RleHeader::parse(block, cursor)?;
        headers.push(Some(cursor));
        cursor += usize::from(header.size);
        if cursor > block.len() {
            return Ok(None);
        }
    }
    Ok(Some(SectionIndex {
        flags,
        headers,
        extent: cursor,
    }))
}

/// Decodes the channels of the bone whose data starts at `at` (right after its [`RleHeader`]) on
/// `frame`. `default_euler` is the bone's bind rotation as Euler angles, used for the axes a
/// rotation track does not have.
pub(crate) fn decode_bone(
    block: &[u8],
    at: usize,
    header: RleHeader,
    flags: u8,
    frame: u32,
    default_euler: [f32; 3],
) -> Result<BoneFrame> {
    let b = reader(block);
    let end = (at + usize::from(header.size)).saturating_sub(2);
    let mut cursor = at;
    let mut out = BoneFrame::default();
    let track = |ptr: &ValuePtr, axis: usize, scale: f32| -> Result<Option<f32>> {
        ptr.track(axis)
            .map(|t| decode_track(block, t, frame, scale))
            .transpose()
    };

    if flags & BONE_POS != 0 {
        if header.tracked & TRACK_POS != 0 {
            let pos_scale = b.f32(cursor)?;
            let ptr = ValuePtr::parse(block, cursor + 4)?;
            cursor += 8;
            let mut v = [0.0; 3];
            for (axis, slot) in v.iter_mut().enumerate() {
                *slot = track(&ptr, axis, pos_scale)?.unwrap_or(0.0);
            }
            out.translation = Some((v, false));
        } else {
            let bytes: &[u8; 6] = b.get(cursor, 6)?.try_into().expect("6 bytes");
            cursor += 6;
            out.translation = Some((half_vec3(bytes), true));
        }
    }
    if flags & BONE_ROT != 0 {
        if header.tracked & TRACK_ROT != 0 {
            let ptr = ValuePtr::parse(block, cursor)?;
            cursor += 4;
            let mut angles = default_euler;
            for (axis, slot) in angles.iter_mut().enumerate() {
                if let Some(v) = track(&ptr, axis, ROT_SCALE)? {
                    *slot = v;
                }
            }
            out.rotation = Some((euler_to_quat(angles), false));
        } else {
            let packed = b.u64(cursor)?;
            cursor += 8;
            out.rotation = Some((unpack_quaternion64(packed), true));
        }
    }
    if flags & BONE_SCALE != 0 {
        if header.tracked & TRACK_SCALE != 0 {
            let ptr = ValuePtr::parse(block, cursor)?;
            cursor += 4;
            let mut v = [0.0; 3];
            for (axis, slot) in v.iter_mut().enumerate() {
                *slot = track(&ptr, axis, SCALE_SCALE)?.unwrap_or(0.0);
            }
            out.scale = Some((v, false));
        } else {
            let bytes: &[u8; 6] = b.get(cursor, 6)?.try_into().expect("6 bytes");
            cursor += 6;
            out.scale = Some((half_vec3(bytes), true));
        }
    }
    if cursor > end {
        return Err(Error::corrupt(format!(
            "animation bone data at {at:#x} is larger than its header says ({} bytes)",
            header.size
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
