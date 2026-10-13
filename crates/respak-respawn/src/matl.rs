//! `matl`: the Respawn material asset, read from `rpak.asset`.
//!
//! Ported from rsx `src/game/rtech/assets/material.{h,cpp}` (`LoadMaterialAsset`, the header
//! structs of asset versions 15 to 23). Only what the JSON export needs is read: identity, the
//! referenced assets and the texture slots. Offsets were derived from the rsx structs.
//!
//! # Header (little-endian, pointers already `.mem` offsets)
//! | field | v15 – v22 | v23.x |
//! |---|---|---|
//! | snapshot guid | – | 0x08 |
//! | guid | 0x10 | 0x10 |
//! | name / surfaceProp / surfaceProp2 pointers | 0x18 / 0x20 / 0x28 | same |
//! | depthShadow, depthPrepass, depthVSM, depthShadowTight, colpass guids | 0x30 – 0x50 | same |
//! | shaderSet guid | 0x58 | same |
//! | textureHandles pointer | 0x60 | same |
//! | streamingTextureHandles pointer | 0x68 | same |
//! | width / height / depth (u16) | 0x72 / 0x74 / 0x76 | same |
//! | samplers (4 bytes) / features (u32) | 0x78 / 0x80 | same |
//! | glueFlags / glueFlags2 (u32) | 0x88 / 0x8C | 0x84 / 0x88 |
//!
//! Layout is chosen by asset version and header size: 15 to 21 share one layout; 22 and 23 are
//! v22 (256 bytes), v23.1 (192 bytes) or v23.2 (200 bytes). Anything else is
//! `UnsupportedVersion`. v12 (Titanfall 2) has its own layout and is not supported.
//!
//! The texture slots are the u64 guids at `textureHandles`, one per shader bind point (0 = empty
//! slot). There is no count field: the streaming array follows directly, so the count is the
//! pointer difference / 8 (rsx does the same, without checking it).

use std::sync::Arc;

use async_trait::async_trait;
use respak_core::capability::{Dependency, Metadata};
use respak_core::{Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey, Version};

use crate::rpak::{RpakAsset, MEM};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// More slots than this cannot be a material.
const MAX_SLOTS: u64 = 128;

/// The fields of a material header the export uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatlHeader {
    /// Asset version with the layout variant as `minor` (23.1, 23.2).
    pub version: Version,
    /// The material's guid.
    pub guid: u64,
    /// The `msnp` snapshot asset (v23 only, else 0).
    pub snapshot: u64,
    /// `.mem` offsets of the name and the two surface property strings.
    pub name_at: Option<u64>,
    /// See [`name_at`](Self::name_at).
    pub surface_prop_at: Option<u64>,
    /// See [`name_at`](Self::name_at).
    pub surface_prop2_at: Option<u64>,
    /// Depth shadow, depth prepass, depth VSM, depth shadow tight, colpass material guids.
    pub depth: [u64; 5],
    /// The shader set (`shds`) guid.
    pub shader_set: u64,
    /// Width, height, depth.
    pub size: [u16; 3],
    /// Sampler bytes.
    pub samplers: [u8; 4],
    /// The "features" word.
    pub features: u32,
    /// `glueFlags`, `glueFlags2`.
    pub glue: [u32; 2],
    /// `.mem` offset of the texture guid array, and how many slots it has.
    pub textures_at: Option<u64>,
    /// Number of texture slots.
    pub slots: u32,
}

struct Layout {
    snapshot: bool,
    glue: usize,
}

fn le<const N: usize>(h: &[u8], at: usize) -> Result<[u8; N]> {
    h.get(at..at + N).and_then(|b| b.try_into().ok()).ok_or_else(|| corrupt("matl header is too short"))
}
fn u16_at(h: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(le(h, at)?))
}
fn u32_at(h: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(le(h, at)?))
}
fn u64_at(h: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(le(h, at)?))
}
/// A relocated pointer slot: all ones or zero = null.
fn ptr_at(h: &[u8], at: usize) -> Result<Option<u64>> {
    let v = u64_at(h, at)?;
    Ok((v != u64::MAX && v != 0).then_some(v))
}

