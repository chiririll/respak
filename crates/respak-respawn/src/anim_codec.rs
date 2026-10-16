//! The per-bone animation encodings of `aseq` / `asqd`: bone flag arrays, RLE bone headers, value
//! tracks (`mstudioanimvalue_t` chains), `Quaternion64` and half-float `Vector48`. Pure functions
//! over byte slices; every read is bounded (`Corrupt`).
//!
//! Ported from rsx `src/game/rtech/utils/studio/studio_r5.{h,cpp}` (`ANIM_BONEFLAG_*`,
//! `mstudio_rle_anim_t`, `mstudioanim_valueptr_t`, `s_AnimSeekLUT`, `ExtractAnimValue`,
//! `CalcBonePosition/Quaternion/Scale`) and `src/core/math/compressedvector.h`. Layouts and the
//! lookup table are in `todo/formats/aseq.md`. Datapoint compression is not decoded (`aseq` rejects it).

use respak_core::{Error, ErrorKind, Result};

use crate::rmdl::B;

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

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
    let b = B(flags);
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
        let v = B(block).u16(at)?;
        let size = v & 0x1FFF;
        if size < 2 {
            return Err(corrupt(format!(
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
        let b = B(block);
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
        return Err(corrupt(format!(
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
    let b = B(block);
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
            return Err(corrupt(format!(
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
    let b = B(block);
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
        return Err(corrupt(format!(
            "animation bone data at {at:#x} is larger than its header says ({} bytes)",
            header.size
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(v: u16) -> [u8; 2] {
        v.to_le_bytes()
    }

    fn near(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-4, "{a:?} != {b:?}");
        }
    }

    /// Packs 6-bit fields little-endian into bytes.
    fn pack6(values: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; flag_array_size(values.len(), FlagWidth::Six)];
        for (i, v) in values.iter().enumerate() {
            let bit = 6 * i;
            let word = u32::from(*v) << (bit & 7);
            out[bit >> 3] |= word as u8;
            if let Some(next) = out.get_mut((bit >> 3) + 1) {
                *next |= (word >> 8) as u8;
            }
        }
        out
    }

    #[test]
    fn flag_arrays_4_bit_and_6_bit() {
        assert_eq!(flag_array_size(1, FlagWidth::Four), 2);
        assert_eq!(flag_array_size(5, FlagWidth::Four), 4);
        assert_eq!(flag_array_size(4, FlagWidth::Six), 4);
        assert_eq!(flag_array_size(5, FlagWidth::Six), 4);
        assert_eq!(flag_array_size(6, FlagWidth::Six), 6);
        assert_eq!(flag_array_size(0, FlagWidth::Six), 0);

        // five bones, nibbles low first; the 0x8 bit is kept (masked 0x0F)
        let four = [0x21, 0x43, 0x0F, 0];
        let got: Vec<_> = (0..5)
            .map(|i| bone_flags(&four, i, FlagWidth::Four).unwrap())
            .collect();
        assert_eq!(got, [1, 2, 3, 4, 0xF]);

        let six = pack6(&[1, 2, 4, 7, 0x3F]);
        let got: Vec<_> = (0..5)
            .map(|i| bone_flags(&six, i, FlagWidth::Six).unwrap())
            .collect();
        assert_eq!(got, [1, 2, 4, 7, 0x3F]);

        assert!(bone_flags(&four, 8, FlagWidth::Four).is_err());
        assert!(bone_flags(&six, 6, FlagWidth::Six).is_err());
    }

    #[test]
    fn rle_header_and_value_pointer() {
        let h = RleHeader::parse(&le(0x123 | (0b101 << 13)), 0).unwrap();
        assert_eq!((h.size, h.tracked), (0x123, 0b101));
        assert!(
            RleHeader::parse(&le(1), 0).is_err(),
            "size below the header itself"
        );
        assert!(RleHeader::parse(&[1], 0).is_err());

        // offset 12 bytes, X and Z present, second/third axis tracks 3 and 5 units on
        let mut ptr = le(12 | (0b101 << 13)).to_vec();
        ptr.extend([3, 5]);
        let mut block = vec![0u8; 4];
        block.extend(ptr);
        let p = ValuePtr::parse(&block, 4).unwrap();
        assert_eq!((p.base, p.axes, p.axis_offsets), (16, 0b101, [3, 5]));
        assert_eq!(p.track(0), Some(16));
        assert_eq!(p.track(1), None);
        assert_eq!(p.track(2), Some(16 + 10));
        assert_eq!(p.track(3), None);
    }

    #[test]
    fn seek_table_sizes() {
        assert_eq!(block_units(0, 5).unwrap(), 6);
        assert_eq!(block_units(1, 4).unwrap(), 4);
        assert_eq!(block_units(2, 200).unwrap(), 2);
        assert_eq!(block_units(3, 200).unwrap(), 3);
        assert_eq!(block_units(7, 9).unwrap(), 7);
        assert_eq!(
            block_units(8, 4).unwrap(),
            3,
            "2-bit residuals: 4 frames in one word"
        );
        assert_eq!(block_units(8, 16).unwrap(), 4);
        assert_eq!(
            block_units(14, 8).unwrap(),
            4,
            "4-bit residuals: 8 frames in two words"
        );
        assert_eq!(block_units(19, 4).unwrap(), 7 + 1);
        assert!(block_units(20, 1).is_err());
    }

    fn words(v: &[i16]) -> Vec<u8> {
        v.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn raw_tracks_and_block_seek() {
        // block A: valid 0, 3 frames (4 units); block B: valid 1, 4 frames
        let mut block = vec![0, 3];
        block.extend(words(&[10, -20, 30]));
        block.extend([1, 4]);
        block.extend(words(&[100]));
        block.extend([5u8, 0xFD, 2, 0]); // deltas +5, -3, +2
        assert_eq!(decode_track(&block, 0, 0, 1.0).unwrap(), 10.0);
        assert_eq!(decode_track(&block, 0, 2, 1.0).unwrap(), 30.0);
        assert_eq!(
            decode_track(&block, 0, 3, 1.0).unwrap(),
            100.0,
            "first frame of block B"
        );
        assert_eq!(decode_track(&block, 0, 4, 1.0).unwrap(), 105.0);
        assert_eq!(
            decode_track(&block, 0, 5, 2.0).unwrap(),
            194.0,
            "(100 - 3) * 2"
        );
        assert_eq!(decode_track(&block, 0, 6, 1.0).unwrap(), 102.0);
        // past the last block: the walk leaves the data
        assert!(decode_track(&block, 0, 7, 1.0).is_err());
        assert!(decode_track(&block, 0, 100, 1.0).is_err());
        assert!(
            decode_track(&block[..3], 0, 0, 1.0).is_err(),
            "truncated values"
        );
        assert!(
            decode_track(&[20, 4, 0, 0], 0, 0, 1.0).is_err(),
            "valid out of range"
        );
    }

    #[test]
    fn packed_residual_tracks() {
        // valid 8: base 10, no polynomial, 2-bit residuals 0,1,2,3 -> 10 - 3 + 2r
        let mut block = vec![8, 4];
        block.extend(words(&[10, 0b1110_0100]));
        let got: Vec<_> = (0..4)
            .map(|f| decode_track(&block, 0, f, 1.0).unwrap())
            .collect();
        assert_eq!(got, [7.0, 9.0, 11.0, 13.0]);

        // valid 14: 4-bit residuals, base 20 -> 20 - 15 + 2r; frames 0..2 residuals 15, 0, 7
        let mut block = vec![14, 3];
        block.extend(words(&[20, 0x07_0F]));
        let got: Vec<_> = (0..3)
            .map(|f| decode_track(&block, 0, f, 1.0).unwrap())
            .collect();
        assert_eq!(got, [35.0, 5.0, 19.0]);
    }

    #[test]
    fn polynomial_tracks() {
        // valid 3: base 100 + 40 * cycle, 5 frames; frame 2 is cycle 0.5
        let mut block = vec![3, 5];
        block.extend(words(&[100, 40]));
        assert_eq!(decode_track(&block, 0, 0, 1.0).unwrap(), 100.0);
        assert_eq!(decode_track(&block, 0, 2, 1.0).unwrap(), 120.0);
        assert_eq!(decode_track(&block, 0, 4, 0.5).unwrap(), 70.0);
        // valid 4: two terms, 100 + 40 c + 8 c^2 at c = 0.5
        let mut block = vec![4, 5];
        block.extend(words(&[100, 40, 8]));
        assert_eq!(decode_track(&block, 0, 2, 1.0).unwrap(), 122.0);
        // valid 9: one term and 2-bit residuals (bit index 1)
        let mut block = vec![9, 4];
        block.extend(words(&[100, 30, 0b11]));
        assert_eq!(decode_track(&block, 0, 0, 1.0).unwrap(), 100.0 - 3.0 + 6.0);
        // a single-frame block never divides by zero
        let mut block = vec![3, 1];
        block.extend(words(&[7, 9]));
        assert_eq!(decode_track(&block, 0, 0, 1.0).unwrap(), 7.0);
    }

    #[test]
    fn quaternion64_known_values() {
        let pack = |x: u64, y: u64, z: u64, neg: u64| x | (y << 21) | (z << 42) | (neg << 63);
        near(
            &unpack_quaternion64(pack(1_048_576, 1_048_576, 1_048_576, 0)),
            &[0.0, 0.0, 0.0, 1.0],
        );
        let q = unpack_quaternion64(pack(1_048_576 + 524_288, 1_048_576, 1_048_576, 0));
        near(&q, &[0.499_999_76, 0.0, 0.0, 0.866_025_5]);
        let q = unpack_quaternion64(pack(1_048_576, 1_048_576 - 524_288, 1_048_576, 1));
        near(&q, &[0.0, -0.499_999_76, 0.0, -0.866_025_5]);
        // a sum above one clamps w to zero instead of NaN
        let q = unpack_quaternion64(pack(0, 0, 0, 0));
        assert_eq!(q[3], 0.0);
    }

    #[test]
    fn half_floats() {
        let bytes = [0x00, 0x3C, 0x00, 0xC0, 0x00, 0x38];
        near(&half_vec3(&bytes), &[1.0, -2.0, 0.5]);
        near(
            &half_vec3(&[0, 0, 0x01, 0, 0xFF, 0x7B]),
            &[0.0, 5.960_464_5e-8, 65504.0],
        );
        assert!(half_vec3(&[0, 0x7C, 0, 0xFC, 1, 0x7C])
            .iter()
            .any(|v| v.is_nan()));
        assert_eq!(half_vec3(&[0, 0x7C, 0, 0, 0, 0])[0], f32::INFINITY);
    }

    #[test]
    fn euler_quaternion_round_trip() {
        near(&euler_to_quat([0.0; 3]), &[0.0, 0.0, 0.0, 1.0]);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        near(
            &euler_to_quat([0.0, 0.0, std::f32::consts::FRAC_PI_2]),
            &[0.0, 0.0, s, s],
        );
        near(
            &euler_to_quat([std::f32::consts::FRAC_PI_2, 0.0, 0.0]),
            &[s, 0.0, 0.0, s],
        );
        for angles in [[0.3, -0.5, 1.2], [-1.0, 0.7, -2.5], [0.0, 0.0, 3.0]] {
            near(&quat_to_euler(euler_to_quat(angles)), &angles);
        }
        let q = quat_mul(
            euler_to_quat([0.0, 0.0, 0.4]),
            euler_to_quat([0.0, 0.0, 0.5]),
        );
        near(&q, &euler_to_quat([0.0, 0.0, 0.9]));
    }

    /// A bone block: header (size, tracked) + `data`.
    fn bone_block(tracked: u8, data: &[u8]) -> Vec<u8> {
        let mut b = le((data.len() as u16 + 2) | (u16::from(tracked) << 13)).to_vec();
        b.extend(data);
        b
    }

    #[test]
    fn constant_channels() {
        let mut data = vec![0x00, 0x3C, 0x00, 0x40, 0x00, 0x42]; // (1, 2, 3)
        let q = 1_048_576u64 | (1_048_576 << 21) | (1_048_576 << 42);
        data.extend(q.to_le_bytes());
        data.extend([0x00, 0x3C, 0x00, 0x3C, 0x00, 0x3C]);
        let block = bone_block(0, &data);
        let header = RleHeader::parse(&block, 0).unwrap();
        let f = decode_bone(&block, 2, header, 7, 5, [0.0; 3]).unwrap();
        assert_eq!(f.translation, Some(([1.0, 2.0, 3.0], true)));
        assert_eq!(f.rotation.map(|r| r.1), Some(true));
        near(&f.rotation.unwrap().0, &[0.0, 0.0, 0.0, 1.0]);
        assert_eq!(f.scale, Some(([1.0; 3], true)));
        // only the channels the flags name are read
        let f = decode_bone(&block, 2, header, BONE_POS, 0, [0.0; 3]).unwrap();
        assert_eq!((f.rotation, f.scale), (None, None));
        // data larger than the header says
        let short = bone_block(0, &data[..8]);
        let header = RleHeader::parse(&short, 0).unwrap();
        assert!(decode_bone(&short, 2, header, 7, 0, [0.0; 3]).is_err());
    }

    #[test]
    fn tracked_rotation_uses_the_bind_pose_for_missing_axes() {
        // ValuePtr at 2: offset 4 (tracks follow it), X and Z present, Z track 4 units after X
        let mut data = le(4 | (0b101 << 13)).to_vec();
        data.extend([0, 4]);
        data.extend([0, 3]); // X: raw, 3 frames
        data.extend(words(&[100, 200, 300]));
        data.extend([0, 3]); // Z
        data.extend(words(&[-10, -20, -30]));
        let block = bone_block(TRACK_ROT, &data);
        let header = RleHeader::parse(&block, 0).unwrap();
        let f = decode_bone(&block, 2, header, BONE_ROT, 1, [0.0, 0.25, 0.0]).unwrap();
        let (q, constant) = f.rotation.unwrap();
        assert!(!constant);
        near(
            &q,
            &euler_to_quat([200.0 * ROT_SCALE, 0.25, -20.0 * ROT_SCALE]),
        );
        assert_eq!(f.translation, None);
    }

    #[test]
    fn tracked_position_values() {
        // posScale 0.5 at 2, ValuePtr at 6 (offset 4 -> tracks at 10), Z only
        let mut data = 0.5f32.to_le_bytes().to_vec();
        data.extend(le(4 | (0b001 << 13)));
        data.extend([0, 0]);
        data.extend([0, 2]);
        data.extend(words(&[4, 8]));
        let block = bone_block(TRACK_POS, &data);
        let header = RleHeader::parse(&block, 0).unwrap();
        let f = decode_bone(&block, 2, header, BONE_POS, 1, [0.0; 3]).unwrap();
        assert_eq!(f.translation, Some(([0.0, 0.0, 4.0], false)));
    }

    #[test]
    fn tracked_scale_values() {
        // ValuePtr at 2 (offset 4 -> tracks at 6), X only
        let mut data = le(4 | (0b100 << 13)).to_vec();
        data.extend([0, 0]);
        data.extend([0, 2]);
        data.extend(words(&[100, 200]));
        let block = bone_block(TRACK_SCALE, &data);
        let header = RleHeader::parse(&block, 0).unwrap();
        let f = decode_bone(&block, 2, header, BONE_SCALE, 0, [0.0; 3]).unwrap();
        let (s, constant) = f.scale.unwrap();
        assert!(!constant);
        near(&s, &[100.0 * SCALE_SCALE, 0.0, 0.0]);
    }

    #[test]
    fn sections_are_indexed_and_report_when_more_bytes_are_needed() {
        // 3 bones, 4-bit flags: bone 0 pos (const), bone 1 none, bone 2 scale (const)
        let mut block = vec![0x01, 0x04];
        block.extend(bone_block(0, &[0; 6]));
        block.extend(bone_block(0, &[0; 6]));
        let index = index_section(&block, 3, FlagWidth::Four).unwrap().unwrap();
        assert_eq!(index.flags, [1, 0, 4]);
        assert_eq!(index.headers, [Some(2), None, Some(10)]);
        assert_eq!(index.extent, block.len());
        assert_eq!(
            index_section(&block[..block.len() - 1], 3, FlagWidth::Four).unwrap(),
            None
        );
        assert_eq!(
            index_section(&block[..1], 3, FlagWidth::Four).unwrap(),
            None
        );
        // a header of size 0 cannot make progress
        let mut bad = vec![0x01, 0x00, 0, 0];
        bad.extend([0; 8]);
        assert!(index_section(&bad, 1, FlagWidth::Four).is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..2000 {
            let block: Vec<u8> = (0..64).map(|_| next() as u8).collect();
            let at = (next() % 70) as usize;
            let _ = decode_track(&block, at, (next() % 40) as u32, 1.0);
            let _ = index_section(&block, (next() % 12) as usize, FlagWidth::Six);
            let _ = index_section(&block, (next() % 12) as usize, FlagWidth::Four);
            if let Ok(h) = RleHeader::parse(&block, at) {
                let _ = decode_bone(
                    &block,
                    at + 2,
                    h,
                    (next() % 8) as u8,
                    (next() % 40) as u32,
                    [0.0; 3],
                );
            }
        }
    }
}
