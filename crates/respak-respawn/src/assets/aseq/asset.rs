use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::{
    Animation, AnimationInfo, Bone, BoneSample, BoneTrack, Dependency, Metadata, Track,
};
use respak_core::{Cx, Data, Error, ErrorKind, FormatId, Result, Value, ValueKey};

use crate::anim_codec::{
    self, BoneFrame, FlagWidth, RleHeader, SectionIndex, BONE_POS, BONE_ROT, BONE_SCALE,
};
use crate::asqd::AsqdAsset;
use crate::guid::hex;
use crate::matl::locate;
use crate::mem::read_clamped;

use super::*;

/// The bytes of `dataExtra`, read in ranges.
#[derive(Debug)]
pub(super) struct Extra {
    pub(super) volume: Operator,
    /// `.mem` or the sequence's starpak entry.
    pub(super) path: String,
    pub(super) base: u64,
    pub(super) len: u64,
}

impl Extra {
    async fn read_upto(&self, offset: u64, max: u64) -> Result<Bytes> {
        read_clamped(
            &self.volume,
            &self.path,
            self.base.saturating_add(offset),
            max,
            self.base.saturating_add(self.len),
        )
        .await
    }
}

/// One decoded section: its bytes and where each bone is in them.
#[derive(Debug)]
pub(super) struct Section {
    pub(super) block: Bytes,
    pub(super) index: SectionIndex,
}

/// The channels of a bone on a frame; the flag says the channel is a track (not a constant).
pub(super) struct Channels {
    pub(super) translation: ([f32; 3], bool),
    pub(super) rotation: ([f32; 4], bool),
    pub(super) scale: ([f32; 3], bool),
}

/// A sequence: `Data` + `Animation` + `Metadata`. One animation per blend.
#[derive(Debug)]
pub struct AseqAsset {
    pub(super) format: FormatId,
    pub(super) key: ValueKey,
    pub(super) header: AseqHeader,
    pub(super) seq: SeqDesc,
    pub(super) anims: Vec<AnimDesc>,
    pub(super) name: Option<String>,
    pub(super) guid: u64,
    /// The bones of the bound skeleton, copied from the rig.
    pub(super) bones: Vec<Bone>,
    /// The bind rotation of each bone as Euler angles (for the axes a rotation track lacks).
    pub(super) default_euler: Vec<[f32; 3]>,
    /// Guid of the rig the bones came from.
    pub(super) rig: u64,
    /// Guids to report as dependencies: asqd per animdesc, autolayer aseq, models, settings.
    pub(super) dependencies: Vec<u64>,
    pub(super) origin: String,
    /// The description block (frame movement tracks are in it).
    pub(super) block: Bytes,
    pub(super) width: FlagWidth,
    pub(super) extra: Option<Extra>,
    pub(super) asqds: Mutex<HashMap<u64, Value>>,
    pub(super) sections: Mutex<HashMap<(u32, SectionData), Arc<Section>>>,
}

impl AseqAsset {
    /// The parsed header.
    pub fn header(&self) -> &AseqHeader {
        &self.header
    }

    /// The sequence description.
    pub fn seq(&self) -> &SeqDesc {
        &self.seq
    }

    /// The animdescs, one per blend.
    pub fn anims(&self) -> &[AnimDesc] {
        &self.anims
    }

    /// Guid of the rig the animation is bound to.
    pub fn rig(&self) -> u64 {
        self.rig
    }

