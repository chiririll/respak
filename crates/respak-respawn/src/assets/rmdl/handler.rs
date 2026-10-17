//! `rpak.asset -> rmdl`: detection and reading.

use std::sync::Arc;

use async_trait::async_trait;
use respak_core::{Cx, Detect, Detected, Error, FormatId, Probe, ReadHandler, Result, Value};

use super::asset::RmdlAsset;
use super::block::reader;
use super::header::{RmdlHeader, CPU_LEN, MAX_STUDIO_LEN};
use super::studio::Studio;
use super::unsupported;
use crate::rpak::{detect_kind, rpak_asset, RpakAsset, MEM};

const MAX_GUIDS: usize = 4096;

/// Read edge `rpak.asset -> rmdl`.
pub struct RmdlRead;

#[async_trait]
impl Detect for RmdlRead {
    /// The asset kind is `mdl_` (a container-provided kind) = 10, with the asset version as the
    /// version; the variant is worked out when the model is read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"mdl_"))
    }
}

/// `count` guids at `.mem` offset `at`.
pub(crate) async fn read_guids(asset: &RpakAsset, at: Option<u64>, count: u32) -> Result<Vec<u64>> {
    let (Some(at), true) = (at, count > 0) else {
        return Ok(Vec::new());
    };
    if count as usize > MAX_GUIDS {
        return Err(Error::corrupt(format!(
            "rmdl lists {count} animation assets"
        )));
    }
    let bytes = asset.read_mem(at, u64::from(count) * 8).await?;
    Ok(bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes")))
        .collect())
}

#[async_trait]
impl ReadHandler for RmdlRead {
    /// Parses the headers, the bones and the body part tables. Vertex data is read later, by `Mesh::read`.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"mdl_")?;
        let major = asset.entry.version;
        if !(16..=20).contains(&major) {
            return Err(unsupported(major.to_string()));
        }
        let header_bytes = asset.read_header().await?;
        let data_at = asset
            .data_mem_offset
            .ok_or_else(|| Error::corrupt("rmdl has no CPU data pointer"))?;
        let cpu = asset.read_mem(data_at, CPU_LEN as u64).await?;
        let studio_at = reader(&header_bytes).u64(0)?;
        let studio_len = u64::from(reader(&cpu).u32(12)?);
        if studio_len == 0 || studio_len > MAX_STUDIO_LEN {
            return Err(Error::corrupt(format!(
                "rmdl studio block of {studio_len} bytes is not usable"
            )));
        }
        let studio_blob = asset.read_mem(studio_at, studio_len).await?;
        let source_filename_offset = reader(&studio_blob).u16(0xD8).ok();
        let header = RmdlHeader::parse(
            major,
            &header_bytes,
            &cpu,
            asset.pak_created_time,
            source_filename_offset,
        )?;
        let studio = Studio::parse(major, header.layout, &studio_blob)?;

        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let name = match header.name_offset {
            Some(at) => crate::txtr::read_name(&asset.volume, at, mem_len).await,
            None => None,
        };
        let anim_rigs = read_guids(asset, header.anim_rigs.0, header.anim_rigs.1).await?;
        let anim_seqs = read_guids(asset, header.anim_seqs.0, header.anim_seqs.1).await?;
        let format = FormatId::new("rmdl").with_version(header.version);
        Ok(Arc::new(RmdlAsset {
            format,
            key: source.key().clone(),
            header,
            studio,
            studio_blob,
            name,
            guid: asset.entry.guid,
            anim_rigs,
            anim_seqs,
            volume: asset.volume.clone(),
            stream: asset.stream.clone(),
        }))
    }
}
