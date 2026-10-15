//! The [`Animation`] capability.

use async_trait::async_trait;

use crate::capability::Bone;
use crate::error::Result;
use crate::Cx;

/// What one animation (one blend of a sequence) is, known without decoding any frame.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationInfo {
    /// A name for files and nodes (the animation's own, or the asset's plus the blend index).
    pub name: String,
    /// Frames per second.
    pub fps: f32,
    /// Number of frames; every [`Track::Dense`] has this many entries.
    pub frame_count: u32,
    /// Whether the animation loops.
    pub looping: bool,
    /// Whether the values are deltas added to the bind pose (`true`) or absolute local transforms.
    pub additive: bool,
    /// Whether the animation carries scale; if not, [`BoneTrack::scale`] is a constant 1.
    pub has_scale: bool,
}

/// One channel of a bone over time: a single value for every frame, or one value per frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Track<T> {
    /// The same value on every frame.
    Constant(T),
    /// One value per frame, `frame_count` of them.
    Dense(Vec<T>),
}

/// The three channels of one bone. A channel the animation has no data for is a
/// [`Track::Constant`] with the bind pose value (absolute) or the identity (additive).
#[derive(Debug, Clone, PartialEq)]
pub struct BoneTrack {
    /// Position relative to the parent.
    pub translation: Track<[f32; 3]>,
    /// Rotation relative to the parent, a quaternion `[x, y, z, w]`.
    pub rotation: Track<[f32; 4]>,
    /// Scale relative to the parent.
    pub scale: Track<[f32; 3]>,
}

/// One bone on one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneSample {
    /// Position relative to the parent.
    pub translation: [f32; 3],
    /// Rotation relative to the parent, a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Scale relative to the parent.
    pub scale: [f32; 3],
}

/// Skeletal animation: per-bone tracks over a [`Skeleton`](crate::capability::Skeleton)'s bones.
///
/// An animation value holds one or more **blends** (a sequence's blend grid; usually one), each
/// its own clip. Lazy like [`Mesh`](crate::capability::Mesh): [`AnimationInfo`] is cheap, the
/// tracks are decoded when asked for. Values are as the game stores them (no coordinate change);
/// root motion is already folded into bone 0.
#[async_trait]
pub trait Animation: Send + Sync {
    /// Number of blends; at least 1.
    fn blend_count(&self) -> u32;

    /// The shape of `blend` (`blend < blend_count()`, anything else is `NotFound`).
    fn info(&self, blend: u32) -> Result<AnimationInfo>;

    /// The bones the tracks are indexed by, parents before children (the bound skeleton).
    fn bones(&self) -> &[Bone];

    /// Per-bone weight in `0.0..=1.0` (how much of the animation applies to the bone), one per
    /// bone; empty when the animation has none (all 1).
    fn bone_weights(&self) -> Vec<f32>;

    /// All frames of one bone (`bone < bones().len()`, else `NotFound`).
    async fn track(&self, blend: u32, bone: u32, cx: &Cx) -> Result<BoneTrack>;

    /// One bone on one frame (`frame < frame_count`, else `NotFound`), without decoding the rest.
    async fn sample(&self, blend: u32, bone: u32, frame: u32, cx: &Cx) -> Result<BoneSample>;
}
