//! The [`Skeleton`] capability.

/// One bone of a [`Skeleton`], in the bind pose, relative to its parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    /// The bone's name.
    pub name: String,
    /// Index of the parent in [`Skeleton::bones`], `None` for a root. A parent always comes before its children.
    pub parent: Option<u32>,
    /// Position relative to the parent.
    pub position: [f32; 3],
    /// Rotation relative to the parent, as a quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Scale relative to the parent.
    pub scale: [f32; 3],
}

/// A bone hierarchy.
pub trait Skeleton: Send + Sync {
    /// The bones, parents before children. A bone's index is what mesh bone weights refer to.
    fn bones(&self) -> &[Bone];
}