impl MatlHeader {
    /// Parses a header of asset `version`; `h` is the whole header as served by the volume.
    pub fn parse(version: u32, h: &[u8]) -> Result<Self> {
        let unsupported = |why: String| Error::new(ErrorKind::UnsupportedVersion { format: "matl".into(), version: why });
        let (layout, variant) = match (version, h.len()) {
            (15..=21, _) => (Layout { snapshot: false, glue: 0x88 }, 0),
            (22 | 23, 256) => (Layout { snapshot: false, glue: 0x88 }, 0),
            (22 | 23, 192) => (Layout { snapshot: true, glue: 0x84 }, 1),
            (22 | 23, 200) => (Layout { snapshot: true, glue: 0x84 }, 2),
            (v, n) => return Err(unsupported(format!("{v} (header of {n} bytes)"))),
        };
        let mut depth = [0u64; 5];
        for (i, d) in depth.iter_mut().enumerate() {
            *d = u64_at(h, 0x30 + 8 * i)?;
        }
        Ok(Self {
            version: if variant == 0 { Version::new(version) } else { Version::with_minor(version, variant) },
            guid: u64_at(h, 0x10)?,
            snapshot: if layout.snapshot { u64_at(h, 0x08)? } else { 0 },
            name_at: ptr_at(h, 0x18)?,
            surface_prop_at: ptr_at(h, 0x20)?,
            surface_prop2_at: ptr_at(h, 0x28)?,
            depth,
            shader_set: u64_at(h, 0x58)?,
            size: [u16_at(h, 0x72)?, u16_at(h, 0x74)?, u16_at(h, 0x76)?],
            samplers: le(h, 0x78)?,
            features: u32_at(h, 0x80)?,
            glue: [u32_at(h, layout.glue)?, u32_at(h, layout.glue + 4)?],
            textures_at: ptr_at(h, 0x60)?,
            slots: {
                let (tex, streaming) = (ptr_at(h, 0x60)?, ptr_at(h, 0x68)?);
                match (tex, streaming) {
                    (Some(t), Some(s)) if s >= t && (s - t) % 8 == 0 && (s - t) / 8 <= MAX_SLOTS => ((s - t) / 8) as u32,
                    (None, _) | (_, None) => 0,
                    _ => return Err(corrupt("matl texture array bounds are inconsistent")),
                }
            },
        })
    }
}

/// A material.
#[derive(Debug)]
pub struct MatlAsset {
    format: FormatId,
    key: ValueKey,
    /// The parsed header.
    pub header: MatlHeader,
    /// The material's name (`material/...`), if the pak stored one.
    pub name: Option<String>,
    /// Surface property strings.
    pub surface_props: [Option<String>; 2],
    /// One guid per texture slot; 0 = empty.
    pub textures: Vec<u64>,
}

impl MatlAsset {
    /// The non-empty slots as `(slot index, texture guid)`.
    pub fn bound_textures(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.textures.iter().enumerate().filter(|(_, g)| **g != 0).map(|(i, g)| (i as u32, *g))
    }
}

impl Metadata for MatlAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("matl")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.header.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.bound_textures().map(|(_, guid)| Dependency { guid, name: None }).collect()
    }
}

impl Data for MatlAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// Read edge `rpak.asset -> matl`.
pub struct MatlRead;

#[async_trait]
impl Detect for MatlRead {
    /// The asset kind is `matl` = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"matl").then(|| Detected { score: 10, version: Some(Version::new(asset.entry.version)) }))
    }
}

