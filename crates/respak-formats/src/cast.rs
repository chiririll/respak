//! `cast`: Cast models (<https://github.com/dtzxporter/cast>), written from any `Mesh` (+ `Skeleton`).
//! Not readable.
//!
//! Ported from rsx `src/core/mdl/cast.{h,cpp}` (`CastExporter`) and `modeldata.cpp`
//! (`ExportModelCAST`, `ParseMaterialIntoCast`).
//!
//! # File
//! `u32 "cast"`, `u32 version = 1`, `u32 rootNodes`, `u32 flags = 0`; then the nodes. A node is a
//! 24-byte header (`u32 id` FourCC, `u32 nodeSize` including the header, its properties and its
//! children, `u64 hash`, `u32 propertyCount`, `u32 childCount`), its properties, its children.
//! A property is `u16 id` (type), `u16 nameSize`, `u32 arrayLength`, the name (no NUL), the data.
//! The type ids are the bytes `b h i l f d s v2 v3 v4` (a one- or two-character code, the second
//! byte 0 for the single characters).
//!
//! # Output
//! One file per (LOD, model): `root` → `modl` → `skel` (`bone`s), `matl`s (each with `file`
//! children for its textures), `mesh`es. A mesh with no geometry in the request gives a
//! skeleton-only file. Node hashes are FNV-1a of the name (rsx uses its own string hash; a hash
//! only has to be unique inside the file).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Animation, Bone, MaterialTexture, Mesh, MeshMaterial, MeshPart, Skeleton, Track};
use respak_core::value::{BlobData, BundleData};
use respak_core::{ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

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

/// FNV-1a, 64 bit.
fn hash64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The Cast material key for a shader slot name, if Cast has one.
fn cast_role(slot: &str) -> Option<&'static str> {
    Some(match slot {
        "albedoTexture" => "albedo",
        "diffuseTexture" => "diffuse",
        "normalTexture" => "normal",
        "glossTexture" => "gloss",
        "roughnessTexture" => "roughness",
        "specTexture" | "specularTexture" => "specular",
        "emissiveTexture" => "emissive",
        "aoTexture" => "ao",
        "cavityTexture" => "cavity",
        _ => return None,
    })
}

#[derive(Debug)]
struct Node {
    id: [u8; 4],
    hash: u64,
    props: Vec<u8>,
    prop_count: u32,
    children: Vec<Node>,
}

impl Node {
    fn new(id: &[u8; 4], hash: u64) -> Self {
        Self { id: *id, hash, props: Vec::new(), prop_count: 0, children: Vec::new() }
    }

    fn prop(&mut self, ty: &[u8], name: &str, len: usize, data: &[u8]) {
        let id = if ty.len() == 1 { [ty[0], 0] } else { [ty[0], ty[1]] };
        self.props.extend_from_slice(&id);
        self.props.extend_from_slice(&(name.len() as u16).to_le_bytes());
        self.props.extend_from_slice(&(len as u32).to_le_bytes());
        self.props.extend_from_slice(name.as_bytes());
        self.props.extend_from_slice(data);
        self.prop_count += 1;
    }

    fn string(&mut self, name: &str, value: &str) {
        let mut data = value.as_bytes().to_vec();
        data.push(0);
        self.prop(b"s", name, 1, &data);
    }

    fn u64(&mut self, name: &str, value: u64) {
        self.prop(b"l", name, 1, &value.to_le_bytes());
    }

    fn u32(&mut self, name: &str, value: u32) {
        self.prop(b"i", name, 1, &value.to_le_bytes());
    }

    fn u16(&mut self, name: &str, value: u16) {
        self.prop(b"h", name, 1, &value.to_le_bytes());
    }

    fn floats(&mut self, ty: &[u8], name: &str, count: usize, values: impl Iterator<Item = f32>) {
        let data: Vec<u8> = values.flat_map(f32::to_le_bytes).collect();
        self.prop(ty, name, count, &data);
    }

    fn size(&self) -> usize {
        24 + self.props.len() + self.children.iter().map(Node::size).sum::<usize>()
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&(self.size() as u32).to_le_bytes());
        out.extend_from_slice(&self.hash.to_le_bytes());
        out.extend_from_slice(&self.prop_count.to_le_bytes());
        out.extend_from_slice(&(self.children.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.props);
        for c in &self.children {
            c.write(out);
        }
    }
}

/// The `skel` node of `bones`.
fn skeleton_node(name: &str, bones: &[Bone]) -> Node {
    let mut skel = Node::new(b"skel", hash64(name));
    for bone in bones {
        let mut node = Node::new(b"bone", 0);
        node.string("n", &bone.name);
        node.u32("p", bone.parent.unwrap_or(u32::MAX));
        node.floats(b"v3", "lp", 1, bone.position.iter().copied());
        node.floats(b"v4", "lr", 1, bone.rotation.iter().copied());
        skel.children.push(node);
    }
    skel
}

/// Builds the bytes of one Cast file: one model with its skeleton, materials and meshes.
#[derive(Debug)]
pub struct CastWriter {
    modl: Node,
    skel: Option<Node>,
    materials: Vec<Node>,
    material_guids: HashSet<u64>,
    meshes: Vec<Node>,
    curves: Vec<Node>,
}

