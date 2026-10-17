//! The Cast node tree and the writer that builds a file from it.

use std::collections::HashSet;

use respak_core::capability::{Bone, MaterialTexture, MeshMaterial, MeshPart};
use respak_core::{Error, ErrorKind, Result};

use super::custom;

/// FNV-1a, 64 bit.
fn hash64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
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
        Self {
            id: *id,
            hash,
            props: Vec::new(),
            prop_count: 0,
            children: Vec::new(),
        }
    }

    fn prop(&mut self, ty: &[u8], name: &str, len: usize, data: &[u8]) {
        let id = if ty.len() == 1 {
            [ty[0], 0]
        } else {
            [ty[0], ty[1]]
        };
        self.props.extend_from_slice(&id);
        self.props
            .extend_from_slice(&(name.len() as u16).to_le_bytes());
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
        Self {
            modl: Node::new(b"modl", 0),
            skel: None,
            materials: Vec::new(),
            material_guids: HashSet::new(),
            meshes: Vec::new(),
            curves: Vec::new(),
        }
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
            let (Some(role), Some((_, path))) =
                (cast_role(slot), files.iter().find(|(g, _)| g == guid))
            else {
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
        let custom = |message: String| {
            Error::new(ErrorKind::Custom {
                source: None,
                message,
            })
        };
        let n = part.positions.len();
        if n > usize::from(u16::MAX) + 1 {
            return Err(custom(format!(
                "mesh {name}: {n} vertices do not fit Cast's 16-bit face indices"
            )));
        }
        let same = |len: usize| len == n;
        if !same(part.normals.len())
            || !(part.colors.is_empty() || same(part.colors.len()))
            || !part.uvs.iter().all(|l| same(l.len()))
        {
            return Err(custom(format!(
                "mesh {name}: vertex streams have different lengths"
            )));
        }
        let influences = part.influences as usize;
        if influences > 0
            && (part.bone_indices.len() != n * influences
                || part.bone_weights.len() != n * influences)
        {
            return Err(custom(format!(
                "mesh {name}: bone streams do not match {influences} influence(s) per vertex"
            )));
        }
        if part.triangles.iter().flatten().any(|&i| i as usize >= n) {
            return Err(custom(format!(
                "mesh {name}: a triangle refers to a vertex that does not exist"
            )));
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
                let data: Vec<u8> = part
                    .bone_indices
                    .iter()
                    .flat_map(|i| i.to_le_bytes())
                    .collect();
                node.prop(b"h", "wb", part.bone_indices.len(), &data);
            } else {
                let data: Vec<u8> = part.bone_indices.iter().map(|&i| i as u8).collect();
                node.prop(b"b", "wb", part.bone_indices.len(), &data);
            }
            node.floats(
                b"f",
                "wv",
                part.bone_weights.len(),
                part.bone_weights.iter().copied(),
            );
        }
        let faces: Vec<u8> = part
            .triangles
            .iter()
            .flat_map(|&[a, b, c]| [c, b, a])
            .flat_map(|i| (i as u16).to_le_bytes())
            .collect();
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
    pub fn curve(
        &mut self,
        bone: &str,
        key: &str,
        mode: &str,
        weight: Option<f32>,
        frames: u32,
        values: &[f32],
    ) -> Result<()> {
        let width = match key {
            "tx" | "ty" | "tz" | "sx" | "sy" | "sz" => 1,
            "rq" => 4,
            other => {
                return Err(custom(format!(
                    "curve {bone}: `{other}` is not a Cast key property"
                )))
            }
        };
        if mode != "absolute" && mode != "additive" {
            return Err(custom(format!(
                "curve {bone}.{key}: mode `{mode}` is not `absolute` or `additive`"
            )));
        }
        if values.len() as u64 != u64::from(frames) * width {
            return Err(custom(format!(
                "curve {bone}.{key}: {} values for {frames} frames",
                values.len()
            )));
        }
        let mut node = Node::new(b"curv", hash64(&format!("{bone}:{key}")));
        node.string("nn", bone);
        node.string("kp", key);
        let count = frames as usize;
        match frames {
            0..=255 => node.prop(
                b"b",
                "kb",
                count,
                &(0..frames).map(|f| f as u8).collect::<Vec<_>>(),
            ),
            256..=65535 => node.prop(
                b"h",
                "kb",
                count,
                &(0..frames)
                    .flat_map(|f| (f as u16).to_le_bytes())
                    .collect::<Vec<_>>(),
            ),
            _ => node.prop(
                b"i",
                "kb",
                count,
                &(0..frames).flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            ),
        }
        node.floats(
            if width == 1 { b"f" } else { b"v4" },
            "kv",
            count,
            values.iter().copied(),
        );
        node.string("m", mode);
        if mode == "additive" {
            node.floats(b"f", "ab", 1, std::iter::once(weight.unwrap_or(1.0)));
        }
        self.curves.push(node);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn mesh_count(&self) -> usize {
        self.meshes.len()
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
