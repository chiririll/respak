//! The `rpak.asset -> stgs` read edge.

use super::*;

/// Read edge `rpak.asset -> stgs`.
pub struct StgsRead;

#[async_trait]
impl Detect for StgsRead {
    /// The asset kind is `stgs`; the header size, not the version, picks the layout.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"stgs").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

/// The NUL-terminated string at `.mem` offset `at` (at most 4 KiB), lossy UTF-8.
pub(super) async fn string_at(asset: &RpakAsset, at: u64, mem_len: u64) -> Result<String> {
    if at >= mem_len {
        return Err(Error::corrupt("stgs: a string is outside the pak's data"));
    }
    let end = (at + 4096).min(mem_len);
    let bytes = asset.read_mem(at, end - at).await?;
    let nul = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::corrupt("stgs: a string has no end"))?;
    Ok(String::from_utf8_lossy(&bytes[..nul]).into_owned())
}

/// The `stlt` the header names, `None` if it is not in any pak that is searched.
pub(super) async fn find_layout(guid: u64, source: &Value, cx: &Cx) -> Result<Option<Value>> {
    if guid == 0 {
        return Ok(None);
    }
    let found = match locate(guid, "stlt", &pak_of(source.key()), cx).await {
        Ok(asset) => asset,
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
            ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e),
    };
    cx.mediator
        .identify_as(found, &FormatId::new("stlt"), cx)
        .await
        .map(Some)
}

/// The checked count of a table in `.mem`.
pub(super) fn entry_count(n: u32) -> Result<u64> {
    let n = u64::from(n);
    if n > LIMIT {
        return Err(Error::corrupt(format!("stgs: implausible entry count {n}")));
    }
    Ok(n)
}

/// The value buffer the layout is applied to (empty when the asset has none).
pub(super) async fn read_values(
    asset: &RpakAsset,
    header: &StgsHeader,
    mem_len: u64,
) -> Result<Bytes> {
    match header.values {
        Some(at) if header.values_size > 0 => {
            let len = u64::from(header.values_size);
            if len > LIMIT || at.saturating_add(len) > mem_len {
                return Err(Error::corrupt(
                    "stgs: the value buffer is outside the pak's data",
                ));
            }
            asset.read_mem(at, len).await
        }
        _ => Ok(Bytes::new()),
    }
}

/// The names of the mods, one per `{page, offset}` slot.
pub(super) async fn read_mod_names(
    asset: &RpakAsset,
    header: &StgsHeader,
    mem_len: u64,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if let Some(at) = header.mod_names {
        let n = entry_count(header.mod_name_count)?;
        if at.saturating_add(8 * n) > mem_len {
            return Err(Error::corrupt(
                "stgs: the mod names are outside the pak's data",
            ));
        }
        let table = asset.read_mem(at, 8 * n).await?;
        for slot in table.chunks_exact(8) {
            // Pointers inside `.mem` are still stored as `{page, offset}`.
            let text = match asset.mem_offset_of(slot.try_into().expect("8 bytes")) {
                Some(p) => string_at(asset, p, mem_len).await?,
                None => String::new(),
            };
            names.push(text);
        }
    }
    Ok(names)
}

/// The mod entries, with the text of the string ones.
pub(super) async fn read_mods(
    asset: &RpakAsset,
    header: &StgsHeader,
    mem_len: u64,
) -> Result<Vec<StgsMod>> {
    let mut mods = Vec::new();
    if let Some(at) = header.mod_values {
        let n = entry_count(header.mod_value_count)?;
        let bytes = MOD_LEN as u64 * n;
        if at.saturating_add(bytes) > mem_len {
            return Err(Error::corrupt("stgs: the mods are outside the pak's data"));
        }
        let table = asset.read_mem(at, bytes).await?;
        let b = reader(&table);
        for i in 0..n as usize {
            let base = i * MOD_LEN;
            let kind = ModType::from_u16(b.u16(base + 2)?);
            let raw = b.u32(base + 8)?;
            let string = match (kind, header.strings) {
                (Some(ModType::String), Some(s)) => {
                    Some(string_at(asset, s + u64::from(raw), mem_len).await?)
                }
                _ => None,
            };
            mods.push(StgsMod {
                name_index: b.u16(base)?,
                kind,
                value_offset: b.u32(base + 4)?,
                raw,
                string,
            });
        }
    }
    Ok(mods)
}

/// Decodes the value buffer with `layout`, resolving the text values through `.mem`.
pub(super) async fn decode_settings(
    asset: &RpakAsset,
    layout: &Layout,
    values: &Bytes,
    mem_len: u64,
) -> Result<Json> {
    // First pass collects the pointers of the text values, the second uses their text.
    let view = reader(values);
    let mut slots: Vec<[u8; 8]> = Vec::new();
    decode(
        layout,
        &view,
        0,
        &mut |s| {
            slots.push(s);
            None
        },
        0,
    )?;
    let mut texts: HashMap<[u8; 8], String> = HashMap::new();
    for slot in slots {
        if let Some(at) = asset.mem_offset_of(slot) {
            if let Ok(text) = string_at(asset, at, mem_len).await {
                texts.insert(slot, text);
            }
        }
    }
    decode(layout, &view, 0, &mut |s| texts.get(&s).cloned(), 0)
}

#[async_trait]
impl ReadHandler for StgsRead {
    /// Parses the header, the name, the value buffer and the mods, and decodes the values when the
    /// layout is found.
    async fn read(&self, source: Value, _detected: &Detected, cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "stgs must be read from an rpak.asset".into(),
            })
        })?;
        if asset.entry.kind != *b"stgs" {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: "the asset is not a stgs".into(),
            }));
        }
        let header = StgsHeader::parse(&asset.read_header().await?)?;
        let mem_len = asset.volume.stat(MEM).await?.content_length();

        let name = match header.name {
            Some(at) => string_at(asset, at, mem_len)
                .await
                .ok()
                .filter(|n| !n.is_empty()),
            None => None,
        };
        let values = read_values(asset, &header, mem_len).await?;
        let mod_names = read_mod_names(asset, &header, mem_len).await?;
        let mods = read_mods(asset, &header, mem_len).await?;

        let mut layout_name = None;
        let mut settings = None;
        if let Some(stlt) = find_layout(header.layout, &source, cx).await? {
            let stlt = stlt
                .downcast_ref::<StltAsset>()
                .ok_or_else(|| Error::corrupt("stgs: the layout is not a stlt"))?;
            layout_name = stlt.layout().name.clone();
            settings = Some(decode_settings(asset, stlt.layout(), &values, mem_len).await?);
        }

        Ok(Arc::new(StgsAsset {
            format: FormatId::new("stgs").with_version(Version::new(header.version)),
            key: source.key().clone(),
            header,
            name,
            guid: asset.entry.guid,
            values,
            mod_names,
            mods,
            layout_name,
            settings,
        }))
    }
}
