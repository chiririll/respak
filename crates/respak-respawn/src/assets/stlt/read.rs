//! The `rpak.asset -> stlt` read edge.

use super::*;

/// Read edge `rpak.asset -> stlt`.
pub struct StltRead;

#[async_trait]
impl Detect for StltRead {
    /// The asset kind is `stlt`; any asset version other than 0 is `UnsupportedVersion` when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        Ok(detect_kind(input.source, *b"stlt"))
    }
}

/// A header's pointers as `.mem` offsets. The asset header is relocated by the volume; a
/// sub header lives in `.mem`, where a pointer is still the stored `{page, offset}`.
pub(super) struct Pointers<'a> {
    asset: &'a RpakAsset,
    relocated: bool,
}

impl Pointers<'_> {
    fn get(&self, h: &Reader<'_>, at: usize) -> Result<Option<u64>> {
        let slot: [u8; 8] = h.array(at)?;
        if self.relocated {
            let v = u64::from_le_bytes(slot);
            Ok((v != 0 && v != u64::MAX).then_some(v))
        } else {
            // A pointer in `.mem` that was never relocated is all zeros, which would otherwise
            // read as page 0, offset 0.
            Ok((slot != [0; 8])
                .then(|| self.asset.mem_offset_of(slot))
                .flatten())
        }
    }
}

pub(super) async fn cstr(asset: &RpakAsset, at: u64, mem_len: u64) -> Option<String> {
    if at >= mem_len {
        return None;
    }
    let end = (at + 512).min(mem_len);
    let bytes = asset.read_mem(at, end - at).await.ok()?;
    let nul = bytes.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&bytes[..nul]).into_owned())
}

pub(super) fn text(names: &[u8], at: usize) -> String {
    let tail = names.get(at..).unwrap_or_default();
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    String::from_utf8_lossy(&tail[..end]).into_owned()
}

/// Reads the layout whose 72-byte header is `header`.
pub(super) fn read_layout<'a>(
    asset: &'a RpakAsset,
    header: bytes::Bytes,
    relocated: bool,
    mem_len: u64,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<Layout>> + Send + 'a>> {
    Box::pin(async move {
        if depth > DEPTH {
            return Err(Error::corrupt("stlt: sub layouts nest too deep"));
        }
        let h = reader(&header);
        let p = Pointers { asset, relocated };
        let name_at = p.get(&h, 0x00)?;
        let field_data = p.get(&h, 0x08)?;
        let field_map = p.get(&h, 0x10)?;
        let table_size = h.u32(0x18)?;
        let field_count = h.u32(0x1C)?;
        let extra_data_size_index = h.u32(0x20)?;
        let element_count = h.i32(0x2C)?;
        let total_size = h.u32(0x30)?;
        let string_data = p.get(&h, 0x38)?;
        let sub_headers = p.get(&h, 0x40)?;
        if table_size > LIMIT || field_count > LIMIT {
            return Err(Error::corrupt(format!(
                "stlt: implausible table of {table_size} slots / {field_count} fields"
            )));
        }

        let name = match name_at {
            Some(at) => cstr(asset, at, mem_len).await,
            None => None,
        };

        let table = match field_data {
            Some(at) if table_size > 0 => {
                let len = u64::from(table_size) * 8;
                if at.saturating_add(len) > mem_len {
                    return Err(Error::corrupt(
                        "stlt: the field table is outside the pak's data",
                    ));
                }
                asset.read_mem(at, len).await?
            }
            _ => bytes::Bytes::new(),
        };
        let map = match field_map {
            Some(at) if field_count > 0 => {
                let len = u64::from(field_count) * 4;
                if at.saturating_add(len) > mem_len {
                    return Err(Error::corrupt(
                        "stlt: the field map is outside the pak's data",
                    ));
                }
                asset.read_mem(at, len).await?
            }
            _ => bytes::Bytes::new(),
        };
        let names = match string_data {
            Some(at) if at < mem_len => asset.read_mem(at, NAMES_WINDOW.min(mem_len - at)).await?,
            _ => bytes::Bytes::new(),
        };

        // Alignment and the sub layout count come from every slot, empty ones included.
        let mut alignment = 1;
        let mut highest = 0u8;
        let slots = reader(&table);
        for i in 0..table_size as usize {
            let ty = slots.u16(i * 8)?;
            if let Some(a) = FieldType::from_u16(ty).and_then(FieldType::alignment) {
                alignment = alignment.max(a);
            }
            highest = highest.max(slots.get(i * 8 + 7, 1)?[0]);
        }

        let entries = reader(&map);
        let mut fields = Vec::with_capacity(field_count as usize);
        for i in 0..field_count as usize {
            let bucket = entries.u16(i * 4)? as usize;
            let help = entries.u16(i * 4 + 2)? as usize;
            if bucket >= table_size as usize {
                return Err(Error::corrupt(format!(
                    "stlt: field {i} is in bucket {bucket}"
                )));
            }
            let raw_ty = slots.u16(bucket * 8)?;
            let ty = FieldType::from_u16(raw_ty)
                .ok_or_else(|| Error::corrupt(format!("stlt: field type {raw_ty}")))?;
            let tail = slots.u32(bucket * 8 + 4)?;
            fields.push(Field {
                name: text(&names, usize::from(slots.u16(bucket * 8 + 2)?)),
                help: text(&names, help),
                ty,
                offset: tail & 0x00FF_FFFF,
                sub: (tail >> 24) as u8,
            });
        }
        fields.sort_by_key(|f| f.offset);

        let mut subs = Vec::new();
        if let Some(at) = sub_headers {
            let count = usize::from(highest) + 1;
            let len = (count * HEADER_LEN) as u64;
            if at.saturating_add(len) > mem_len {
                return Err(Error::corrupt(
                    "stlt: the sub layouts are outside the pak's data",
                ));
            }
            let block = asset.read_mem(at, len).await?;
            for chunk in block.chunks_exact(HEADER_LEN) {
                subs.push(
                    read_layout(asset, block.slice_ref(chunk), false, mem_len, depth + 1).await?,
                );
            }
        } else if fields.iter().any(|f| f.sub != 0) {
            return Err(Error::corrupt(
                "stlt: a field names a sub layout but there are none",
            ));
        }

        Ok(Layout {
            name,
            fields,
            element_count,
            total_size,
            extra_data_size_index,
            alignment,
            subs,
        })
    })
}

#[async_trait]
impl ReadHandler for StltRead {
    /// Parses the header, the fields and the sub layouts.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = rpak_asset(&source, *b"stlt")?;
        if asset.entry.version != 0 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "stlt".into(),
                version: asset.entry.version.to_string(),
            }));
        }
        let header = asset.read_header().await?;
        if header.len() != HEADER_LEN {
            return Err(Error::corrupt(format!(
                "stlt header is {} bytes, expected {HEADER_LEN}",
                header.len()
            )));
        }
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let layout = read_layout(asset, header, true, mem_len, 0).await?;
        Ok(Arc::new(StltAsset {
            format: FormatId::new("stlt").with_version(Version::new(0)),
            key: source.key().clone(),
            layout,
            guid: asset.entry.guid,
        }))
    }
}
