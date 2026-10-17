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

mod text;

pub use text::{format_g, to_csv, to_json, DtblCsvEncode, DtblJsonEncode};

mod read;
pub use read::DtblRead;

use std::sync::Arc;

use async_trait::async_trait;
use opendal::Operator;
use respak_core::capability::{Cell, Column, ColumnType, Dependency, Metadata, NameSource, Table};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::guid::hex;
use crate::rmdl::reader;
use crate::rpak::{RpakAsset, MEM};

/// Pak `createdTime` bounds (exclusive) of the V1.1 column layout.
const CREATED_V1_1_FROM: u64 = 0x1d6_92d8_9727_5335;
const CREATED_V1_1_TO: u64 = 0x1da_975b_0a10_6ef2;

/// What a string cell reads as when the data was stripped from the pak (first byte `0x0f`).
pub const DATA_EXCLUDED: &str = "!!DATA EXCLUDED!!";

/// The longest string a cell or column name may have.
const MAX_STRING: u64 = 1 << 20;

/// The layout variant: `0`, `1` or `1.1` (see the module docs).
pub fn variant(asset_version: u32, created_time: u64) -> Result<Version> {
    match asset_version {
        0 => Ok(Version::new(0)),
        1 if created_time > CREATED_V1_1_FROM && created_time < CREATED_V1_1_TO => {
            Ok(Version::with_minor(1, 1))
        }
        1 => Ok(Version::new(1)),
        other => Err(Error::new(ErrorKind::UnsupportedVersion {
            format: "dtbl".into(),
            version: other.to_string(),
        })),
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
        let (len, stride_at) = if version.major == 0 {
            (0x20, 0x18)
        } else {
            (0x28, 0x20)
        };
        if h.len() != len {
            return Err(Error::corrupt(format!(
                "dtbl@{version} header is {} bytes, expected {len}",
                h.len()
            )));
        }
        let b = reader(h);
        let count = |at: usize, what: &str| -> Result<u32> {
            u32::try_from(b.i32(at)?).map_err(|_| Error::corrupt(format!("dtbl: negative {what}")))
        };
        let ptr = |at: usize| -> Result<Option<u64>> {
            b.u64(at).map(|p| (p != 0 && p != u64::MAX).then_some(p))
        };
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

/// The text cells of `asset` and `asset_noprecache` columns are asset paths; plain `string` cells
/// can be too (a settings key, a model name).
impl NameSource for DtblAsset {
    fn name_candidates(&self) -> Vec<String> {
        let text: Vec<usize> = (0..self.columns.len())
            .filter(|&i| {
                matches!(
                    self.columns[i].ty,
                    ColumnType::String | ColumnType::Asset | ColumnType::AssetNoPrecache
                )
            })
            .collect();
        self.rows
            .iter()
            .flat_map(|row| text.iter().filter_map(|&i| row.get(i)))
            .filter_map(|cell| match cell {
                Cell::String(s) if !s.is_empty() => Some(s.clone()),
                _ => None,
            })
            .collect()
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
    fn as_name_source(&self) -> Option<&dyn NameSource> {
        Some(self)
    }
}

#[cfg(test)]
mod tests;
