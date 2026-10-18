//! `subt`: the subtitles of one language, read from `rpak.asset`. Apex Legends, asset versions 0
//! and 1 (one 24-byte header).
//!
//! Ported from rsx `src/game/rtech/assets/subtitles.{h,cpp}` (`SubtitlesAssetHeader_v0_t`,
//! `SubtitlesAssetEntry_t`, `SubtitlesAsset`). Read edge plus a TXT encoder; CSV and JSON come
//! from the generic `table.csv` / `table.json` through the [`Table`] capability. See
//! `docs/formats/subt.md`.

mod read;
mod text;

pub use read::SubtRead;
pub use text::SubtTxtEncode;

use respak_core::capability::{Cell, Column, ColumnType, Dependency, Metadata, Table};
use respak_core::reader::Reader;
use respak_core::{Data, Error, FormatId, Result, ValueKey, Version};

/// Bytes of the header (`SubtitlesAssetHeader_v0_t`).
const HEADER_LEN: usize = 24;

/// Bytes of one table slot (`SubtitlesAssetEntry_t`).
const ENTRY_LEN: usize = 8;

/// Largest slot count taken as plausible.
const LIMIT: i32 = 1 << 24;

/// The colour of a line without a `<clr:R,G,B>` tag.
pub const WHITE: [u8; 3] = [255, 255, 255];

/// The header of a `subt` (versions 0 and 1 share it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtHeader {
    /// `stringCount`: a capacity hint only.
    pub string_count: i32,
    /// `entryCount`: slots in the table, empty ones included.
    pub entry_count: i32,
    /// Page pointer of the slot array, as a `.mem` offset (`None` if null).
    pub entries: Option<u64>,
    /// Page pointer of the string block, as a `.mem` offset (`None` if null).
    pub strings: Option<u64>,
}

impl SubtHeader {
    /// Parses the relocated asset header; a wrong length, a negative or implausible count or a
    /// null slot pointer with a non-zero count is `Corrupt`.
    pub fn parse(header: &[u8]) -> Result<Self> {
        if header.len() != HEADER_LEN {
            return Err(Error::corrupt(format!(
                "subt header is {} bytes, expected {HEADER_LEN}",
                header.len()
            )));
        }
        let h = Reader::new("subt header", header);
        let string_count = h.i32(0x00)?;
        let entry_count = h.i32(0x04)?;
        let pointer = |at| -> Result<Option<u64>> {
            let v = u64::from_le_bytes(h.array(at)?);
            Ok((v != 0 && v != u64::MAX).then_some(v))
        };
        let entries = pointer(0x08)?;
        let strings = pointer(0x10)?;
        if string_count < 0 || !(0..=LIMIT).contains(&entry_count) {
            return Err(Error::corrupt(format!(
                "subt: implausible counts ({string_count} strings, {entry_count} slots)"
            )));
        }
        if entry_count > 0 && entries.is_none() {
            return Err(Error::corrupt(
                "subt: slots are counted but their pointer is null",
            ));
        }
        Ok(Self {
            string_count,
            entry_count,
            entries,
            strings,
        })
    }
}

/// One subtitle line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtEntry {
    /// The key of the line (never 0: those slots are empty).
    pub hash: u32,
    /// The colour from the `<clr:R,G,B>` tag, [`WHITE`] without one.
    pub color: [u8; 3],
    /// The text after the tag. `<delay>` and `<len>` tags stay in it.
    pub text: String,
}

/// Splits a leading `<clr:R,G,B>` tag off `s`. Returns the colour and the rest; a missing or
/// malformed tag (no `>`, fewer or more than three numbers, a value over 255) gives `None` and
/// the whole string, so the caller can warn.
pub fn parse_color_tag(s: &str) -> (Option<[u8; 3]>, &str) {
    let Some(tag) = s.strip_prefix("<clr:") else {
        return (None, s);
    };
    let Some((inner, rest)) = tag.split_once('>') else {
        return (None, s);
    };
    let mut color = [0u8; 3];
    let mut parts = inner.split(',');
    for c in &mut color {
        match parts.next().and_then(|p| p.trim().parse().ok()) {
            Some(v) => *c = v,
            None => return (None, s),
        }
    }
    if parts.next().is_some() {
        return (None, s);
    }
    (Some(color), rest)
}

/// A `subt`: `Data` + `Metadata` + `Table`. The table has the columns `hash` (string, `0x` and
/// lowercase hex), `color` (vector `[r, g, b]`) and `subtitle` (string), one row per line.
#[derive(Debug)]
pub struct SubtAsset {
    format: FormatId,
    key: ValueKey,
    language: Option<String>,
    entries: Vec<SubtEntry>,
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
    guid: u64,
}

impl SubtAsset {
    pub(crate) fn new(
        key: ValueKey,
        version: u32,
        language: Option<String>,
        entries: Vec<SubtEntry>,
        guid: u64,
    ) -> Self {
        let column = |name: &str, ty| Column {
            name: name.into(),
            ty,
        };
        let rows = entries
            .iter()
            .map(|e| {
                vec![
                    Cell::String(format!("0x{:08x}", e.hash)),
                    Cell::Vector(e.color.map(f32::from)),
                    Cell::String(e.text.clone()),
                ]
            })
            .collect();
        Self {
            format: FormatId::new("subt").with_version(Version::new(version)),
            key,
            language,
            entries,
            columns: vec![
                column("hash", ColumnType::String),
                column("color", ColumnType::Vector),
                column("subtitle", ColumnType::String),
            ],
            rows,
            guid,
        }
    }

    /// The lines, in table order, empty slots skipped.
    pub fn entries(&self) -> &[SubtEntry] {
        &self.entries
    }

    /// The language (`subtitles_<language>` pak), set only if the name hashes to the guid.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// The asset guid.
    pub fn guid(&self) -> u64 {
        self.guid
    }
}

impl Metadata for SubtAsset {
    fn name(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> Option<&str> {
        Some("subt")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Table for SubtAsset {
    fn columns(&self) -> &[Column] {
        &self.columns
    }
    fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }
}

impl Data for SubtAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
    fn as_table(&self) -> Option<&dyn Table> {
        Some(self)
    }
}

#[cfg(test)]
mod tests;
