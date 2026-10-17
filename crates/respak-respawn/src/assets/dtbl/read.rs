//! The `rpak.asset -> dtbl` read edge.

use super::*;

/// `len` bytes of `.mem` at `at`, `Corrupt` (naming `what`) when they leave it.
pub(super) async fn read_block(
    volume: &Operator,
    at: Option<u64>,
    len: u64,
    mem_len: u64,
    what: &str,
) -> Result<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let at = at.ok_or_else(|| Error::corrupt(format!("dtbl: the {what} pointer is null")))?;
    match at.checked_add(len) {
        Some(end) if end <= mem_len => Ok(volume.read_with(MEM).range(at..end).await?.to_vec()),
        _ => Err(Error::corrupt(format!(
            "dtbl: {len} bytes of {what} at {at:#x} are outside the pak's data"
        ))),
    }
}

/// The NUL-terminated string at `.mem` offset `at`; `Corrupt` when it does not end inside `.mem`.
pub(super) async fn read_cstr(volume: &Operator, at: u64, mem_len: u64) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = at;
    let mut chunk = 256;
    while pos < mem_len && pos - at < MAX_STRING {
        let end = (pos + chunk).min(mem_len);
        let bytes = volume.read_with(MEM).range(pos..end).await?.to_vec();
        if let Some(nul) = bytes.iter().position(|&b| b == 0) {
            out.extend_from_slice(&bytes[..nul]);
            return Ok(out);
        }
        out.extend_from_slice(&bytes);
        pos = end;
        chunk *= 2;
    }
    Err(Error::corrupt(format!(
        "dtbl: the string at {at:#x} does not end inside the pak's data"
    )))
}

/// The string a `.mem` pointer slot points to (an empty string for a null pointer).
pub(super) async fn read_string(
    asset: &RpakAsset,
    slot: &[u8],
    mem_len: u64,
    what: &str,
) -> Result<String> {
    let slot: [u8; 8] = slot.try_into().expect("8 bytes");
    if slot == [0; 8] {
        return Ok(String::new());
    }
    let at = asset.mem_offset_of(slot).ok_or_else(|| {
        Error::corrupt(format!("dtbl: the {what} points outside the pak's pages"))
    })?;
    let bytes = read_cstr(&asset.volume, at, mem_len).await?;
    if bytes.first() == Some(&0x0f) {
        return Ok(DATA_EXCLUDED.to_owned());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Read edge `rpak.asset -> dtbl`.
pub struct DtblRead;

#[async_trait]
impl Detect for DtblRead {
    /// The asset kind is `dtbl` = 10; asset versions other than 0 and 1 are `UnsupportedVersion`
    /// when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"dtbl").then(|| Detected {
            score: 10,
            version: variant(asset.entry.version, asset.pak_created_time).ok(),
        }))
    }
}

#[async_trait]
impl ReadHandler for DtblRead {
    /// Reads the whole table: column names, rows and the strings they point to.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "dtbl must be read from an rpak.asset".into(),
            })
        })?;
        if asset.entry.kind != *b"dtbl" {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: "the asset is not a dtbl".into(),
            }));
        }
        let version = variant(asset.entry.version, asset.pak_created_time)?;
        let header = DtblHeader::parse(version, &asset.read_header().await?)?;
        let volume = &asset.volume;
        let mem_len = volume.stat(MEM).await?.content_length();

        let col_size = header.column_size();
        let records = read_block(
            volume,
            header.columns,
            u64::from(header.num_columns) * col_size,
            mem_len,
            "columns",
        )
        .await?;
        let (type_at, offset_at) = if col_size == 0x18 {
            (0x10, 0x14)
        } else {
            (0x08, 0x0C)
        };
        let mut columns = Vec::with_capacity(header.num_columns as usize);
        let mut offsets = Vec::with_capacity(header.num_columns as usize);
        for record in records.chunks_exact(col_size as usize) {
            let b = reader(record);
            let index = columns.len();
            let raw = b.i32(type_at)?;
            let ty = column_type(raw).ok_or_else(|| {
                Error::corrupt(format!("dtbl: column {index} has unknown type {raw}"))
            })?;
            let offset = u32::try_from(b.i32(offset_at)?).ok().filter(|&o| {
                o.checked_add(cell_size(ty))
                    .is_some_and(|end| end <= header.row_stride)
            });
            let offset = offset.ok_or_else(|| {
                Error::corrupt(format!(
                    "dtbl: column {index} is outside the row stride {}",
                    header.row_stride
                ))
            })?;
            let name = read_string(asset, &record[..8], mem_len, "column name").await?;
            columns.push(Column { name, ty });
            offsets.push(offset as usize);
        }

        let stride = header.row_stride as usize;
        let block = read_block(
            volume,
            header.rows,
            u64::from(header.num_rows) * u64::from(header.row_stride),
            mem_len,
            "rows",
        )
        .await?;
        let mut rows = Vec::with_capacity(header.num_rows as usize);
        for i in 0..header.num_rows as usize {
            let row = reader(&block[i * stride..(i + 1) * stride]);
            let mut cells = Vec::with_capacity(columns.len());
            for (column, &at) in columns.iter().zip(&offsets) {
                cells.push(match column.ty {
                    ColumnType::Bool => Cell::Bool(row.u8(at)? != 0),
                    ColumnType::Int => Cell::Int(row.i32(at)?),
                    ColumnType::Float => Cell::Float(row.f32(at)?),
                    ColumnType::Vector => Cell::Vector(row.floats::<3>(at)?),
                    ColumnType::String | ColumnType::Asset | ColumnType::AssetNoPrecache => {
                        Cell::String(
                            read_string(asset, row.get(at, 8)?, mem_len, "string cell").await?,
                        )
                    }
                });
            }
            rows.push(cells);
        }

        tracing::debug!(guid = %hex(asset.entry.guid), columns = columns.len(), rows = rows.len(), "dtbl read");
        Ok(Arc::new(DtblAsset {
            format: FormatId::new("dtbl").with_version(version),
            key: source.key().clone(),
            header,
            guid: asset.entry.guid,
            columns,
            rows,
        }))
    }
}
