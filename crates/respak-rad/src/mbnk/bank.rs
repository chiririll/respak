//! A parsed bank and its volume index.

use std::collections::BTreeMap;

use respak_core::detect::magic;
use respak_core::reader::Reader;
use respak_core::volume::VolumeEntry;
use respak_core::{Error, ErrorKind, Result};

use super::entry::{cstring, read_entry, SourceEntry};
use super::layout::{header_fields, BankLayout, MAGIC};
use crate::mstr::NOT_LOCALISED;

/// A parsed bank.
#[derive(Debug)]
pub struct Bank {
    layout: BankLayout,
    build_tag: u32,
    stem: String,
    sources: Vec<SourceEntry>,
    /// Volume path → index into `sources`.
    index: BTreeMap<String, usize>,
}

impl Bank {
    /// Parses a whole `.mbnk`. The entry array not fitting the file, or a string offset out of
    /// range → `Corrupt`; an unknown version → `UnsupportedVersion`.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if !magic(bytes, 0, MAGIC) || bytes.len() < 12 {
            return Err(Error::corrupt("mbnk: not a CBNK file"));
        }
        let version = Reader::new("mbnk", bytes).i32(4)?;
        let layout = BankLayout::for_version(version).ok_or_else(|| {
            Error::new(ErrorKind::UnsupportedVersion {
                format: "mbnk".into(),
                version: version.to_string(),
            })
        })?;
        let r = Reader::new("mbnk", bytes);
        let h = header_fields(layout.header);
        if bytes.len() < h.len {
            return Err(Error::corrupt(format!(
                "mbnk: {} bytes is shorter than the v{version} header",
                bytes.len()
            )));
        }
        let sources_at = if h.sources_wide {
            r.u64(h.sources)?
        } else {
            u64::from(r.u32(h.sources)?)
        };
        let strings_at = r.u64(h.strings)?;
        let unlocalised = u64::from(r.u32(h.unlocalised)?);
        let localised = u64::from(r.u32(h.localised)?);
        let build_tag = r.u32(h.build_tag)?;

        let languages = layout.languages.len() as u64;
        let count = localised
            .checked_mul(languages - 1)
            .and_then(|l| l.checked_add(unlocalised))
            .ok_or_else(|| Error::corrupt("mbnk: source count overflows"))?;
        let size = layout.entry_size();
        let table_len = count
            .checked_mul(size)
            .ok_or_else(|| Error::corrupt("mbnk: source table size overflows"))?;
        let table_end = sources_at
            .checked_add(table_len)
            .filter(|&end| end <= bytes.len() as u64);
        let table_end = table_end.ok_or_else(|| {
            Error::corrupt(format!(
                "mbnk: {count} sources at {sources_at} do not fit the file"
            ))
        })?;

        let stem = cstring(bytes, strings_at)?;
        let mut sources = Vec::with_capacity(count as usize);
        let mut first_name = None;
        for i in 0..count {
            let at = (sources_at + i * size) as usize;
            let raw = read_entry(
                layout.source,
                Reader::new("mbnk", r.get(at, size as usize)?),
            )?;
            // Bank v48 keeps the names in a block of their own right after the table.
            let name_at = if layout.source == 48 {
                let first = *first_name.get_or_insert(raw.name_offset);
                let rel = raw
                    .name_offset
                    .checked_sub(first)
                    .ok_or_else(|| Error::corrupt("mbnk: name offset before the first one"))?;
                table_end + rel
            } else {
                strings_at
                    .checked_add(raw.name_offset)
                    .ok_or_else(|| Error::corrupt("mbnk: name offset overflows"))?
            };
            let language = (raw.language != NOT_LOCALISED).then(|| {
                layout
                    .languages
                    .get(usize::from(raw.language))
                    .map_or_else(|| format!("lang{}", raw.language), |l| (*l).to_owned())
            });
            let header_end = raw
                .header_offset
                .checked_add(u64::from(raw.header_size))
                .ok_or_else(|| Error::corrupt("mbnk: header range overflows"))?;
            sources.push(SourceEntry {
                name: cstring(bytes, name_at)?,
                language,
                language_idx: raw.language,
                patch_idx: raw.patch,
                sample_rate: raw.sample_rate,
                sample_count: raw.sample_count,
                header_range: raw.header_offset..header_end,
                data_offset: raw.data_offset,
            });
        }

        let mut index = BTreeMap::new();
        for (i, s) in sources.iter().enumerate() {
            let mut path = Self::volume_name(s);
            if index.contains_key(&path) {
                path = format!("{}~{i}.asrc", path.trim_end_matches(".asrc"));
            }
            index.insert(path, i);
        }
        Ok(Self {
            layout,
            build_tag,
            stem,
            sources,
            index,
        })
    }

    /// The layout the version table gave.
    pub fn layout(&self) -> BankLayout {
        self.layout
    }

    /// The build tag every `.mstr` of the bank carries.
    pub fn build_tag(&self) -> u32 {
        self.build_tag
    }

    /// The bank stem (the first string), which names the `.mstr` files.
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// The sources, in table order.
    pub fn sources(&self) -> &[SourceEntry] {
        &self.sources
    }

    /// Volume path of `source`: `name.asrc`, or `name.<lang>.asrc` when localised.
    pub fn volume_name(source: &SourceEntry) -> String {
        let name = source.name.replace('\\', "/");
        let name = name.trim_start_matches('/');
        match &source.language {
            Some(lang) => format!("{name}.{lang}.asrc"),
            None => format!("{name}.asrc"),
        }
    }

    /// The source at a volume path.
    pub fn find(&self, path: &str) -> Option<&SourceEntry> {
        self.index.get(path).map(|&i| &self.sources[i])
    }

    pub(super) fn is_dir(&self, path: &str) -> bool {
        let prefix = format!("{path}/");
        self.index
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(k, _)| k.starts_with(&prefix))
    }

    pub(super) fn list(&self, dir: &str) -> Vec<VolumeEntry> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<VolumeEntry> = Vec::new();
        for (path, &i) in self.index.range(prefix.clone()..) {
            let Some(rest) = path.strip_prefix(&prefix) else {
                break;
            };
            match rest.split_once('/') {
                Some((folder, _)) => {
                    if out.last().is_none_or(|e| e.name != folder) {
                        out.push(VolumeEntry {
                            name: folder.to_owned(),
                            is_dir: true,
                            len: 0,
                        });
                    }
                }
                None => out.push(VolumeEntry {
                    name: rest.to_owned(),
                    is_dir: false,
                    len: self.sources[i].descriptor().len() as u64,
                }),
            }
        }
        out
    }
}