#[async_trait]
impl ReadHandler for MatlRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source
            .downcast_ref::<RpakAsset>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "matl must be read from an rpak.asset".into() }))?;
        let header = MatlHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();

        let name = match header.name_at {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let mut surface_props = [None, None];
        for (slot, at) in surface_props.iter_mut().zip([header.surface_prop_at, header.surface_prop2_at]) {
            if let Some(at) = at {
                *slot = crate::txtr::read_name(&asset.volume, at, mem_len).await;
            }
        }

        let mut textures = Vec::new();
        if let (Some(at), n) = (header.textures_at, u64::from(header.slots)) {
            if n > 0 {
                if at.checked_add(n * 8).is_none_or(|end| end > mem_len) {
                    return Err(corrupt("matl texture array runs past the end of the pak's pages"));
                }
                let bytes = asset.read_mem(at, n * 8).await?;
                textures = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes"))).collect();
            }
        }

        let format = FormatId::new("matl").with_version(header.version);
        Ok(Arc::new(MatlAsset { format, key: source.key().clone(), header, name, surface_props, textures }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header of `len` bytes with recognisable values.
    fn header(len: usize, v23: bool) -> Vec<u8> {
        let mut h = vec![0u8; len];
        let put = |h: &mut Vec<u8>, at: usize, bytes: &[u8]| h[at..at + bytes.len()].copy_from_slice(bytes);
        put(&mut h, 0x08, &0x5555u64.to_le_bytes());
        put(&mut h, 0x10, &0xAABBu64.to_le_bytes());
        put(&mut h, 0x18, &0x100u64.to_le_bytes());
        put(&mut h, 0x20, &u64::MAX.to_le_bytes());
        for i in 0..5 {
            put(&mut h, 0x30 + 8 * i, &(0x1000 + i as u64).to_le_bytes());
        }
        put(&mut h, 0x58, &0x77u64.to_le_bytes());
        put(&mut h, 0x60, &0x200u64.to_le_bytes());
        put(&mut h, 0x68, &0x218u64.to_le_bytes());
        put(&mut h, 0x72, &512u16.to_le_bytes());
        put(&mut h, 0x74, &256u16.to_le_bytes());
        put(&mut h, 0x76, &1u16.to_le_bytes());
        put(&mut h, 0x78, &[1, 2, 3, 4]);
        put(&mut h, 0x80, &9u32.to_le_bytes());
        let glue = if v23 { 0x84 } else { 0x88 };
        put(&mut h, glue, &0xA1u32.to_le_bytes());
        put(&mut h, glue + 4, &0xA2u32.to_le_bytes());
        h
    }

    #[test]
    fn every_supported_layout_parses_the_same_fields() {
        for (version, len, v23, expect) in [
            (15, 0x100, false, Version::new(15)),
            (16, 240, false, Version::new(16)),
            (21, 240, false, Version::new(21)),
            (22, 256, false, Version::new(22)),
            (23, 192, true, Version::with_minor(23, 1)),
            (23, 200, true, Version::with_minor(23, 2)),
        ] {
            let h = MatlHeader::parse(version, &header(len, v23)).unwrap();
            assert_eq!(h.version, expect, "v{version}/{len}");
            assert_eq!((h.guid, h.shader_set, h.size, h.features), (0xAABB, 0x77, [512, 256, 1], 9));
            assert_eq!((h.name_at, h.surface_prop_at, h.samplers, h.glue), (Some(0x100), None, [1, 2, 3, 4], [0xA1, 0xA2]));
            assert_eq!(h.depth, [0x1000, 0x1001, 0x1002, 0x1003, 0x1004]);
            assert_eq!((h.textures_at, h.slots), (Some(0x200), 3));
            assert_eq!(h.snapshot, if v23 { 0x5555 } else { 0 });
        }
    }

    #[test]
    fn unknown_versions_and_header_sizes_are_unsupported() {
        for (version, len) in [(12, 240), (14, 240), (24, 240), (22, 230), (23, 256 + 8)] {
            let err = MatlHeader::parse(version, &header(len.max(0x90), false)).unwrap_err();
            assert!(matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }), "{version}/{len}: {err}");
        }
    }

    #[test]
    fn inconsistent_texture_arrays_are_corrupt() {
        for (tex, streaming) in [(0x218u64, 0x200u64), (0x200, 0x205), (0x200, 0x200 + 8 * 1000)] {
            let mut h = header(240, false);
            h[0x60..0x68].copy_from_slice(&tex.to_le_bytes());
            h[0x68..0x70].copy_from_slice(&streaming.to_le_bytes());
            let err = MatlHeader::parse(16, &h).unwrap_err();
            assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
        }
    }

    #[test]
    fn a_short_header_is_corrupt_not_a_panic() {
        assert!(MatlHeader::parse(16, &[0; 0x20]).is_err());
    }
}