    fn anim(&self, blend: u32) -> Result<&AnimDesc> {
        self.anims.get(blend as usize).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: format!("blend {blend}"),
            })
        })
    }

    fn bone(&self, bone: u32) -> Result<&Bone> {
        self.bones.get(bone as usize).ok_or_else(|| {
            Error::new(ErrorKind::NotFound {
                path: format!("bone {bone}"),
            })
        })
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| hex(self.guid))
    }

    /// The `asqd` with the guid `guid`, found through the mediator (and kept).
    async fn asqd(&self, guid: u64, cx: &Cx) -> Result<Value> {
        if let Some(v) = self.asqds.lock().expect("lock").get(&guid) {
            return Ok(v.clone());
        }
        let asset = locate(guid, "asqd", &self.origin, cx).await?;
        let value = cx
            .mediator
            .identify_as(asset, &FormatId::new("asqd"), cx)
            .await?;
        self.asqds.lock().expect("lock").insert(guid, value.clone());
        Ok(value)
    }

    /// Up to `want` bytes at the start of `section` of the animation `anim`.
    async fn window(
        &self,
        anim: &AnimDesc,
        section: SectionData,
        want: u64,
        cx: &Cx,
    ) -> Result<Bytes> {
        match section {
            SectionData::Asqd(offset) => {
                let value = self.asqd(anim.asqd, cx).await?;
                let asqd = value
                    .downcast_ref::<AsqdAsset>()
                    .ok_or_else(|| Error::corrupt(format!("{} is not an asqd", hex(anim.asqd))))?;
                asqd.read_upto(u64::from(offset), want).await
            }
            SectionData::Extra(offset) => {
                let extra = self.extra.as_ref().ok_or_else(|| {
                    Error::new(ErrorKind::MissingDependency {
                        item: self.label(),
                        file: "(the sequence has an external section but no dataExtra)".into(),
                    })
                })?;
                extra.read_upto(u64::from(offset), want).await
            }
        }
    }

    /// The decoded section `section` of `blend`, read once.
    async fn section(&self, blend: u32, section: SectionData, cx: &Cx) -> Result<Arc<Section>> {
        if let Some(s) = self.sections.lock().expect("lock").get(&(blend, section)) {
            return Ok(s.clone());
        }
        cx.check_cancelled()?;
        let anim = self.anim(blend)?;
        let mut want = SECTION_WINDOW;
        let decoded = loop {
            let bytes = self.window(anim, section, want, cx).await?;
            match anim_codec::index_section(&bytes, self.bones.len(), self.width)? {
                Some(index) => {
                    let keep = bytes.len().min(index.extent + TRACK_REACH);
                    break Section {
                        block: bytes.slice(..keep),
                        index,
                    };
                }
                None if (bytes.len() as u64) < want || want >= MAX_SECTION => {
                    return Err(Error::corrupt(format!(
                        "{}: section {section:?} of animation {blend} runs past the end of its data",
                        self.label()
                    )));
                }
                None => want *= 4,
            }
        };
        let decoded = Arc::new(decoded);
        self.sections
            .lock()
            .expect("lock")
            .insert((blend, section), decoded.clone());
        Ok(decoded)
    }

    /// The rest values of a bone: the bind pose, or the identity for an additive animation.
    fn rest(&self, bone: &Bone, additive: bool) -> Channels {
        if additive {
            Channels {
                translation: ([0.0; 3], false),
                rotation: ([0.0, 0.0, 0.0, 1.0], false),
                scale: ([1.0; 3], false),
            }
        } else {
            Channels {
                translation: (bone.position, false),
                rotation: (bone.rotation, false),
                scale: (bone.scale, false),
            }
        }
    }

    /// Whether the channels of `bone` in `anim` are all the rest pose.
    fn is_rest(anim: &AnimDesc, bone: u32) -> bool {
        !(anim.has_data() || bone == 0 && anim.movement.is_some())
    }

    /// The channels of `bone` on `frame` of `blend`.
    async fn channels(&self, blend: u32, bone: u32, frame: u32, cx: &Cx) -> Result<Channels> {
        let anim = self.anim(blend)?;
        let rest_bone = self.bone(bone)?;
        let additive = anim.flags & ANIM_DELTA != 0;
        let mut out = self.rest(rest_bone, additive);
        if anim.has_data() {
            if anim.flags & ANIM_DATAPOINT != 0 {
                return Err(unsupported("datapoint compression"));
            }
            let (place, local) = anim.locate(frame)?;
            let section = self.section(blend, place, cx).await?;
            let slot = bone as usize;
            if let Some(header_at) = section.index.headers[slot] {
                let header = RleHeader::parse(&section.block, header_at)?;
                let flags = section.index.flags[slot] & (BONE_POS | BONE_ROT | BONE_SCALE);
                let data = anim_codec::decode_bone(
                    &section.block,
                    header_at + 2,
                    header,
                    flags,
                    local,
                    self.default_euler[slot],
                )?;
                out = self.merge(rest_bone, additive, data);
            }
        }
        if self.seq.flags & ANIM_VALID == 0 {
            out.scale = ([1.0; 3], false);
        }
        if bone == 0 {
            if let Some(movement) = &anim.movement {
                let (position, yaw) = movement.at(&self.block, frame)?;
                out.translation.0 = add(out.translation.0, position);
                if yaw != 0.0 {
                    let half = yaw.to_radians() * 0.5;
                    out.rotation.0 =
                        anim_codec::quat_mul([0.0, 0.0, half.sin(), half.cos()], out.rotation.0);
                }
                out.translation.1 = true;
                out.rotation.1 |= yaw != 0.0;
            }
        }
        Ok(out)
    }

    /// Combines decoded data with the bind pose (a track adds to a position / scale, a constant replaces it).
    fn merge(&self, bone: &Bone, additive: bool, data: BoneFrame) -> Channels {
        let mut out = self.rest(bone, additive);
        if let Some((v, constant)) = data.translation {
            out.translation = if constant {
                (v, false)
            } else {
                (add(out.translation.0, v), true)
            };
        }
        if let Some((q, constant)) = data.rotation {
            out.rotation = (q, !constant);
        }
        if let Some((v, constant)) = data.scale {
            out.scale = if constant {
                (v, false)
            } else {
                (add(out.scale.0, v), true)
            };
        }
        out
    }
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// A channel over all frames: a constant if no frame was a track and all values are equal.
fn collapse<T: PartialEq + Copy>(frames: Vec<(T, bool)>) -> Track<T> {
    match frames.first() {
        Some(&(first, false)) if frames.iter().all(|&(v, tracked)| !tracked && v == first) => {
            Track::Constant(first)
        }
        _ => Track::Dense(frames.into_iter().map(|(v, _)| v).collect()),
    }
}