impl Default for CastWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CastWriter {
    /// An empty file with a `root` and a `modl` node.
    pub fn new() -> Self {
        Self { modl: Node::new(b"modl", 0), skel: None, materials: Vec::new(), material_guids: HashSet::new(), meshes: Vec::new(), curves: Vec::new() }
    }

    /// Names the model and adds its `skel` with `bones`.
    pub fn model(&mut self, name: &str, bones: &[Bone]) {
        self.modl.hash = hash64(name);
        self.skel = Some(skeleton_node(name, bones));
    }

    /// Adds a `matl` node, with a `file` child per texture whose path is given in `files`
    /// (`(texture guid, path relative to the cast file)`). Slots Cast has no key for are left out.
    /// Adding the same guid twice does nothing.
    pub fn material(&mut self, material: &MeshMaterial, files: &[(u64, String)]) {
        if !self.material_guids.insert(material.guid) {
            return;
        }
        let mut node = Node::new(b"matl", material.guid);
        node.string("n", &material.name);
        node.string("t", "pbr");
        for MaterialTexture { slot, guid, .. } in &material.textures {
            let (Some(role), Some((_, path))) = (cast_role(slot), files.iter().find(|(g, _)| g == guid)) else {
                continue;
            };
            node.u64(role, *guid);
            let mut file = Node::new(b"file", *guid);
            file.string("p", path);
            node.children.push(file);
        }
        self.materials.push(node);
    }

    /// Adds a `mesh` node for `part`, named `name`: positions, normals, colours, UV layers, bone
    /// indices and weights, triangles with the winding reversed. More than 65535 vertices is an error.
    pub fn mesh(&mut self, name: &str, part: &MeshPart, bone_count: usize) -> Result<()> {
        let custom = |message: String| Error::new(ErrorKind::Custom { source: None, message });
        let n = part.positions.len();
        if n > usize::from(u16::MAX) + 1 {
            return Err(custom(format!("mesh {name}: {n} vertices do not fit Cast's 16-bit face indices")));
        }
        let same = |len: usize| len == n;
        if !same(part.normals.len())
            || !(part.colors.is_empty() || same(part.colors.len()))
            || !part.uvs.iter().all(|l| same(l.len()))
        {
            return Err(custom(format!("mesh {name}: vertex streams have different lengths")));
        }
        let influences = part.influences as usize;
        if influences > 0
            && (part.bone_indices.len() != n * influences || part.bone_weights.len() != n * influences)
        {
            return Err(custom(format!("mesh {name}: bone streams do not match {influences} influence(s) per vertex")));
        }
        if part.triangles.iter().flatten().any(|&i| i as usize >= n) {
            return Err(custom(format!("mesh {name}: a triangle refers to a vertex that does not exist")));
        }

        let mut node = Node::new(b"mesh", hash64(name));
        node.string("n", name);
        node.floats(b"v3", "vp", n, part.positions.iter().flatten().copied());
        node.floats(b"v3", "vn", n, part.normals.iter().flatten().copied());
        if !part.colors.is_empty() {
            let data: Vec<u8> = part.colors.iter().flatten().copied().collect();
            node.prop(b"i", "vc", n, &data);
        }
        for (i, layer) in part.uvs.iter().enumerate() {
            node.floats(b"v2", &format!("u{i}"), n, layer.iter().flatten().copied());
        }
        if influences > 0 {
            if bone_count > 256 {
                let data: Vec<u8> = part.bone_indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                node.prop(b"h", "wb", part.bone_indices.len(), &data);
            } else {
                let data: Vec<u8> = part.bone_indices.iter().map(|&i| i as u8).collect();
                node.prop(b"b", "wb", part.bone_indices.len(), &data);
            }
            node.floats(b"f", "wv", part.bone_weights.len(), part.bone_weights.iter().copied());
        }
        let faces: Vec<u8> = part.triangles.iter().flat_map(|&[a, b, c]| [c, b, a]).flat_map(|i| (i as u16).to_le_bytes()).collect();
        node.prop(b"h", "f", part.triangles.len() * 3, &faces);
        node.u16("ul", part.uvs.len() as u16);
        if influences > 0 {
            node.u16("mi", influences as u16);
        }
        node.u64("m", part.material);
        self.meshes.push(node);
        Ok(())
    }

    /// Turns the file into an animation: an `anim` node (instead of `modl`) named `name`, with
    /// `fps` and `looping`, and the `skel` of `bones` beside the curves. Replaces
    /// [`model`](Self::model); a file is one or the other.
    pub fn animation(&mut self, name: &str, fps: f32, looping: bool, bones: &[Bone]) {
        let mut anim = Node::new(b"anim", hash64(name));
        anim.floats(b"f", "fr", 1, std::iter::once(fps));
        anim.prop(b"b", "lo", 1, &[u8::from(looping)]);
        self.modl = anim;
        self.skel = Some(skeleton_node(name, bones));
    }

