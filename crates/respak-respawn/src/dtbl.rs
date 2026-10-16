//! `dtbl`: a datatable, read from `rpak.asset`. Apex Legends, asset versions 0 and 1. Typed columns,
//! rows of fixed stride; exposed as the `Table` capability.
//!
//! Ported from rsx `src/game/rtech/assets/datatable.{h,cpp}` (`DatatableAssetHeader_v0_t`,
//! `DatatableAssetHeader_v1_t`, `DatatableAssetColumn_v0_t`, `DatatableAssetColumn_v1_1_t`).
//!
//! | field | V0 (0x20) | V1 / V1.1 (0x28) |
//! |---|---|---|
//! | numColumns / numRows (i32) | 0x00 / 0x04 | same |
//! | columns / rows (ptr) | 0x08 / 0x10 | same |
//! | rowStride (i32) | 0x18 | 0x20 |
//!
//! A column is `{ptr name, i32 type, i32 rowOffset}` (0x10), or in V1.1 `{ptr name, u8[8],
//! i32 type, i32 rowOffset}` (0x18). V1.1 is asset version 1 in a pak created between
//! 2020-09-25 and 2024-04-25 (exclusive). rsx checks no bounds; here anything outside the pak's
//! data or the row stride is `Corrupt`.

use std::sync::Arc;

use async_trait::async_trait;
use opendal::Operator;
use respak_core::capability::{Cell, Column, ColumnType, Dependency, Metadata, Table};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value, ValueKey, Version,
};

use crate::matl_json::hex;
use crate::rmdl::B;
use crate::rpak::{RpakAsset, MEM};

/// Pak `createdTime` bounds (exclusive) of the V1.1 column layout.
const CREATED_V1_1_FROM: u64 = 0x1d6_92d8_9727_5335;
const CREATED_V1_1_TO: u64 = 0x1da_975b_0a10_6ef2;

/// What a string cell reads as when the data was stripped from the pak (first byte `0x0f`).
pub const DATA_EXCLUDED: &str = "!!DATA EXCLUDED!!";

/// The longest string a cell or column name may have.
const MAX_STRING: u64 = 1 << 20;

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// The layout variant: `0`, `1` or `1.1` (see the module docs).
pub fn variant(asset_version: u32, created_time: u64) -> Result<Version> {
    match asset_version {
        0 => Ok(Version::new(0)),
        1 if created_time > CREATED_V1_1_FROM && created_time < CREATED_V1_1_TO => Ok(Version::with_minor(1, 1)),
        1 => Ok(Version::new(1)),
        other => Err(Error::new(ErrorKind::UnsupportedVersion { format: "dtbl".into(), version: other.to_string() })),
    }
}

/// The parsed asset header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtblHeader {
    /// The layout variant.
    pub version: Version,
    /// Number of columns.
    pub num_columns: u32,
    /// Number of rows.
    pub num_rows: u32,
    /// Where the column records are in `.mem`, `None` if null.
    pub columns: Option<u64>,
    /// Where the rows are in `.mem`, `None` if null.
    pub rows: Option<u64>,
    /// Bytes per row.
    pub row_stride: u32,
}

impl DtblHeader {
    /// Parses a relocated header of variant `version`; a length other than the variant's
    /// (0x20 or 0x28) or a negative count is `Corrupt`.
    pub fn parse(version: Version, h: &[u8]) -> Result<Self> {
        let (len, stride_at) = if version.major == 0 { (0x20, 0x18) } else { (0x28, 0x20) };
        if h.len() != len {
            return Err(corrupt(format!("dtbl@{version} header is {} bytes, expected {len}", h.len())));
        }
        let b = B(h);
        let count = |at: usize, what: &str| -> Result<u32> {
            u32::try_from(b.i32(at)?).map_err(|_| corrupt(format!("dtbl: negative {what}")))
        };
        let ptr = |at: usize| -> Result<Option<u64>> { b.u64(at).map(|p| (p != 0 && p != u64::MAX).then_some(p)) };
        Ok(Self {
            version,
            num_columns: count(0x00, "column count")?,
            num_rows: count(0x04, "row count")?,
            columns: ptr(0x08)?,
            rows: ptr(0x10)?,
            row_stride: count(stride_at, "row stride")?,
        })
    }

