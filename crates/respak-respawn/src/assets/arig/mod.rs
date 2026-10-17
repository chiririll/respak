//! `arig`: a skeleton-only model that lists the animation sequences of its bones, read from
//! `rpak.asset`. Apex Legends, asset versions 4 to 7. Yields a [`Skeleton`]; no Cast export of the
//! rig itself.
//!
//! Ported from rsx `src/game/rtech/assets/animrig.{h,cpp}` (`AnimRigAssetHeader_v4_t/_v5_t`,
//! `LoadAnimRigAsset`) and `model.cpp` (`GetModelVersionFromAsset`, `ParseExternalSequences`).
//! Offsets are in `docs/formats/arig.md`. The embedded studiohdr is the `rmdl` one: the bones come
//! from the same parser (`rmdl::parse_bones`).
//!
//! Behaviour changes against rsx: a pak without a `mdl_` is fine (rsx throws); a rig with local
//! sequences is `Corrupt` instead of an assert; the studio variant is told by the `studio` option
//! (see [`StudioSelect`]).

mod owners;
pub(crate) use owners::*;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use opendal::Operator;
use respak_core::capability::{Bone, Dependency, Metadata, Skeleton};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

use crate::guid::hex;
use crate::guid::string_to_guid;
use crate::matl::{locate, pak_of};
use crate::rmdl::{self, parse_bones, read_guids, reader, StudioLayout};
use crate::rpak::{RpakAsset, MEM};

fn unsupported(version: impl Into<String>) -> Error {
    Error::unsupported("arig", version)
}

const HEADER_LEN: usize = 0x28;
/// How much of `.mem` after the studiohdr is read as the rig's studio block (its length is not stored).
const STUDIO_WINDOW: u64 = 2 << 20;

/// How the studiohdr variant of a rig is found (the `studio` option): `"auto"` or a layout name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum StudioSelect {
    /// A sibling `mdl_` of the same pak first (rsx behaviour), then the heuristic of
    /// `arig.md` (`sourceFilenameOffset` and the bone data stride). Written `"auto"`.
    #[default]
    Auto,
    /// A named layout: `"v16"`, `"v17"` or `"v19_2"` ([`StudioLayout`]); the major is taken from
    /// the layout (16, 18, 20).
    Layout(String),
}

impl From<String> for StudioSelect {
    fn from(s: String) -> Self {
        if s == "auto" {
            Self::Auto
        } else {
            Self::Layout(s)
        }
    }
}

impl From<StudioSelect> for String {
    fn from(s: StudioSelect) -> Self {
        match s {
            StudioSelect::Auto => "auto".into(),
            StudioSelect::Layout(name) => name,
        }
    }
}

/// The decoded asset header (0x28 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArigHeader {
    /// The asset version, 4 to 7.
    pub version: Version,
    /// Where the studiohdr is in `.mem`.
    pub studio_offset: u64,
    /// The name pointer as a `.mem` offset, `None` if null.
    pub name_offset: Option<u64>,
    /// Where the `aseq` guids are (`.mem` offset) and how many (`i32` at 0x14 in v4, `i16` at 0x12 later).
    pub anim_seqs: (Option<u64>, u32),
}

impl ArigHeader {
    /// Parses a relocated header of `asset_version` 4 to 7; others are `UnsupportedVersion`.
    pub fn parse(asset_version: u32, header: &[u8]) -> Result<Self> {
        if !(4..=7).contains(&asset_version) {
            return Err(unsupported(asset_version.to_string()));
        }
        if header.len() != HEADER_LEN {
            return Err(Error::corrupt(format!(
                "arig header is {} bytes, expected {HEADER_LEN}",
                header.len()
            )));
        }
        let h = reader(header);
        let ptr = |at: usize| -> Result<Option<u64>> {
            let v = h.u64(at)?;
            Ok((v != u64::MAX && v != 0).then_some(v))
        };
        let count = if asset_version == 4 {
            u32::try_from(h.i32(0x14)?)
                .map_err(|_| Error::corrupt("arig: negative sequence count"))?
        } else {
            u32::from(h.u16(0x12)?)
        };
        Ok(Self {
            version: Version::new(asset_version),
            studio_offset: ptr(0x00)?
                .ok_or_else(|| Error::corrupt("arig has no studio data pointer"))?,
            name_offset: ptr(0x08)?,
            anim_seqs: (ptr(0x18)?, count),
        })
    }
}