    /// Adds a `curv` node for `bone`: `key` is Cast's key property (`tx`, `ty`, `tz`, `rq`, `sx`,
    /// `sy`, `sz`), `mode` `"absolute"` or `"additive"` (`weight`, written only when additive).
    /// `values` holds `frames` entries of 1 (`t*`, `s*`) or 4 (`rq`) floats, one per frame; every
    /// frame is keyed (`kb` is `u8`, `u16` or `u32` by `frames`). A length that does not match is
    /// an error.
    pub fn curve(&mut self, bone: &str, key: &str, mode: &str, weight: Option<f32>, frames: u32, values: &[f32]) -> Result<()> {
        let width = match key {
            "tx" | "ty" | "tz" | "sx" | "sy" | "sz" => 1,
            "rq" => 4,
            other => return Err(custom(format!("curve {bone}: `{other}` is not a Cast key property"))),
        };
        if mode != "absolute" && mode != "additive" {
            return Err(custom(format!("curve {bone}.{key}: mode `{mode}` is not `absolute` or `additive`")));
        }
        if values.len() as u64 != u64::from(frames) * width {
            return Err(custom(format!("curve {bone}.{key}: {} values for {frames} frames", values.len())));
        }
        let mut node = Node::new(b"curv", hash64(&format!("{bone}:{key}")));
        node.string("nn", bone);
        node.string("kp", key);
        let count = frames as usize;
        match frames {
            0..=255 => node.prop(b"b", "kb", count, &(0..frames).map(|f| f as u8).collect::<Vec<_>>()),
            256..=65535 => node.prop(b"h", "kb", count, &(0..frames).flat_map(|f| (f as u16).to_le_bytes()).collect::<Vec<_>>()),
            _ => node.prop(b"i", "kb", count, &(0..frames).flat_map(u32::to_le_bytes).collect::<Vec<_>>()),
        }
        node.floats(if width == 1 { b"f" } else { b"v4" }, "kv", count, values.iter().copied());
        node.string("m", mode);
        if mode == "additive" {
            node.floats(b"f", "ab", 1, std::iter::once(weight.unwrap_or(1.0)));
        }
        self.curves.push(node);
        Ok(())
    }

    /// The finished file.
    pub fn finish(mut self) -> Vec<u8> {
        let mut modl = self.modl;
        modl.children.extend(self.skel.take());
        modl.children.append(&mut self.materials);
        modl.children.append(&mut self.meshes);
        modl.children.append(&mut self.curves);
        let mut root = Node::new(b"root", 0);
        root.children.push(modl);
        let mut out = Vec::with_capacity(16 + root.size());
        out.extend_from_slice(b"cast");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        root.write(&mut out);
        out
    }
}

fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn custom(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Custom { source: None, message: message.into() })
}

fn is_cancelled(e: &Error) -> bool {
    matches!(e.kind(), ErrorKind::Cancelled)
}

/// The last path component of `name` without its extension.
fn stem_of(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file_safe(last.rsplit_once('.').map_or(last, |(s, _)| s))
}

/// Capability edge `Mesh -> cast`: a composite, `<bodypart>_<i>_LOD<n>.cast` per (LOD, model)
/// plus `textures/<material>/<slot>.<ext>`.
pub struct CastEncode;

