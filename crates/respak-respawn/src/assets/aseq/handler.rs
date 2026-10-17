use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use respak_core::{
    Cx, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
};

use crate::anim_codec::{self};
use crate::arig::{explicit_rig, owner_of, Owners};
use crate::matl::pak_of;
use crate::rmdl::read_guids;
use crate::rpak::{detect_kind, rpak_asset, MEM};

use super::asset::{AseqAsset, Extra};
use super::*;

/// Read edge `rpak.asset -> aseq`.
pub struct AseqRead {
    rig: RigSelect,
    owners: Owners,
}

impl AseqRead {
    /// A reader that binds sequences to the skeleton `rig` says. An `arig` it finds is read by the
    /// registered `arig` edge, with its `studio` setting.
    pub fn new(rig: RigSelect) -> Self {
        Self {
            rig,
            owners: Owners::default(),
        }
    }
}

#[async_trait]
impl Detect for AseqRead {
    /// The asset kind is `aseq` = 10, with the asset version as the version; the variant is worked
    /// out when the sequence is read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"aseq"))
    }
}

#[async_trait]
impl ReadHandler for AseqRead {
    /// Parses the header, the seqdesc and the animdescs, resolves the rig (`MissingDependency` if
    /// there is none) and checks that the bone weight list fits it.
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"aseq")?;
        let version = variant(asset.entry.version, asset.pak_created_time)?;
        let header = AseqHeader::parse(version, &asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        if header.data_offset >= mem_len {
            return Err(Error::corrupt(
                "aseq: the data pointer is outside the pak's data",
            ));
        }
        let origin = pak_of(source.key());

        let rig = match &self.rig {
            RigSelect::Auto => owner_of(asset.entry.guid, &origin, &self.owners, cx).await?,
            RigSelect::Explicit(name) => explicit_rig(name, &origin, cx).await?,
        };
        let skeleton = rig.as_skeleton().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "the rig has no skeleton".into(),
            })
        })?;
        let bones = skeleton.bones().to_vec();
        let rig_guid = rig.as_metadata().and_then(|m| m.guid()).unwrap_or(0);

        let block = asset
            .read_mem(
                header.data_offset,
                SEQ_WINDOW.min(mem_len - header.data_offset),
            )
            .await?;
        let seq = SeqDesc::parse(&block, 0, bones.len())?;
        let anims = seq
            .anim_offsets
            .iter()
            .map(|&off| AnimDesc::parse(&block, off as usize))
            .collect::<Result<Vec<_>>>()?;

        let name = match header.name_offset {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let models = read_guids(asset, header.models.0, header.models.1).await?;
        let settings = read_guids(asset, header.settings.0, header.settings.1).await?;
        let mut dependencies: Vec<u64> = Vec::new();
        for &guid in anims
            .iter()
            .map(|a| &a.asqd)
            .chain(&seq.autolayers)
            .chain(&models)
            .chain(&settings)
        {
            if guid != 0 && !dependencies.contains(&guid) {
                dependencies.push(guid);
            }
        }

        let extra = match header.extra {
            (Some(at), len) if len > 0 => Some(Extra {
                volume: asset.volume.clone(),
                path: MEM.to_owned(),
                base: at,
                len: u64::from(len),
            }),
            (None, len) if len > 0 => asset.stream.clone().map(|path| Extra {
                volume: asset.volume.clone(),
                path,
                base: 0,
                len: u64::from(len),
            }),
            _ => None,
        };
        let default_euler = bones
            .iter()
            .map(|b| anim_codec::quat_to_euler(b.rotation))
            .collect();
        Ok(Arc::new(AseqAsset {
            format: FormatId::new("aseq").with_version(version),
            key: source.key().clone(),
            width: flag_width(version),
            header,
            seq,
            anims,
            name,
            guid: asset.entry.guid,
            bones,
            default_euler,
            rig: rig_guid,
            dependencies,
            origin,
            block,
            extra,
            asqds: Mutex::new(HashMap::new()),
            sections: Mutex::new(HashMap::new()),
        }))
    }
}