#[async_trait]
impl Animation for AseqAsset {
    fn blend_count(&self) -> u32 {
        self.anims.len() as u32
    }

    /// Name, fps, frame count, looping (`0x1`), additive (`0x4`) and scale (seqdesc `0x20000`).
    fn info(&self, blend: u32) -> Result<AnimationInfo> {
        let anim = self.anim(blend)?;
        let name = if anim.name.is_empty() {
            format!("{}_{blend}", self.seq.label)
        } else {
            anim.name.clone()
        };
        Ok(AnimationInfo {
            name,
            fps: anim.fps,
            frame_count: anim.frame_count,
            looping: anim.flags & ANIM_LOOPING != 0,
            additive: anim.flags & ANIM_DELTA != 0,
            has_scale: self.seq.flags & ANIM_VALID != 0,
        })
    }

    fn bones(&self) -> &[Bone] {
        &self.bones
    }

    fn bone_weights(&self) -> Vec<f32> {
        match &self.seq.weights {
            Weights::Zero => vec![0.0; self.bones.len()],
            Weights::One => Vec::new(),
            Weights::List(list) => list.clone(),
        }
    }

    /// Every frame of the bone: channels without data are `Constant` (bind pose, or identity for
    /// additive); bone 0 includes the frame movement.
    async fn track(&self, blend: u32, bone: u32, cx: &Cx) -> Result<BoneTrack> {
        let anim = self.anim(blend)?;
        let rest_bone = self.bone(bone)?;
        if anim.frame_count == 0 || Self::is_rest(anim, bone) {
            let rest = self.rest(rest_bone, anim.flags & ANIM_DELTA != 0);
            return Ok(BoneTrack {
                translation: Track::Constant(rest.translation.0),
                rotation: Track::Constant(rest.rotation.0),
                scale: Track::Constant(if self.seq.flags & ANIM_VALID != 0 {
                    rest.scale.0
                } else {
                    [1.0; 3]
                }),
            });
        }
        let n = anim.frame_count as usize;
        let (mut t, mut r, mut s) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        for frame in 0..anim.frame_count {
            if frame % 256 == 0 {
                cx.check_cancelled()?;
            }
            let c = self.channels(blend, bone, frame, cx).await?;
            t.push(c.translation);
            r.push(c.rotation);
            s.push(c.scale);
        }
        Ok(BoneTrack {
            translation: collapse(t),
            rotation: collapse(r),
            scale: collapse(s),
        })
    }

    async fn sample(&self, blend: u32, bone: u32, frame: u32, cx: &Cx) -> Result<BoneSample> {
        let anim = self.anim(blend)?;
        if frame >= anim.frame_count {
            return Err(Error::new(ErrorKind::NotFound {
                path: format!("frame {frame}"),
            }));
        }
        let c = self.channels(blend, bone, frame, cx).await?;
        Ok(BoneSample {
            translation: c.translation.0,
            rotation: c.rotation.0,
            scale: c.scale.0,
        })
    }
}

impl Metadata for AseqAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("aseq")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.dependencies
            .iter()
            .map(|&guid| Dependency { guid, name: None })
            .collect()
    }
}

impl Data for AseqAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_animation(&self) -> Option<&dyn Animation> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}