#[async_trait]
impl ConvertHandler for CastEncode {
    /// Reads the requested models, converts the textures through the mediator, and writes the
    /// files. Checks cancellation per model and per texture. A model without geometry is skipped;
    /// a mesh with no LODs at all gives `<stem>.cast` with the skeleton only. A texture that cannot
    /// be converted is left out with a warning.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<CastOptions>::parse(target)?;
        let opts = options.options();
        let mesh: &dyn Mesh = source
            .as_mesh()
            .ok_or_else(|| custom(format!("`{}` has no Mesh capability", source.format())))?;
        let skeleton: &dyn Skeleton = source
            .as_skeleton()
            .ok_or_else(|| custom(format!("`{}` has no Skeleton capability", source.format())))?;
        let bones = skeleton.bones();
        let info = mesh.info();
        if opts.skin >= info.skin_count {
            return Err(Error::new(ErrorKind::InvalidOptions {
                message: format!("skin {}: the model has {} skin(s)", opts.skin, info.skin_count),
            }));
        }
        let lods = if info.lod_count == 0 { Vec::new() } else { opts.lods.resolve(info.lod_count)? };
        let stem = source.as_metadata().and_then(|m| m.name()).map_or_else(|| "model".to_owned(), stem_of);

        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("cast"), options.normalized());
        let cast_value = |bytes: Vec<u8>| -> Value {
            Arc::new(BlobData::with_format(FormatId::new("cast"), Bytes::from(bytes), key.clone()))
        };
        let mut items: Vec<(String, Value)> = Vec::new();

        if lods.is_empty() {
            let mut writer = CastWriter::new();
            writer.model(&stem, bones);
            items.push((format!("{stem}.cast"), cast_value(writer.finish())));
            return Ok(Arc::new(BundleData::named(items, key.clone())));
        }

        // Materials and their textures: each texture is converted once and shared by name.
        let materials = mesh.materials(opts.skin, cx).await?;
        let image = Target { format: FormatId::new(opts.image.clone()), options: Json::Null, via: Vec::new() };
        let mut texture_paths: BTreeMap<u64, String> = BTreeMap::new();
        if opts.textures {
            for material in &materials {
                for texture in &material.textures {
                    cx.check_cancelled()?;
                    if texture_paths.contains_key(&texture.guid) {
                        continue;
                    }
                    let Some(value) = &texture.texture else { continue };
                    match cx.mediator.convert(value.clone(), image.clone(), cx).await {
                        Ok(converted) => {
                            let ext = cx.mediator.extension_of(converted.format().name()).unwrap_or(converted.format().name()).to_owned();
                            if converted.as_blob().is_some() {
                                let path = format!("textures/{}/{}.{ext}", file_safe(&material.name), file_safe(&texture.slot));
                                items.push((path.clone(), converted));
                                texture_paths.insert(texture.guid, path);
                            } else {
                                cx.warn(format!("material {}: texture {:#018x} did not produce a file", material.name, texture.guid));
                            }
                        }
                        Err(e) if is_cancelled(&e) => return Err(e),
                        Err(e) => cx.warn(format!("material {}: texture {:#018x} ({}) is skipped: {e}", material.name, texture.guid, texture.slot)),
                    }
                }
            }
        }
        let files: Vec<(u64, String)> = texture_paths.into_iter().collect();

        for &lod in &lods {
            for (model, model_info) in info.models.iter().enumerate() {
                cx.check_cancelled()?;
                let data = mesh.read(lod, model as u32, opts.skin, cx).await?;
                if data.parts.is_empty() {
                    continue;
                }
                let name = format!("{}_{}_LOD{lod}", file_safe(&model_info.bodypart), model_info.index);
                let mut writer = CastWriter::new();
                writer.model(&name, bones);
                for material in materials.iter().filter(|m| data.parts.iter().any(|p| p.material == m.guid)) {
                    writer.material(material, &files);
                }
                for part in &data.parts {
                    let material_name = materials
                        .iter()
                        .find(|m| m.guid == part.material)
                        .map_or_else(|| format!("{:#018x}", part.material), |m| m.name.clone());
                    writer.mesh(&format!("{name}_{material_name}"), part, bones.len())?;
                }
                items.push((format!("{name}.cast"), cast_value(writer.finish())));
            }
        }
        Ok(Arc::new(BundleData::named(items, key.clone())))
    }
}

/// Bone 0's position and rotation `[x, y, z, w]` after rsx's root fix-up: the rotation as Source
/// angles with yaw - 90 degrees, the position `(x, y)` to `(y, -x)` (`ParseAnimDesc_Origin`).
fn root_fixup(position: [f32; 3], rotation: [f32; 4]) -> ([f32; 3], [f32; 4]) {
    // Lowering the Source yaw by 90 degrees is a turn about Z applied after the rest of the rotation.
    let (s, c) = (-std::f32::consts::FRAC_PI_4).sin_cos();
    ([position[1], -position[0], position[2]], quat_mul([0.0, 0.0, s, c], rotation))
}

/// The Hamilton product `a * b` of two quaternions `[x, y, z, w]`.
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// One value per frame from a track.
fn per_frame<T: Copy>(track: &Track<T>, frames: u32, what: &str) -> Result<Vec<T>> {
    match track {
        Track::Constant(v) => Ok(vec![*v; frames as usize]),
        Track::Dense(v) if v.len() == frames as usize => Ok(v.clone()),
        Track::Dense(v) => Err(custom(format!("{what}: {} values for {frames} frames", v.len()))),
    }
}

/// Capability edge `Animation -> cast`: a composite, `<stem>_<i>.cast` per written blend, each
/// with the skeleton and, per bone, position (`tx`, `ty`, `tz`), rotation (`rq`) and, when the
/// animation has scale, scale (`sx`, `sy`, `sz`) curves.
pub struct CastAnimationEncode;

#[async_trait]
impl ConvertHandler for CastAnimationEncode {
    /// Writes the blends chosen by [`CastOptions::blend`]. Bone 0 gets [`root_fixup`] when
    /// [`CastOptions::root_fixup`] is `Rsx`. Checks cancellation per blend and per bone. A
    /// `Constant` track is written as full-length curves like rsx (no collapsing). Additive
    /// animations write the bone weights as `ab`.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<CastOptions>::parse(target)?;
        let opts = options.options();
        let animation: &dyn Animation = source
            .as_animation()
            .ok_or_else(|| custom(format!("`{}` has no Animation capability", source.format())))?;
        let blends = opts.blend.resolve(animation.blend_count())?;
        let bones = animation.bones();
        let weights = animation.bone_weights();
        let stem = source.as_metadata().and_then(|m| m.name()).map_or_else(|| "animation".to_owned(), stem_of);