    /// Bytes per column record.
    pub fn column_size(&self) -> u64 {
        if self.version == Version::with_minor(1, 1) {
            0x18
        } else {
            0x10
        }
    }
}

/// The size of a cell of type `ty` in a row.
fn cell_size(ty: ColumnType) -> u32 {
    match ty {
        ColumnType::Bool => 1,
        ColumnType::Int | ColumnType::Float => 4,
        ColumnType::Vector => 12,
        ColumnType::String | ColumnType::Asset | ColumnType::AssetNoPrecache => 8,
    }
}

fn column_type(raw: i32) -> Option<ColumnType> {
    Some(match raw {
        0 => ColumnType::Bool,
        1 => ColumnType::Int,
        2 => ColumnType::Float,
        3 => ColumnType::Vector,
        4 => ColumnType::String,
        5 => ColumnType::Asset,
        6 => ColumnType::AssetNoPrecache,
        _ => return None,
    })
}

/// A `dtbl`: `Data` + `Table` + `Metadata`.
#[derive(Debug)]
pub struct DtblAsset {
    format: FormatId,
    key: ValueKey,
    header: DtblHeader,
    guid: u64,
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
}

impl DtblAsset {
    /// The parsed header.
    pub fn header(&self) -> &DtblHeader {
        &self.header
    }
}

impl Table for DtblAsset {
    fn columns(&self) -> &[Column] {
        &self.columns
    }
    fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }
}

