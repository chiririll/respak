//! The [`Mesh`] capability.

use async_trait::async_trait;

use crate::error::Result;
use crate::{Cx, Value};

/// What a mesh holds, known without reading any vertex data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshInfo {
    /// The models (a body part's variants), in file order. Every model exists in every LOD.
    pub models: Vec<ModelInfo>,
    /// Number of levels of detail, 0 when the mesh has no geometry (a skeleton only).
    pub lod_count: u32,
    /// Skin families; at least 1. Family 0 is the default.
    pub skin_count: u32,
}

/// One model of a [`Mesh`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Name of the body part the model belongs to.
    pub bodypart: String,
    /// Index of the model inside its body part.
    pub index: u32,
}

/// The geometry of one model at one level of detail.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshModel {
    /// Parts of the model, one per material.
    pub parts: Vec<MeshPart>,
}

/// Vertices and triangles that share one material.
///
/// Triangles are in the **engine's order** (front faces wind the way the game draws them); a
/// writer whose format wants the other winding reverses them. UVs are as stored (not flipped).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshPart {
    /// Guid of the material, as in [`MeshMaterial::guid`].
    pub material: u64,
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals, one per vertex.
    pub normals: Vec<[f32; 3]>,
    /// Vertex colours (RGBA), empty if the mesh has none.
    pub colors: Vec<[u8; 4]>,
    /// UV layers, each with one entry per vertex.
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// Bone influences per vertex; `bone_indices` and `bone_weights` hold this many entries per
    /// vertex (padded with weight 0). 0 when the mesh is not skinned.
    pub influences: u32,
    /// Indices into [`Skeleton::bones`](crate::capability::Skeleton::bones), `influences` per vertex.
    pub bone_indices: Vec<u16>,
    /// Weights in `0.0..=1.0`, `influences` per vertex; they sum to 1 per vertex.
    pub bone_weights: Vec<f32>,
    /// Triangles as vertex indices.
    pub triangles: Vec<[u32; 3]>,
}

/// A material a mesh refers to, with its textures already located.
#[derive(Debug, Clone)]
pub struct MeshMaterial {
    /// The material's guid.
    pub guid: u64,
    /// A short name for files and nodes.
    pub name: String,
    /// The textures bound to it, in slot order.
    pub textures: Vec<MaterialTexture>,
}

/// One texture of a [`MeshMaterial`].
#[derive(Debug, Clone)]
pub struct MaterialTexture {
    /// The shader's name for the slot (e.g. `albedoTexture`); `slot_<n>` if the shader could not be read.
    pub slot: String,
    /// The texture's guid.
    pub guid: u64,
    /// The texture as a value with a `Bitmap` capability, `None` if it could not be found.
    pub texture: Option<Value>,
}

/// Geometry: models in levels of detail, split into parts by material.
///
/// Lazy like [`Bitmap`](crate::capability::Bitmap): [`MeshInfo`] is cheap, `read` loads (and
/// decodes) only the requested model and LOD.
#[async_trait]
pub trait Mesh: Send + Sync {
    /// The shape of the mesh.
    fn info(&self) -> MeshInfo;

    /// One model at one LOD, with the materials of `skin` (`lod < lod_count`, `model < models.len()`,
    /// `skin < skin_count`; anything else is `NotFound`).
    async fn read(&self, lod: u32, model: u32, skin: u32, cx: &Cx) -> Result<MeshModel>;

    /// The materials used by `skin`, with their textures located through the mediator. A material
    /// that cannot be found is still listed (no textures) and a warning says why.
    async fn materials(&self, skin: u32, cx: &Cx) -> Result<Vec<MeshMaterial>>;
}