        let key = ValueKey::converted(Arc::new(source.key().clone()), FormatId::new("cast"), options.normalized());
        let mut items: Vec<(String, Value)> = Vec::new();
        for blend in blends {
            cx.check_cancelled()?;
            let info = animation.info(blend)?;
            let frames = info.frame_count;
            let mode = if info.additive { "additive" } else { "absolute" };
            let mut writer = CastWriter::new();
            writer.animation(&info.name, info.fps, info.looping, bones);
            for (index, bone) in bones.iter().enumerate() {
                cx.check_cancelled()?;
                if frames == 0 {
                    break;
                }
                let track = animation.track(blend, index as u32, cx).await?;
                let what = format!("bone {}", bone.name);
                let mut translation = per_frame(&track.translation, frames, &what)?;
                let mut rotation = per_frame(&track.rotation, frames, &what)?;
                if index == 0 && opts.root_fixup == RootFixup::Rsx {
                    for (t, r) in translation.iter_mut().zip(rotation.iter_mut()) {
                        (*t, *r) = root_fixup(*t, *r);
                    }
                }
                let weight = info.additive.then(|| weights.get(index).copied().unwrap_or(1.0));
                for (axis, name) in ["tx", "ty", "tz"].into_iter().enumerate() {
                    let values: Vec<f32> = translation.iter().map(|v| v[axis]).collect();
                    writer.curve(&bone.name, name, mode, weight, frames, &values)?;
                }
                writer.curve(&bone.name, "rq", mode, weight, frames, &rotation.concat())?;
                if info.has_scale {
                    let scale = per_frame(&track.scale, frames, &what)?;
                    for (axis, name) in ["sx", "sy", "sz"].into_iter().enumerate() {
                        let values: Vec<f32> = scale.iter().map(|v| v[axis]).collect();
                        writer.curve(&bone.name, name, mode, weight, frames, &values)?;
                    }
                }
            }
            let value: Value =
                Arc::new(BlobData::with_format(FormatId::new("cast"), Bytes::from(writer.finish()), key.clone()));
            items.push((format!("{stem}_{blend}.cast"), value));
        }
        Ok(Arc::new(BundleData::named(items, key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parsed node, for tests.
    #[derive(Debug)]
    struct Parsed {
        id: String,
        size: u32,
        hash: u64,
        props: Vec<(Vec<u8>, String, u32, Vec<u8>)>,
        children: Vec<Parsed>,
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn type_len(ty: &[u8]) -> usize {
        match ty {
            b"b\0" => 1,
            b"h\0" => 2,
            b"i\0" | b"f\0" => 4,
            b"l\0" | b"d\0" => 8,
            b"v2" => 8,
            b"v3" => 12,
            b"v4" => 16,
            _ => panic!("type {ty:?}"),
        }
    }

    fn parse_node(b: &[u8], at: usize) -> (Parsed, usize) {
        let id = String::from_utf8(b[at..at + 4].to_vec()).unwrap();
        let size = u32_at(b, at + 4);
        let hash = u64::from_le_bytes(b[at + 8..at + 16].try_into().unwrap());
        let (pc, cc) = (u32_at(b, at + 16), u32_at(b, at + 20));
        let mut p = at + 24;
        let mut props = Vec::new();
        for _ in 0..pc {
            let ty = b[p..p + 2].to_vec();
            let name_len = u16::from_le_bytes([b[p + 2], b[p + 3]]) as usize;
            let arr = u32_at(b, p + 4);
            let name = String::from_utf8(b[p + 8..p + 8 + name_len].to_vec()).unwrap();
            let start = p + 8 + name_len;
            let len = if ty == b"s\0" {
                b[start..].iter().position(|&c| c == 0).unwrap() + 1
            } else {
                type_len(&ty) * arr as usize
            };
            props.push((ty, name, arr, b[start..start + len].to_vec()));
            p = start + len;
        }
        let mut children = Vec::new();
        for _ in 0..cc {
            let (c, next) = parse_node(b, p);
            children.push(c);
            p = next;
        }
        assert_eq!(p - at, size as usize, "node {id}: nodeSize covers header, properties and children");
        (Parsed { id, size, hash, props, children }, p)
    }

    fn parse(bytes: &[u8]) -> Parsed {
        assert_eq!(&bytes[0..4], b"cast");
        assert_eq!(u32_at(bytes, 4), 1);
        assert_eq!(u32_at(bytes, 8), 1);
        assert_eq!(u32_at(bytes, 12), 0);
        let (root, end) = parse_node(bytes, 16);
        assert_eq!(end, bytes.len());
        root
    }

    fn prop<'a>(n: &'a Parsed, name: &str) -> &'a (Vec<u8>, String, u32, Vec<u8>) {
        n.props.iter().find(|p| p.1 == name).unwrap_or_else(|| panic!("no property {name} in {}", n.id))
    }

    fn bone(name: &str, parent: Option<u32>) -> Bone {
        Bone { name: name.into(), parent, position: [1.0, 2.0, 3.0], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] }
    }

    fn part(vertices: usize) -> MeshPart {
        MeshPart {
            material: 7,
            positions: (0..vertices).map(|i| [i as f32, 0.0, 0.0]).collect(),
            normals: vec![[0.0, 0.0, 1.0]; vertices],
            uvs: vec![vec![[0.5, 0.25]; vertices]],
            triangles: vec![[0, 1, 2]],
            ..MeshPart::default()
        }
    }

    #[test]
    fn skeleton_only_file() {
        let mut w = CastWriter::new();
        w.model("m", &[bone("root", None), bone("child", Some(0))]);
        let root = parse(&w.finish());
        assert_eq!(root.id, "root");
        let modl = &root.children[0];
        assert_eq!(modl.id, "modl");
        let skel = &modl.children[0];
        assert_eq!(skel.id, "skel");
        assert_eq!(skel.children.len(), 2);
        let b1 = &skel.children[1];
        assert_eq!(b1.id, "bone");
        assert_eq!(prop(b1, "n").3, b"child\0");
        assert_eq!(prop(b1, "p").3, 0u32.to_le_bytes());
        assert_eq!(prop(&skel.children[0], "p").3, u32::MAX.to_le_bytes());
        assert_eq!(prop(b1, "lp").0, b"v3");
        assert_eq!(prop(b1, "lr").0, b"v4");
        assert_eq!(prop(b1, "lr").3.len(), 16);
    }

    #[test]
    fn mesh_streams_and_reversed_winding() {
        let mut w = CastWriter::new();
        w.model("m", &[bone("root", None)]);
        let mut p = part(3);
        p.influences = 2;
        p.bone_indices = vec![0, 0, 0, 0, 0, 0];
        p.bone_weights = vec![1.0, 0.0, 0.5, 0.5, 1.0, 0.0];
        p.colors = vec![[1, 2, 3, 4]; 3];
        w.mesh("m_mat", &p, 1).unwrap();
        let root = parse(&w.finish());
        let mesh = root.children[0].children.iter().find(|c| c.id == "mesh").unwrap();
        assert_eq!(prop(mesh, "vp").2, 3);
        assert_eq!(prop(mesh, "vp").3.len(), 36);
        assert_eq!(prop(mesh, "u0").0, b"v2");
        assert_eq!(prop(mesh, "wb").0, b"b\0");
        assert_eq!(prop(mesh, "wb").2, 6);
        assert_eq!(prop(mesh, "wv").3.len(), 24);
        assert_eq!(prop(mesh, "vc").3, vec![1, 2, 3, 4, 1, 2, 3, 4, 1, 2, 3, 4]);
        assert_eq!(prop(mesh, "f").3, vec![2, 0, 1, 0, 0, 0]);
        assert_eq!(prop(mesh, "ul").3, 1u16.to_le_bytes());
        assert_eq!(prop(mesh, "mi").3, 2u16.to_le_bytes());
        assert_eq!(prop(mesh, "m").3, 7u64.to_le_bytes());
    }

    #[test]
    fn more_than_256_bones_use_16_bit_indices() {
        let mut w = CastWriter::new();
        w.model("m", &[]);
        let mut p = part(3);
        p.influences = 1;
        p.bone_indices = vec![300, 1, 2];
        p.bone_weights = vec![1.0; 3];
        w.mesh("x", &p, 400).unwrap();
        let root = parse(&w.finish());
        let mesh = root.children[0].children.iter().find(|c| c.id == "mesh").unwrap();
        assert_eq!(prop(mesh, "wb").0, b"h\0");
        assert_eq!(&prop(mesh, "wb").3[..2], &300u16.to_le_bytes());
    }

    #[test]
    fn unskinned_mesh_has_no_weight_streams() {
        let mut w = CastWriter::new();
        w.model("m", &[]);
        w.mesh("x", &part(3), 0).unwrap();
        let root = parse(&w.finish());
        let mesh = root.children[0].children.iter().find(|c| c.id == "mesh").unwrap();
        assert!(mesh.props.iter().all(|p| p.1 != "wb" && p.1 != "wv" && p.1 != "mi" && p.1 != "vc"));
    }

    #[test]
    fn too_many_vertices_and_bad_streams_are_errors() {
        let mut w = CastWriter::new();
        let big = MeshPart { positions: vec![[0.0; 3]; 65537], normals: vec![[0.0; 3]; 65537], ..MeshPart::default() };
        assert!(w.mesh("big", &big, 0).is_err());
        let mut short = part(3);
        short.normals.pop();
        assert!(w.mesh("short", &short, 0).is_err());
        let mut out_of_range = part(3);
        out_of_range.triangles = vec![[0, 1, 3]];
        assert!(w.mesh("range", &out_of_range, 0).is_err());
        let mut weights = part(3);
        weights.influences = 2;
        assert!(w.mesh("weights", &weights, 0).is_err());
        assert!(w.meshes.is_empty(), "a failed mesh adds nothing");
    }

    #[test]
    fn material_is_written_once_with_known_slots_only() {
        let mat = MeshMaterial {
            guid: 7,
            name: "wall".into(),
            textures: vec![
                MaterialTexture { slot: "albedoTexture".into(), guid: 100, texture: None },
                MaterialTexture { slot: "weirdTexture".into(), guid: 101, texture: None },
                MaterialTexture { slot: "normalTexture".into(), guid: 102, texture: None },
            ],
        };
        let files = vec![(100, "textures/wall/albedoTexture.png".to_owned()), (101, "x.png".to_owned())];
        let mut w = CastWriter::new();
        w.model("m", &[]);
        w.material(&mat, &files);
        w.material(&mat, &files);
        let root = parse(&w.finish());
        let matls: Vec<_> = root.children[0].children.iter().filter(|c| c.id == "matl").collect();
        assert_eq!(matls.len(), 1);
        let m = matls[0];
        assert_eq!(m.hash, 7);
        assert_eq!(prop(m, "n").3, b"wall\0");
        assert_eq!(prop(m, "t").3, b"pbr\0");
        assert_eq!(prop(m, "albedo").3, 100u64.to_le_bytes());
        assert!(m.props.iter().all(|p| p.1 != "normal"), "no file, no slot");
        assert_eq!(m.children.len(), 1);
        assert_eq!(m.children[0].id, "file");
        assert_eq!(m.children[0].hash, 100);
        assert_eq!(prop(&m.children[0], "p").3, b"textures/wall/albedoTexture.png\0");
    }

    #[test]
    fn children_come_in_skel_matl_mesh_order() {
        let mut w = CastWriter::new();
        w.model("m", &[bone("r", None)]);
        w.mesh("a", &part(3), 1).unwrap();
        w.material(&MeshMaterial { guid: 1, name: "x".into(), textures: vec![] }, &[]);
        let root = parse(&w.finish());
        let ids: Vec<_> = root.children[0].children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["skel", "matl", "mesh"]);
        assert!(root.size > 0);
        assert_ne!(root.children[0].hash, 0);
    }

