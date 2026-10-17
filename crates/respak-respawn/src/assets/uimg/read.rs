//! The `rpak.asset -> uimg` read edge.

use super::*;

/// `len` bytes of `.mem` at `at`, which must lie inside it.
pub(super) async fn mem(
    volume: &Operator,
    what: &str,
    at: Option<u64>,
    len: u64,
    mem_len: u64,
) -> Result<Bytes> {
    let at = at.ok_or_else(|| Error::corrupt(format!("uimg has no {what}")))?;
    if at.checked_add(len).is_none_or(|end| end > mem_len) {
        return Err(Error::corrupt(format!(
            "uimg {what} runs past the end of the pak's pages"
        )));
    }
    Ok(volume.read_with(MEM).range(at..at + len).await?.to_bytes())
}

/// The NUL-terminated string at `at` in the names blob.
pub(super) fn cstr(blob: &[u8], at: usize) -> Option<String> {
    let tail = blob.get(at..)?;
    let nul = tail.iter().position(|&b| b == 0)?;
    (nul > 0).then(|| String::from_utf8_lossy(&tail[..nul]).into_owned())
}

/// Read edge `rpak.asset -> uimg`.
pub struct UimgRead;

#[async_trait]
impl Detect for UimgRead {
    /// The asset kind is `uimg` (a container-provided kind) = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"uimg").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

#[async_trait]
impl ReadHandler for UimgRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "uimg must be read from an rpak.asset".into(),
            })
        })?;
        let header = UimgHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let n = u64::from(header.count);
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let v = &asset.volume;

        let offsets = mem(v, "offsets", header.offsets, n * OFFSET_LEN, mem_len).await?;
        let dims = mem(
            v,
            "dimensions",
            header.dimensions,
            n * DIMENSION_LEN,
            mem_len,
        )
        .await?;
        let hashes = mem(
            v,
            "hashes",
            header.hashes,
            n * header.hash_stride(),
            mem_len,
        )
        .await?;
        let bounds = mem(v, "bounds", asset.data_mem_offset, n * BOUNDS_LEN, mem_len).await?;

        // (hash, offset into the names blob) per sprite.
        let stride = header.hash_stride() as usize;
        let entries: Vec<(u32, usize)> = hashes
            .chunks_exact(stride)
            .map(|e| {
                let e = Reader::new("uimg", e);
                let at = if header.version == 10 {
                    u32::from(e.u16(6)?)
                } else {
                    e.u32(4)?
                };
                Ok((e.u32(0)?, at as usize))
            })
            .collect::<Result<_>>()?;
        let (offsets_r, dims_r, bounds_r) = (
            Reader::new("uimg", &offsets),
            Reader::new("uimg", &dims),
            Reader::new("uimg", &bounds),
        );
        // One read for all names: up to the last start plus the longest path.
        let names = match header.names {
            Some(at) if !entries.is_empty() => {
                let reach = entries.iter().map(|e| e.1 as u64).max().unwrap_or(0) + NAME_MAX;
                let len = reach.min(mem_len.saturating_sub(at));
                Some(mem(v, "names", Some(at), len, mem_len).await?)
            }
            _ => None,
        };

        let mut missing = 0;
        let sprites = entries
            .iter()
            .enumerate()
            .map(|(i, &(hash, at))| {
                let named = names.as_deref().and_then(|blob| cstr(blob, at));
                missing += usize::from(named.is_none());
                Ok(Sprite {
                    named: named.is_some(),
                    path: named.unwrap_or_else(|| format!("0x{hash:08X}")),
                    hash,
                    uv: bounds_r.floats(i * 16)?,
                    layout: offsets_r.floats(i * 32)?,
                    render: [dims_r.u16(i * 4)?, dims_r.u16(i * 4 + 2)?],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if missing > 0 && names.is_some() {
            cx.warn(format!(
                "uimg {:#018x}: {missing} sprite name(s) are not readable; their hashes are used",
                asset.entry.guid
            ));
        }

        let info = BitmapInfo {
            width: header.width,
            height: header.height,
            depth: 1,
            array_len: 1,
            faces: 1,
            mip_count: 1,
            format: PixelFormat::Rgba8,
        };
        let format = FormatId::new("uimg").with_version(Version::new(header.version));
        Ok(Arc::new(UimgAsset {
            format,
            key: source.key().clone(),
            header,
            sprites,
            guid: asset.entry.guid,
            info,
            origin: pak_of(source.key()),
            cx: cx.clone(),
        }))
    }
}
