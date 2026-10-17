//! Options of the `cast` target.

use respak_core::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};

/// Which LODs to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LodSelect {
    /// Every LOD of the mesh (`"all"`).
    All(AllLods),
    /// One LOD (`1`).
    One(u32),
    /// A list of LODs (`[0, 2]`).
    List(Vec<u32>),
}

/// The word `"all"` in [`LodSelect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AllLods {
    /// Every LOD.
    All,
}

impl Default for LodSelect {
    fn default() -> Self {
        Self::One(0)
    }
}

impl LodSelect {
    /// The LODs to write for a mesh with `lod_count` LODs, ascending and without repeats.
    /// A LOD the mesh does not have is `InvalidOptions`.
    pub fn resolve(&self, lod_count: u32) -> Result<Vec<u32>> {
        let mut lods = match self {
            Self::All(_) => (0..lod_count).collect(),
            Self::One(n) => vec![*n],
            Self::List(l) => l.clone(),
        };
        lods.sort_unstable();
        lods.dedup();
        if let Some(bad) = lods.iter().find(|&&l| l >= lod_count) {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!("lod {bad}: the model has {lod_count} LOD(s)"),
            }));
        }
        Ok(lods)
    }
}

/// Whether the root bone of an animation gets rsx's coordinate fix-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootFixup {
    /// As rsx writes it: bone 0 turned by -90 degrees of yaw, position `(x, y)` to `(y, -x)`.
    #[default]
    Rsx,
    /// Bone 0 as the animation stores it.
    None,
}

/// Which blends of an animation to write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BlendSelect {
    /// Every blend (`"all"`).
    All(AllBlends),
    /// One blend (`1`).
    One(u32),
}

/// The word `"all"` in [`BlendSelect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AllBlends {
    /// Every blend.
    All,
}

impl Default for BlendSelect {
    fn default() -> Self {
        Self::All(AllBlends::All)
    }
}

impl BlendSelect {
    /// The blends to write for an animation with `blend_count` blends, ascending. A blend the
    /// animation does not have is `InvalidOptions`.
    pub fn resolve(&self, blend_count: u32) -> Result<Vec<u32>> {
        match self {
            Self::All(_) => Ok((0..blend_count).collect()),
            Self::One(n) if *n < blend_count => Ok(vec![*n]),
            Self::One(n) => Err(Error::new(ErrorKind::InvalidOptions {
                message: format!("blend {n}: the animation has {blend_count} blend(s)"),
            })),
        }
    }
}

/// Options of the `cast` target: `{"type":"cast","lods":[0,1],"skin":2,"textures":true,"image":"png"}`
/// for a mesh, `{"type":"cast","root_fixup":"none","blend":0}` for an animation (each side ignores
/// the other's options).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CastOptions {
    /// LODs to write; default `0`. A LOD the mesh does not have is `InvalidOptions`.
    pub lods: LodSelect,
    /// Skin family whose materials are used; default 0.
    pub skin: u32,
    /// Whether the materials' textures are written under `textures/`; default true.
    pub textures: bool,
    /// Format the textures are converted to; default `png`.
    pub image: String,
    /// Animations: whether bone 0 gets rsx's coordinate fix-up; default `"rsx"`.
    pub root_fixup: RootFixup,
    /// Animations: which blends to write; default `"all"`. A blend the animation does not have is `InvalidOptions`.
    pub blend: BlendSelect,
}

impl Default for CastOptions {
    fn default() -> Self {
        Self {
            lods: LodSelect::default(),
            skin: 0,
            textures: true,
            image: "png".into(),
            root_fixup: RootFixup::default(),
            blend: BlendSelect::default(),
        }
    }
}
