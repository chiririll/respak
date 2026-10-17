//! The `rpak.asset -> uiia` read edge.

use super::*;

/// Read edge `rpak.asset -> uiia`.
pub struct UiiaRead;

#[async_trait]
impl Detect for UiiaRead {
    /// The asset kind is `uiia` (a container-provided kind) = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"uiia").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

/// Whether the high resolution can be read: it lives in the starpak, and without it the low one
/// is all there is.
pub(super) async fn pick_quality(
    asset: &RpakAsset,
    header: &UiiaHeader,
    label: &str,
    cx: &Cx,
) -> Result<Quality> {
    let Some(path) = asset.stream.as_deref().filter(|_| header.streamed_size > 0) else {
        return Ok(Quality::Low);
    };
    match asset.volume.stat(path).await {
        Ok(meta) if meta.content_length() >= header.streamed_size => Ok(Quality::High),
        Ok(_) => Err(Error::corrupt(format!(
            "uiia {label}: its streamed data does not fit the starpak entry"
        ))),
        Err(e) => {
            let e = Error::from(e);
            if matches!(
                e.kind(),
                ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
            ) {
                cx.warn(format!(
                    "uiia {label}: the high resolution is skipped: {path} is not available ({e})"
                ));
                Ok(Quality::Low)
            } else {
                Err(e)
            }
        }
    }
}

#[async_trait]
impl ReadHandler for UiiaRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "uiia must be read from an rpak.asset".into(),
            })
        })?;
        let data_offset = asset
            .data_mem_offset
            .ok_or_else(|| Error::corrupt("uiia has no CPU data"))?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        if data_offset
            .checked_add(CPU_LEN)
            .is_none_or(|end| end > mem_len)
        {
            return Err(Error::corrupt(
                "uiia CPU data runs past the end of the pak's pages",
            ));
        }
        let cpu = asset.read_mem(data_offset, CPU_LEN).await?;
        let header = UiiaHeader::parse(asset.entry.version, &asset.read_header().await?, &cpu)?;

        // Pointers inside page data are still `{page, offset}` pairs.
        let slot = |at: usize| asset.mem_offset_of(cpu[at..at + 8].try_into().expect("8 bytes"));
        let pixels = slot(0x28).ok_or_else(|| Error::corrupt("uiia has no image data"))?;
        let name = match slot(0x30) {
            Some(at) => read_name(&asset.volume, at, mem_len).await,
            None => None,
        }
        .map(|n| {
            if n.starts_with("ui_image/") {
                n
            } else {
                format!("ui_image/{n}")
            }
        });
        let label = name.as_deref().unwrap_or("(unnamed)");

        let quality = pick_quality(asset, &header, label, cx).await?;
        let res = if quality == Quality::High {
            header.high
        } else {
            header.low
        };
        let info = BitmapInfo {
            width: res.width,
            height: res.height,
            depth: 1,
            array_len: 1,
            faces: 1,
            mip_count: 1,
            format: PixelFormat::Rgba8,
        };
        let format = FormatId::new("uiia").with_version(Version::new(header.version));
        Ok(Arc::new(UiiaAsset {
            format,
            key: source.key().clone(),
            header,
            name,
            guid: asset.entry.guid,
            quality,
            info,
            volume: asset.volume.clone(),
            data_offset: pixels,
            stream: asset.stream.clone(),
        }))
    }
}