impl Metadata for DtblAsset {
    fn name(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> Option<&str> {
        Some("dtbl")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for DtblAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_table(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

/// `len` bytes of `.mem` at `at`, `Corrupt` (naming `what`) when they leave it.
async fn read_block(volume: &Operator, at: Option<u64>, len: u64, mem_len: u64, what: &str) -> Result<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let at = at.ok_or_else(|| corrupt(format!("dtbl: the {what} pointer is null")))?;
    match at.checked_add(len) {
        Some(end) if end <= mem_len => Ok(volume.read_with(MEM).range(at..end).await?.to_vec()),
        _ => Err(corrupt(format!("dtbl: {len} bytes of {what} at {at:#x} are outside the pak's data"))),
    }
}

/// The NUL-terminated string at `.mem` offset `at`; `Corrupt` when it does not end inside `.mem`.
async fn read_cstr(volume: &Operator, at: u64, mem_len: u64) -> Result<Vec<u8>> {
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
    Err(corrupt(format!("dtbl: the string at {at:#x} does not end inside the pak's data")))
}

/// The string a `.mem` pointer slot points to (an empty string for a null pointer).
async fn read_string(asset: &RpakAsset, slot: &[u8], mem_len: u64, what: &str) -> Result<String> {
    let slot: [u8; 8] = slot.try_into().expect("8 bytes");
    if slot == [0; 8] {
        return Ok(String::new());
    }
    let at = asset.mem_offset_of(slot).ok_or_else(|| corrupt(format!("dtbl: the {what} points outside the pak's pages")))?;
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
        let asset = source
            .downcast_ref::<RpakAsset>()
            .ok_or_else(|| Error::new(ErrorKind::Custom { source: None, message: "dtbl must be read from an rpak.asset".into() }))?;
        if asset.entry.kind != *b"dtbl" {
            return Err(Error::new(ErrorKind::Custom { source: None, message: "the asset is not a dtbl".into() }));
        }
        let version = variant(asset.entry.version, asset.pak_created_time)?;
        let header = DtblHeader::parse(version, &asset.read_header().await?)?;
        let volume = &asset.volume;
        let mem_len = volume.stat(MEM).await?.content_length();

        let col_size = header.column_size();
        let records = read_block(volume, header.columns, u64::from(header.num_columns) * col_size, mem_len, "columns").await?;
        let (type_at, offset_at) = if col_size == 0x18 { (0x10, 0x14) } else { (0x08, 0x0C) };
        let mut columns = Vec::with_capacity(header.num_columns as usize);
        let mut offsets = Vec::with_capacity(header.num_columns as usize);
        for record in records.chunks_exact(col_size as usize) {
            let b = B(record);
            let index = columns.len();
            let raw = b.i32(type_at)?;
            let ty = column_type(raw).ok_or_else(|| corrupt(format!("dtbl: column {index} has unknown type {raw}")))?;
            let offset = u32::try_from(b.i32(offset_at)?).ok().filter(|&o| o.checked_add(cell_size(ty)).is_some_and(|end| end <= header.row_stride));
            let offset = offset.ok_or_else(|| corrupt(format!("dtbl: column {index} is outside the row stride {}", header.row_stride)))?;
            let name = read_string(asset, &record[..8], mem_len, "column name").await?;
            columns.push(Column { name, ty });
            offsets.push(offset as usize);
        }

        let stride = header.row_stride as usize;
        let block = read_block(volume, header.rows, u64::from(header.num_rows) * u64::from(header.row_stride), mem_len, "rows").await?;
        let mut rows = Vec::with_capacity(header.num_rows as usize);
        for i in 0..header.num_rows as usize {
            let row = B(&block[i * stride..(i + 1) * stride]);
            let mut cells = Vec::with_capacity(columns.len());
            for (column, &at) in columns.iter().zip(&offsets) {
                cells.push(match column.ty {
                    ColumnType::Bool => Cell::Bool(row.u8(at)? != 0),
                    ColumnType::Int => Cell::Int(row.i32(at)?),
                    ColumnType::Float => Cell::Float(row.f32(at)?),
                    ColumnType::Vector => Cell::Vector(row.floats::<3>(at)?),
                    ColumnType::String | ColumnType::Asset | ColumnType::AssetNoPrecache => {
                        Cell::String(read_string(asset, row.get(at, 8)?, mem_len, "string cell").await?)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn header(v1: bool, cols: i32, rows: i32, stride: i32) -> Vec<u8> {
        let mut h = vec![0u8; if v1 { 0x28 } else { 0x20 }];
        h[0..4].copy_from_slice(&cols.to_le_bytes());
        h[4..8].copy_from_slice(&rows.to_le_bytes());
        h[8..16].copy_from_slice(&0x100u64.to_le_bytes());
        h[16..24].copy_from_slice(&0x200u64.to_le_bytes());
        let at = if v1 { 0x20 } else { 0x18 };
        h[at..at + 4].copy_from_slice(&stride.to_le_bytes());
        h
    }

    #[test]
    fn the_variant_comes_from_the_asset_version_and_pak_date() {
        assert_eq!(variant(0, 0).unwrap(), Version::new(0));
        assert_eq!(variant(1, CREATED_V1_1_FROM).unwrap(), Version::new(1));
        assert_eq!(variant(1, CREATED_V1_1_FROM + 1).unwrap(), Version::with_minor(1, 1));
        assert_eq!(variant(1, CREATED_V1_1_TO - 1).unwrap(), Version::with_minor(1, 1));
        assert_eq!(variant(1, CREATED_V1_1_TO).unwrap(), Version::new(1));
        assert!(matches!(variant(2, 0).unwrap_err().kind(), ErrorKind::UnsupportedVersion { .. }));
    }

    #[test]
    fn both_header_layouts_parse() {
        let h = DtblHeader::parse(Version::new(0), &header(false, 3, 5, 24)).unwrap();
        assert_eq!((h.num_columns, h.num_rows, h.columns, h.rows, h.row_stride), (3, 5, Some(0x100), Some(0x200), 24));
        assert_eq!(h.column_size(), 0x10);
        let h = DtblHeader::parse(Version::with_minor(1, 1), &header(true, 3, 5, 24)).unwrap();
        assert_eq!((h.row_stride, h.column_size()), (24, 0x18));
    }

    #[test]
    fn a_wrong_header_length_or_negative_count_is_corrupt() {
        let short = DtblHeader::parse(Version::new(1), &header(false, 1, 1, 4)).unwrap_err();
        assert!(matches!(short.kind(), ErrorKind::Corrupt { .. }));
        let negative = DtblHeader::parse(Version::new(0), &header(false, -1, 1, 4)).unwrap_err();
        assert!(matches!(negative.kind(), ErrorKind::Corrupt { .. }));
    }
}