    #[test]
    fn lod_select() {
        assert_eq!(LodSelect::default().resolve(3).unwrap(), vec![0]);
        assert_eq!(LodSelect::All(AllLods::All).resolve(3).unwrap(), vec![0, 1, 2]);
        assert_eq!(LodSelect::List(vec![2, 0, 2]).resolve(3).unwrap(), vec![0, 2]);
        assert!(matches!(LodSelect::One(3).resolve(3).unwrap_err().kind(), ErrorKind::InvalidOptions { .. }));
        let parsed: CastOptions = serde_json::from_str(r#"{"lods":"all","skin":2}"#).unwrap();
        assert_eq!(parsed.lods, LodSelect::All(AllLods::All));
        let parsed: CastOptions = serde_json::from_str(r#"{"lods":[0,1]}"#).unwrap();
        assert_eq!(parsed.lods, LodSelect::List(vec![0, 1]));
        let parsed: CastOptions = serde_json::from_str(r#"{"lods":1}"#).unwrap();
        assert_eq!(parsed.lods, LodSelect::One(1));
    }

    #[test]
    fn stems() {
        assert_eq!(stem_of("mdl/props/crate_01.rmdl"), "crate_01");
        assert_eq!(stem_of("plain"), "plain");
    }

    fn curves(root: &Parsed) -> Vec<&Parsed> {
        root.children[0].children.iter().filter(|c| c.id == "curv").collect()
    }

    #[test]
    fn animation_file_has_anim_node_skeleton_and_curves() {
        let mut w = CastWriter::new();
        w.animation("walk", 24.0, true, &[bone("root", None), bone("child", Some(0))]);
        w.curve("root", "tx", "absolute", None, 3, &[1.0, 2.0, 3.0]).unwrap();
        w.curve("child", "rq", "absolute", None, 2, &[0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]).unwrap();
        let root = parse(&w.finish());
        let anim = &root.children[0];
        assert_eq!(anim.id, "anim");
        assert_ne!(anim.hash, 0);
        assert_eq!(prop(anim, "fr").0, b"f\0");
        assert_eq!(prop(anim, "fr").3, 24.0f32.to_le_bytes());
        assert_eq!(prop(anim, "lo").0, b"b\0");
        assert_eq!(prop(anim, "lo").3, [1]);
        assert!(anim.props.iter().all(|p| p.1 != "n"), "rsx writes no name");
        let ids: Vec<_> = anim.children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["skel", "curv", "curv"]);
        assert_eq!(anim.children[0].children.len(), 2);

        let c = curves(&root);
        let tx = c[0];
        assert_eq!(prop(tx, "nn").3, b"root\0");
        assert_eq!(prop(tx, "kp").3, b"tx\0");
        assert_eq!((prop(tx, "kb").0.as_slice(), prop(tx, "kb").2, prop(tx, "kb").3.as_slice()), (&b"b\0"[..], 3, &[0u8, 1, 2][..]));
        assert_eq!((prop(tx, "kv").0.as_slice(), prop(tx, "kv").2), (&b"f\0"[..], 3));
        assert_eq!(prop(tx, "kv").3, [1.0f32, 2.0, 3.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        assert_eq!(prop(tx, "m").3, b"absolute\0");
        assert!(tx.props.iter().all(|p| p.1 != "ab"), "no weight in absolute mode");

        let rq = c[1];
        assert_eq!(prop(rq, "nn").3, b"child\0");
        assert_eq!((prop(rq, "kv").0.as_slice(), prop(rq, "kv").2, prop(rq, "kv").3.len()), (&b"v4"[..], 2, 32));

        let mut looped = CastWriter::new();
        looped.animation("x", 30.0, false, &[]);
        assert_eq!(prop(&parse(&looped.finish()).children[0], "lo").3, [0]);
    }

    #[test]
    fn additive_curves_carry_their_weight() {
        let mut w = CastWriter::new();
        w.animation("delta", 30.0, false, &[bone("root", None)]);
        w.curve("root", "ty", "additive", Some(0.25), 1, &[5.0]).unwrap();
        w.curve("root", "tz", "additive", None, 1, &[5.0]).unwrap();
        w.curve("root", "sx", "absolute", Some(0.5), 1, &[5.0]).unwrap();
        let root = parse(&w.finish());
        let c = curves(&root);
        assert_eq!(prop(c[0], "m").3, b"additive\0");
        assert_eq!((prop(c[0], "ab").0.as_slice(), prop(c[0], "ab").3.as_slice()), (&b"f\0"[..], &0.25f32.to_le_bytes()[..]));
        assert_eq!(prop(c[1], "ab").3, 1.0f32.to_le_bytes(), "no weight means 1");
        assert!(c[2].props.iter().all(|p| p.1 != "ab"));
    }

    #[test]
    fn key_buffer_width_follows_the_frame_count() {
        for (frames, ty) in [(1u32, b"b\0"), (255, b"b\0"), (256, b"h\0"), (65535, b"h\0"), (65536, b"i\0")] {
            let mut w = CastWriter::new();
            w.animation("a", 30.0, false, &[]);
            w.curve("b", "tx", "absolute", None, frames, &vec![0.5; frames as usize]).unwrap();
            let root = parse(&w.finish());
            let kb = prop(curves(&root)[0], "kb");
            assert_eq!(kb.0, ty, "{frames} frames");
            assert_eq!(kb.2, frames);
            let width = match ty {
                b"b\0" => 1,
                b"h\0" => 2,
                _ => 4,
            };
            assert_eq!(kb.3.len(), frames as usize * width);
            // the last key is the last frame
            let last = &kb.3[kb.3.len() - width..];
            let mut bytes = [0u8; 4];
            bytes[..width].copy_from_slice(last);
            assert_eq!(u32::from_le_bytes(bytes), frames - 1);
        }
    }

    #[test]
    fn bad_curves_are_errors_and_add_nothing() {
        let mut w = CastWriter::new();
        w.animation("a", 30.0, false, &[]);
        assert!(w.curve("b", "tx", "absolute", None, 3, &[1.0, 2.0]).is_err(), "too few values");
        assert!(w.curve("b", "rq", "absolute", None, 2, &[0.0; 4]).is_err(), "a quaternion has four floats");
        assert!(w.curve("b", "tw", "absolute", None, 1, &[0.0]).is_err(), "unknown key");
        assert!(w.curve("b", "tx", "relative", None, 1, &[0.0]).is_err(), "unknown mode");
        assert!(curves(&parse(&w.finish())).is_empty());
    }

    #[test]
    fn zero_frames_make_empty_curves() {
        let mut w = CastWriter::new();
        w.animation("a", 30.0, false, &[]);
        w.curve("b", "tx", "absolute", None, 0, &[]).unwrap();
        let root = parse(&w.finish());
        assert_eq!(prop(curves(&root)[0], "kb").2, 0);
    }

    fn euler(roll: f32, pitch: f32, yaw: f32) -> [f32; 4] {
        let (sr, cr) = (roll * 0.5).sin_cos();
        let (sp, cp) = (pitch * 0.5).sin_cos();
        let (sy, cy) = (yaw * 0.5).sin_cos();
        [sr * cp * cy - cr * sp * sy, cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy, cr * cp * cy + sr * sp * sy]
    }

    fn near(a: &[f32], b: &[f32]) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-5, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn root_fixup_turns_the_yaw_by_minus_90_and_swaps_the_plane() {
        let (p, q) = root_fixup([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(p, [2.0, -1.0, 3.0]);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        near(&q, &[0.0, 0.0, -s, s]);

        // the same as lowering the Source yaw by 90 degrees
        let (_, q) = root_fixup([0.0; 3], euler(0.3, 0.2, 0.5));
        near(&q, &euler(0.3, 0.2, 0.5 - std::f32::consts::FRAC_PI_2));
        let (_, q) = root_fixup([0.0; 3], euler(0.0, 0.0, std::f32::consts::FRAC_PI_2));
        near(&q, &[0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn blend_select() {
        assert_eq!(BlendSelect::default().resolve(3).unwrap(), vec![0, 1, 2]);
        assert_eq!(BlendSelect::default().resolve(0).unwrap(), Vec::<u32>::new());
        assert_eq!(BlendSelect::One(1).resolve(3).unwrap(), vec![1]);
        assert!(matches!(BlendSelect::One(3).resolve(3).unwrap_err().kind(), ErrorKind::InvalidOptions { .. }));
        let parsed: CastOptions = serde_json::from_str(r#"{"blend":2,"root_fixup":"none"}"#).unwrap();
        assert_eq!((parsed.blend, parsed.root_fixup), (BlendSelect::One(2), RootFixup::None));
        let parsed: CastOptions = serde_json::from_str(r#"{"blend":"all"}"#).unwrap();
        assert_eq!((parsed.blend, parsed.root_fixup), (BlendSelect::default(), RootFixup::Rsx));
        assert!(serde_json::from_str::<CastOptions>(r#"{"root_fixup":"flip"}"#).is_err());
        assert!(serde_json::from_str::<CastOptions>(r#"{"blend":"some"}"#).is_err());
    }
}
