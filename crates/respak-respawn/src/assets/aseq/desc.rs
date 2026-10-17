use respak_core::reader::Reader;
use respak_core::{Error, ErrorKind, Result};

use crate::anim_codec::{self};
use crate::rmdl::{fix_offset, reader, FixedOffset};

use super::*;

/// Where a bone weight list comes from (`weightlistindex` of the seqdesc).
#[derive(Debug, Clone, PartialEq)]
pub enum Weights {
    /// The special value 1: every weight 0.
    Zero,
    /// The special value 3: every weight 1.
    One,
    /// One weight per bone.
    List(Vec<f32>),
}

/// The fields of `mstudioseqdesc_v18_t` the conversion uses.
#[derive(Debug, Clone, PartialEq)]
pub struct SeqDesc {
    /// The label (`szlabelindex`).
    pub label: String,
    /// `flags`; `0x20000` means the sequence has scale.
    pub flags: i32,
    /// The blend grid `groupsize[2]`; the blend count is their product.
    pub group_size: [u8; 2],
    /// Offsets of the animdescs, relative to the seqdesc (`FIX_OFFSET`ed).
    pub anim_offsets: Vec<u32>,
    /// The per-bone weights.
    pub weights: Weights,
    /// Guids of the sequences layered on this one (`mstudioautolayer_v8_t`, 0x20 bytes each).
    pub autolayers: Vec<u64>,
}

impl SeqDesc {
    /// Parses the seqdesc at `at` in `block` for a rig of `bone_count` bones.
    pub fn parse(block: &[u8], at: usize, bone_count: usize) -> Result<Self> {
        let b = reader(block);
        let label = b.cstr(b.fixed(at, at)?, 256)?;
        let flags = b.i32(at + 0x04)?;
        let group_size = [b.u8(at + 0x54)?, b.u8(at + 0x55)?];
        let blends = usize::from(group_size[0]) * usize::from(group_size[1]);
        if blends == 0 {
            return Err(Error::corrupt(format!(
                "aseq `{label}` has no animations (group size {group_size:?})"
            )));
        }
        let table = b.fixed(at, at + 0x2A)?;
        let anim_offsets = (0..blends)
            .map(|i| Ok(fix_offset(b.u16(table + 2 * i)?)))
            .collect::<Result<Vec<u32>>>()?;
        let weights = match b.u16(at + 0x52)? {
            0 | 3 => Weights::One,
            1 => Weights::Zero,
            _ => {
                let list = b.fixed(at, at + 0x52)?;
                Weights::List(
                    (0..bone_count)
                        .map(|i| b.f32(list + 4 * i))
                        .collect::<Result<_>>()?,
                )
            }
        };
        // Autolayers are only listed as dependencies, so a table that does not fit is not an error.
        let count = usize::from(b.u16(at + 0x4E)?).min(MAX_AUTOLAYERS);
        let layers = b.fixed(at, at + 0x50)?;
        let autolayers = (0..count)
            .map_while(|i| b.u64(layers + 0x20 * i).ok())
            .collect();
        Ok(Self {
            label,
            flags,
            group_size,
            anim_offsets,
            weights,
            autolayers,
        })
    }
}

/// How a section of an animdesc is stored (`Stall_2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionData {
    /// At this byte offset in the animdesc's `asqd` blob.
    Asqd(u32),
    /// At this byte offset in the sequence's `dataExtra`.
    Extra(u32),
}

/// The root motion of an animation (`mstudioframemovement_t`): offsets of tracks for x, y, z and
/// yaw per section of `section_frames` frames.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameMovement {
    /// Scale of each track (x, y, z, yaw in degrees).
    pub scale: [f32; 4],
    /// Frames per section, 0 if the animation is one section.
    pub section_frames: i32,
    /// Per section the block offset of each track, `None` if the axis has none.
    pub sections: Vec<[Option<usize>; 4]>,
}

impl FrameMovement {
    fn parse(b: &Reader, at: usize, frame_count: u32) -> Result<Self> {
        let scale = b.floats::<4>(at)?;
        let section_frames = b.i32(at + 0x10)?;
        let count = if section_frames > 0 {
            (frame_count.max(1) as usize - 1) / section_frames as usize + 1
        } else {
            1
        };
        if count > MAX_SECTIONS {
            return Err(Error::corrupt(format!(
                "frame movement with {count} sections"
            )));
        }
        let sections = (0..count)
            .map(|i| {
                let section = b.fixed(at, at + 0x14 + 2 * i)?;
                let mut tracks = [None; 4];
                for (k, track) in tracks.iter_mut().enumerate() {
                    let off = usize::from(b.u16(section + 2 * k)?);
                    *track = (off != 0).then_some(section + off);
                }
                Ok(tracks)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            scale,
            section_frames,
            sections,
        })
    }