/// The studiohdr variant of a rig: `(major, layout)` for `rmdl::parse_bones`. `sibling` is the
/// version of a `mdl_` found in the same pak; `studio` is the studiohdr block (for the heuristic).
/// No variant found is `UnsupportedVersion`.
pub fn studio_variant(
    select: &StudioSelect,
    sibling: Option<Version>,
    studio: &[u8],
) -> Result<(u32, StudioLayout)> {
    match select {
        StudioSelect::Layout(name) => match name.as_str() {
            "v16" => Ok((16, StudioLayout::V16)),
            "v17" => Ok((18, StudioLayout::V17)),
            "v19_2" => Ok((20, StudioLayout::V19_2)),
            other => Err(unsupported(format!(
                "studio layout `{other}` (v16, v17 or v19_2)"
            ))),
        },
        StudioSelect::Auto => {
            if let Some(v) = sibling {
                return Ok((v.major, StudioLayout::for_version(v)?));
            }
            let b = reader(studio);
            let unknown = || {
                unsupported("studio layout (no model beside the rig and the block does not say; set `studio`)")
            };
            let word = |at: usize| b.u16(at).map_err(|_| unknown());
            let (marker, linear, bones, bone_hdr) =
                (word(0xD8)?, word(0xCA)?, word(0x74)?, word(0x76)?);
            if marker == 244 {
                Ok((20, StudioLayout::V19_2))
            } else if linear != 0 {
                Ok((19, StudioLayout::V17))
            } else if bones != 0 && bone_hdr != 0 {
                Ok((18, StudioLayout::V17))
            } else {
                Err(unknown())
            }
        }
    }
}

/// A rig: `Data` + `Skeleton` + `Metadata`.
#[derive(Debug)]
pub struct ArigAsset {
    format: FormatId,
    key: ValueKey,
    header: ArigHeader,
    layout: StudioLayout,
    name: Option<String>,
    guid: u64,
    bones: Vec<Bone>,
    anim_seqs: Vec<u64>,
}

impl ArigAsset {
    /// The parsed header.
    pub fn header(&self) -> &ArigHeader {
        &self.header
    }

    /// The studiohdr layout the bones were read with.
    pub fn layout(&self) -> StudioLayout {
        self.layout
    }

    /// The `aseq` guids the rig lists.
    pub fn anim_seqs(&self) -> &[u64] {
        &self.anim_seqs
    }
}

impl Skeleton for ArigAsset {
    fn bones(&self) -> &[Bone] {
        &self.bones
    }
}

impl Metadata for ArigAsset {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("arig")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.anim_seqs
            .iter()
            .map(|&guid| Dependency { guid, name: None })
            .collect()
    }
}

impl Data for ArigAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_skeleton(&self) -> Option<&dyn Skeleton> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// Read edge `rpak.asset -> arig`.
pub struct ArigRead {
    studio: StudioSelect,
    /// The first `mdl_` of each pak (by virtual path of the pak), looked up once.
    first_models: std::sync::Mutex<HashMap<String, Arc<OnceCell<Option<String>>>>>,
}

impl ArigRead {
    /// A reader that finds the studiohdr variant as `studio` says.
    pub fn new(studio: StudioSelect) -> Self {
        Self {
            studio,
            first_models: Default::default(),
        }
    }
}

#[async_trait]
impl Detect for ArigRead {
    /// The asset kind is `arig` = 10, with the asset version as the version.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"arig").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

#[async_trait]
impl ReadHandler for ArigRead {
    /// Reads the header, the studiohdr (bones only; `numlocalseq != 0` is `Corrupt`) and the
    /// `aseq` guid list.
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "arig must be read from an rpak.asset".into(),
            })
        })?;
        if asset.entry.kind != *b"arig" {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: "the asset is not an arig".into(),
            }));
        }
        let header = ArigHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        if header.studio_offset >= mem_len {
            return Err(Error::corrupt(
                "arig: the studio data pointer is outside the pak's data",
            ));
        }
        let studio = asset
            .read_mem(
                header.studio_offset,
                STUDIO_WINDOW.min(mem_len - header.studio_offset),
            )
            .await?;
        let sibling = match self.studio {
            StudioSelect::Auto => {
                let origin = pak_of(source.key());
                let cell = self
                    .first_models
                    .lock()
                    .expect("the model map is not poisoned")
                    .entry(origin.clone())
                    .or_default()
                    .clone();
                let first = cell
                    .get_or_init(|| async {
                        list_assets(&asset.volume, ".mdl_")
                            .await
                            .ok()
                            .and_then(|names| names.into_iter().next())
                    })
                    .await;
                match first {
                    Some(name) => sibling_model(name, &origin, cx).await,
                    None => None,
                }
            }
            StudioSelect::Layout(_) => None,
        };
        let (major, layout) = studio_variant(&self.studio, sibling, &studio)?;
        let b = reader(&studio);
        let local = b.u16(0x7A)?;
        if local != 0 {
            return Err(Error::corrupt(format!(
                "arig has {local} local sequences (a rig has none)"
            )));
        }
        let bones = parse_bones(&b, major, layout)?;
        let name = match header.name_offset {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let anim_seqs = read_guids(asset, header.anim_seqs.0, header.anim_seqs.1).await?;
        Ok(Arc::new(ArigAsset {
            format: FormatId::new("arig").with_version(header.version),
            key: source.key().clone(),
            header,
            layout,
            name,
            guid: asset.entry.guid,
            bones,
            anim_seqs,
        }))
    }
}
