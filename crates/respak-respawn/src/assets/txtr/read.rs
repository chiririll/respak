//! The `rpak.asset -> txtr` read edge.

use super::*;

/// Read edge `rpak.asset -> txtr`.
pub struct TxtrRead;

#[async_trait]
impl Detect for TxtrRead {
    /// The asset kind is `txtr` (a container-provided kind) = 10.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"txtr"))
    }
}

/// The NUL-terminated name at `.mem` offset `at`, if it fits in 512 bytes.
pub(crate) async fn read_name(volume: &Operator, at: u64, mem_len: u64) -> Option<String> {
    if at >= mem_len {
        return None;
    }
    let end = (at + 512).min(mem_len);
    let bytes = volume.read_with(MEM).range(at..end).await.ok()?.to_vec();
    let nul = bytes.iter().position(|&b| b == 0)?;
    let name = String::from_utf8_lossy(&bytes[..nul]).into_owned();
    (!name.is_empty()).then_some(name)
}

/// The length of `path` in the volume. Errors the volume raised itself (`MissingDependency`)
/// come back as they were.
pub(super) async fn stat_len(volume: &Operator, path: &str) -> Result<u64> {
    Ok(volume.stat(path).await?.content_length())
}

#[async_trait]
impl ReadHandler for TxtrRead {
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"txtr")?;
        let header = TxtrHeader::parse(asset.entry.version, &asset.read_header().await?)?;
        let pixel = pixel_format(header.format_index).ok_or_else(|| {
            unsupported(format!(
                "{} (pixel format #{})",
                header.version, header.format_index
            ))
        })?;

        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let data_offset = asset
            .data_mem_offset
            .ok_or_else(|| Error::corrupt("txtr has no data pointer"))?;

        let name = match header.name_offset {
            Some(at) => read_name(&asset.volume, at, mem_len).await,
            None => None,
        }
        .map(|n| {
            if n.to_ascii_lowercase().starts_with("texture/") {
                n
            } else {
                format!("texture/{n}")
            }
        });
        let label = name.as_deref().unwrap_or("(unnamed)");

        // The permanent mips (shifts `streamed + optional ..`), smallest first in the CPU data.
        let first_permanent = header.first_permanent_shift();
        let total = header.total_mips();
        let mut cursor = 0u64;
        let mut by_shift = Vec::new();
        for shift in (first_permanent..total).rev() {
            let len = pixel.mip_len(
                (header.width >> shift).max(1),
                (header.height >> shift).max(1),
            );
            let stride = len.div_ceil(16).max(1) * 16;
            by_shift.push((
                shift,
                MipLoc {
                    store: Store::Mem,
                    offset: cursor,
                    stride,
                    len,
                    codec: 0,
                },
            ));
            cursor += stride * u64::from(header.array_size);
        }
        if data_offset
            .checked_add(cursor)
            .is_none_or(|end| end > mem_len)
        {
            return Err(Error::corrupt(
                "txtr mips run past the end of the pak's pages",
            ));
        }

        // The streamed mips. A starpak that is missing, or has no entry for this asset, makes the
        // mandatory mips `MissingDependency`; the optional ones are dropped with a warning.
        if header.streamed > 0 {
            let path = asset.stream.as_deref().ok_or_else(|| {
                Error::new(ErrorKind::MissingDependency {
                    item: label.to_owned(),
                    file: "(no mandatory starpak reference)".into(),
                })
            })?;
            let size = match stat_len(&asset.volume, path).await {
                Ok(n) => n,
                Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => {
                    return Err(Error::new(ErrorKind::MissingDependency {
                        item: label.to_owned(),
                        file: format!("{path} (no entry in the starpak)"),
                    }));
                }
                Err(e) => return Err(e),
            };
            let mips = layout_entry(
                &header,
                pixel,
                Store::Stream,
                header.optional..header.optional + header.streamed,
            )?;
            if entry_len(&header, &mips) > size {
                return Err(Error::corrupt(format!("texture {label}: its streamed mips do not fit the starpak entry ({size} bytes)")));
            }
            by_shift.extend(mips);
        }
        let mut first = 0;
        if header.optional > 0 {
            let usable = match asset.opt.as_deref() {
                None => Err("no optional starpak reference".to_owned()),
                Some(path) => match stat_len(&asset.volume, path).await {
                    Ok(n) => Ok(n),
                    Err(e)
                        if matches!(
                            e.kind(),
                            ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
                        ) =>
                    {
                        Err(format!("{path} is not available ({e})"))
                    }
                    Err(e) => return Err(e),
                },
            };
            match usable {
                Ok(size) => {
                    let mips = layout_entry(&header, pixel, Store::Opt, 0..header.optional)?;
                    if entry_len(&header, &mips) > size {
                        return Err(Error::corrupt(format!("texture {label}: its optional mips do not fit the starpak entry ({size} bytes)")));
                    }
                    by_shift.extend(mips);
                }
                Err(why) => {
                    first = header.optional;
                    cx.warn(format!(
                        "texture {label}: {} optional mip(s) are skipped: {why}",
                        header.optional
                    ));
                }
            }
        }

        // Level 0 is the largest available mip.
        by_shift.sort_by_key(|(shift, _)| *shift);
        let mips: Vec<MipLoc> = by_shift.into_iter().map(|(_, m)| m).collect();
        let info = BitmapInfo {
            width: (header.width >> first).max(1),
            height: (header.height >> first).max(1),
            depth: 1,
            array_len: header.array_size,
            faces: 1,
            mip_count: total - first,
            format: pixel,
        };
        let format = FormatId::new("txtr").with_version(Version::new(header.version));
        Ok(Arc::new(TxtrAsset {
            format,
            key: source.key().clone(),
            header,
            name,
            guid: asset.entry.guid,
            info,
            mips,
            volume: asset.volume.clone(),
            data_offset,
            stream: asset.stream.clone(),
            opt: asset.opt.clone(),
        }))
    }
}