    /// The accumulated movement on `frame`: position and yaw in degrees.
    pub(super) fn at(&self, block: &[u8], frame: u32) -> Result<([f32; 3], f32)> {
        let (section, local) = if self.section_frames > 0 {
            let sf = self.section_frames as u32;
            ((frame / sf) as usize, frame % sf)
        } else {
            (0, frame)
        };
        let tracks = self.sections.get(section).ok_or_else(|| {
            Error::corrupt(format!("frame movement has no section for frame {frame}"))
        })?;
        let mut v = [0.0; 4];
        for (k, track) in tracks.iter().enumerate() {
            if let Some(at) = track {
                v[k] = anim_codec::decode_track(block, *at, local, self.scale[k])?;
            }
        }
        Ok(([v[0], v[1], v[2]], v[3]))
    }
}

/// The fields of `mstudioanimdesc_v19_1_t` the conversion uses.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimDesc {
    /// The name (`sznameindex`), empty if none.
    pub name: String,
    /// Frames per second; outside `0..=2048` is `Corrupt`.
    pub fps: f32,
    /// `flags` (`0x1` looping, `0x4` delta, `0x20` all zeros, `0x20000` has data, `0x40000` frame
    /// movement, `0x200000` datapoint).
    pub flags: i32,
    /// Number of frames; outside `0..=0x20000` is `Corrupt`.
    pub frame_count: u32,
    /// Guid of the `asqd` with the data, 0 if none.
    pub asqd: u64,
    /// Frames of the first, "stall" section.
    pub stall_frames: u16,
    /// Frames of every later section, 0 if the animation is one section.
    pub section_frames: u16,
    /// The section table in order.
    pub sections: Vec<SectionData>,
    /// The root motion, if the animation has any.
    pub movement: Option<FrameMovement>,
}

impl AnimDesc {
    /// Parses the animdesc at `at` in `block` (offsets inside it are relative to `at`).
    pub fn parse(block: &[u8], at: usize) -> Result<Self> {
        let b = reader(block);
        let fps = b.f32(at)?;
        let flags = b.i32(at + 0x04)?;
        let frames = b.i32(at + 0x08)?;
        // rsx's `ANIMDESC_SANITY_CHECK`
        if !(0.0..=2048.0).contains(&fps) {
            return Err(Error::corrupt(format!(
                "animation with {fps} frames per second"
            )));
        }
        let frame_count = u32::try_from(frames)
            .ok()
            .filter(|&n| n <= 0x20000)
            .ok_or_else(|| Error::corrupt(format!("animation with {frames} frames")))?;
        let name = if b.u16(at + 0x0C)? == 0 {
            String::new()
        } else {
            b.cstr(b.fixed(at, at + 0x0C)?, 256)?
        };
        let asqd = b.u64(at + 0x18)?;
        let stall_frames = b.u16(at + 0x2C)?;
        let section_frames = b.u16(at + 0x2E)?;

        let mut sections = Vec::new();
        if section_frames != 0 && b.u16(at + 0x2A)? != 0 {
            let count = (frame_count as usize).saturating_sub(usize::from(stall_frames) + 1)
                / usize::from(section_frames)
                + 2;
            let table = b.fixed(at, at + 0x2A)?;
            // A table that ends early stays short; `locate` reports the missing entry when it is needed.
            for entry in (0..count.min(MAX_SECTIONS)).map_while(|i| b.i32(table + 4 * i).ok()) {
                sections.push(if entry < 0 {
                    SectionData::Extra(!entry as u32)
                } else {
                    SectionData::Asqd(entry as u32)
                });
            }
        }
        let movement = if flags & ANIM_FRAMEMOVEMENT != 0 && b.u16(at + 0x0E)? != 0 {
            Some(FrameMovement::parse(
                &b,
                b.fixed(at, at + 0x0E)?,
                frame_count,
            )?)
        } else {
            None
        };
        Ok(Self {
            name,
            fps,
            flags,
            frame_count,
            asqd,
            stall_frames,
            section_frames,
            sections,
            movement,
        })
    }

    /// Where the data of `frame` is and the section-local frame (`Stall_2`). Frames before `stall_frames` are held by section 0, which is the
    /// start of the `asqd` blob (`Asqd(0)`).
    pub fn locate(&self, frame: u32) -> Result<(SectionData, u32)> {
        if frame >= self.frame_count {
            return Err(Error::new(ErrorKind::NotFound {
                path: format!("frame {frame}"),
            }));
        }
        let (stall, sf) = (u32::from(self.stall_frames), u32::from(self.section_frames));
        if sf == 0 || self.frame_count <= 1 || frame < stall {
            return Ok((SectionData::Asqd(0), frame));
        }
        // rsx's `Stall_2` has no case for the last frame: it is found like any other. The trailing
        // section that RLE animations carry (counted in the section count) is never read.
        let base = (frame - stall) / sf;
        let (index, local) = (base, frame - stall - base * sf);
        self.sections
            .get(index as usize)
            .map(|&s| (s, local))
            .ok_or_else(|| {
                Error::corrupt(format!(
                    "animation `{}`: no section {index} for frame {frame} (the table has {})",
                    self.name,
                    self.sections.len()
                ))
            })
    }

    /// Whether the animation has RLE data to decode.
    pub(super) fn has_data(&self) -> bool {
        self.flags & ANIM_VALID != 0 && self.flags & ANIM_ALLZEROS == 0 && self.asqd != 0
    }
}
