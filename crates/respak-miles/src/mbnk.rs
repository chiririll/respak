//! `mbnk`: a Miles sound bank, opened as a volume of sources.
//!
//! # The volume
//! ```text
//! general.mbnk/
//!   <source name>.asrc            non-localised source
//!   <source name>.<lang>.asrc     localised source (language from the per-version table)
//! ```
//! Every source is listed; a missing `.mstr` shows up only when the source is opened. A source
//! name with `/` in it becomes folders.
//!
//! # Cost
//! The whole `.mbnk` is read (banks are small next to the streams). Nothing of any `.mstr` is
//! touched while listing.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::Operator;
use respak_core::capability::Container;
use respak_core::detect::{magic, u32le};
use respak_core::value::FileData;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::asrc::{AsrcData, StreamHeads};
use crate::corrupt;
use crate::mstr::NOT_LOCALISED;

/// Bytes of the largest bank header (v45), enough to detect and pick a layout.
pub const MBNK_HEADER_LEN: usize = 0xC8;

/// `'CBNK'` as it lies on disk.
const MAGIC: &[u8] = b"KNBC";

const LANG_R2: &[&str] = &[
    "english",
    "french",
    "german",
    "spanish",
    "italian",
    "japanese",
    "polish",
    "portuguese",
    "russian",
    "tchinese",
    "mspanish",
];
const LANG_R5_9: &[&str] = &[
    "english", "french", "german", "spanish", "italian", "japanese", "polish", "russian",
    "mandarin",
];
const LANG_R5_10: &[&str] = &[
    "english", "french", "german", "spanish", "italian", "japanese", "polish", "russian",
    "mandarin", "korean",
];

/// Header, source-entry and language-table layout of a bank version (`miles.md`, "Bank versions").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankLayout {
    /// Bank version (`major` of the detected `Version`).
    pub version: i32,
    /// Header layout: 13, 28, 45 or 49.
    pub header: u8,
    /// Source-entry layout: 13, 28, 39, 48 or 49.
    pub source: u8,
    /// Languages in the table (11, 9 or 10).
    pub languages: &'static [&'static str],
}

impl BankLayout {
    /// The layout of `version`, `None` for a version the table does not know (→
    /// `UnsupportedVersion`).
    pub fn for_version(version: i32) -> Option<Self> {
        let (header, source, languages) = match version {
            13 => (13, 13, LANG_R2),
            28 | 32 | 33 | 34 | 36 | 38 => (28, 28, LANG_R5_9),
            39 | 40 | 42..=46 => (45, 39, LANG_R5_10),
            48 => (45, 48, LANG_R5_10),
            49 => (49, 49, LANG_R5_10),
            _ => return None,
        };
        Some(Self {
            version,
            header,
            source,
            languages,
        })
    }

    /// Bytes of one source entry.
    fn entry_size(&self) -> u64 {
        match self.source {
            13 | 28 => 0x58,
            39 => 0x48,
            _ => 0x50,
        }
    }
}

/// Where the fields the reader needs sit in a header layout.
#[derive(Debug, Clone, Copy)]
struct HeaderFields {
    len: usize,
    /// Offset of the `sources` pointer, and whether it is a 4-byte one (v49) or an 8-byte one.
    sources: usize,
    sources_wide: bool,
    strings: usize,
    unlocalised: usize,
    localised: usize,
    build_tag: usize,
}

fn header_fields(header: u8) -> HeaderFields {
    match header {
        13 => HeaderFields {
            len: 0xC0,
            sources: 0x48,
            sources_wide: true,
            strings: 0x70,
            localised: 0x9C,
            unlocalised: 0xA0,
            build_tag: 0xBC,
        },
        28 => HeaderFields {
            len: 0xC0,
            sources: 0x48,
            sources_wide: true,
            strings: 0x70,
            localised: 0x94,
            unlocalised: 0x98,
            build_tag: 0xB0,
        },
        45 => HeaderFields {
            len: 0xC8,
            sources: 0x50,
            sources_wide: true,
            strings: 0x80,
            localised: 0xA4,
            unlocalised: 0xA8,
            build_tag: 0x10,
        },
        _ => HeaderFields {
            len: 0xA0,
            sources: 0x34,
            sources_wide: false,
            strings: 0x70,
            localised: 0x3C,
            unlocalised: 0x38,
            build_tag: 0x10,
        },
    }
}

/// One source of a bank, with what is needed to find its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceEntry {
    /// Name from the string table (the v48 name block for bank v48).
    pub name: String,
    /// Language name for a localised source.
    pub language: Option<String>,
    /// Language table index (`0xFFFF` = not localised).
    pub language_idx: u16,
    /// Patch index (`0` = the base `.mstr`).
    pub patch_idx: u16,
    /// Sample rate recorded in the bank (the codec header wins for decoding).
    pub sample_rate: u16,
    /// Frames recorded in the bank.
    pub sample_count: u64,
    /// Codec header block, absolute offsets in the `.mstr`.
    pub header_range: Range<u64>,
    /// Offset of the encoded data, relative to the `.mstr` header's `streamDataOffset`.
    pub data_offset: u64,
}

impl SourceEntry {
    /// The bytes of the `.asrc` entry in the volume: a few `key=value` lines. Audio is never in
    /// them.
    fn descriptor(&self) -> String {
        format!(
            "name={}\nlanguage={}\npatch={}\nsample_rate={}\nsample_count={}\n",
            self.name,
            self.language.as_deref().unwrap_or(""),
            self.patch_idx,
            self.sample_rate,
            self.sample_count
        )
    }
}

fn at<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    b[off..off + N]
        .try_into()
        .expect("slice length checked by the caller")
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(at(b, off))
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(at(b, off))
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(at(b, off))
}

/// The fields of one entry, whatever its layout.
struct RawEntry {
    name_offset: u64,
    language: u16,
    patch: u16,
    sample_rate: u16,
    sample_count: u64,
    header_size: u32,
    header_offset: u64,
    data_offset: u64,
}

fn read_entry(source: u8, e: &[u8]) -> RawEntry {
    match source {
        13 => RawEntry {
            name_offset: u64::from(u32_at(e, 0x10)),
            language: u16_at(e, 0x50),
            patch: u16_at(e, 0x52),
            sample_rate: u16_at(e, 0x14),
            sample_count: u64::from(u32_at(e, 0x34)),
            header_size: u32_at(e, 0x30),
            header_offset: u64_at(e, 0x38),
            data_offset: u64_at(e, 0x40),
        },
        28 => RawEntry {
            name_offset: u64::from(u32_at(e, 0x10)),
            language: u16_at(e, 0x0C),
            patch: u16_at(e, 0x0E),
            sample_rate: u16_at(e, 0x14),
            sample_count: u64::from(u32_at(e, 0x34)),
            header_size: u32_at(e, 0x30),
            header_offset: u64_at(e, 0x38),
            data_offset: u64_at(e, 0x40),
        },
        39 => RawEntry {
            name_offset: u64::from(u32_at(e, 0)),
            language: u16_at(e, 4),
            patch: u16_at(e, 6),
            sample_rate: u16_at(e, 0x0C),
            sample_count: u64::from(u32_at(e, 0x24)),
            header_size: u32_at(e, 0x20),
            header_offset: u64_at(e, 0x28),
            data_offset: u64_at(e, 0x30),
        },
        _ => RawEntry {
            name_offset: u64_at(e, 0),
            language: u16_at(e, 8),
            patch: u16_at(e, 0x0A),
            sample_rate: u16_at(e, 0x10),
            sample_count: u64_at(e, 0x28),
            header_size: u32_at(e, 0x24),
            header_offset: u64_at(e, 0x30),
            data_offset: u64_at(e, 0x38),
        },
    }
}

/// The NUL-terminated string at `off`.
fn cstring(bytes: &[u8], off: u64) -> Result<String> {
    let tail = usize::try_from(off)
        .ok()
        .and_then(|o| bytes.get(o..))
        .ok_or_else(|| corrupt(format!("mbnk: string offset {off} is past the end")))?;
    let end = tail
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| corrupt("mbnk: unterminated string"))?;
    Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
}

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
            return Err(corrupt("mbnk: not a CBNK file"));
        }
        let version = u32le(bytes, 4).expect("length checked") as i32;
        let layout = BankLayout::for_version(version).ok_or_else(|| {
            Error::new(ErrorKind::UnsupportedVersion {
                format: "mbnk".into(),
                version: version.to_string(),
            })
        })?;
        let h = header_fields(layout.header);
        if bytes.len() < h.len {
            return Err(corrupt(format!(
                "mbnk: {} bytes is shorter than the v{version} header",
                bytes.len()
            )));
        }
        let sources_at = if h.sources_wide {
            u64_at(bytes, h.sources)
        } else {
            u64::from(u32_at(bytes, h.sources))
        };
        let strings_at = u64_at(bytes, h.strings);
        let unlocalised = u64::from(u32_at(bytes, h.unlocalised));
        let localised = u64::from(u32_at(bytes, h.localised));
        let build_tag = u32_at(bytes, h.build_tag);

        let languages = layout.languages.len() as u64;
        let count = localised
            .checked_mul(languages - 1)
            .and_then(|l| l.checked_add(unlocalised))
            .ok_or_else(|| corrupt("mbnk: source count overflows"))?;
        let size = layout.entry_size();
        let table_len = count
            .checked_mul(size)
            .ok_or_else(|| corrupt("mbnk: source table size overflows"))?;
        let table_end = sources_at
            .checked_add(table_len)
            .filter(|&end| end <= bytes.len() as u64);
        let table_end = table_end.ok_or_else(|| {
            corrupt(format!(
                "mbnk: {count} sources at {sources_at} do not fit the file"
            ))
        })?;

        let stem = cstring(bytes, strings_at)?;
        let mut sources = Vec::with_capacity(count as usize);
        let mut first_name = None;
        for i in 0..count {
            let at = (sources_at + i * size) as usize;
            let raw = read_entry(layout.source, &bytes[at..at + size as usize]);
            // Bank v48 keeps the names in a block of their own right after the table.
            let name_at = if layout.source == 48 {
                let first = *first_name.get_or_insert(raw.name_offset);
                let rel = raw
                    .name_offset
                    .checked_sub(first)
                    .ok_or_else(|| corrupt("mbnk: name offset before the first one"))?;
                table_end + rel
            } else {
                strings_at
                    .checked_add(raw.name_offset)
                    .ok_or_else(|| corrupt("mbnk: name offset overflows"))?
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
                .ok_or_else(|| corrupt("mbnk: header range overflows"))?;
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

    fn is_dir(&self, path: &str) -> bool {
        let prefix = format!("{path}/");
        self.index
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(k, _)| k.starts_with(&prefix))
    }

    fn list(&self, dir: &str) -> Vec<VolumeEntry> {
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

fn not_found(path: &str) -> Error {
    Error::new(ErrorKind::NotFound {
        path: path.to_owned(),
    })
}

struct MbnkSource(Arc<Bank>);

#[async_trait]
impl VolumeSource for MbnkSource {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty() || self.0.is_dir(path) {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        let s = self.0.find(path).ok_or_else(|| not_found(path))?;
        Ok(VolumeStat {
            is_dir: false,
            len: s.descriptor().len() as u64,
        })
    }

    /// The bytes of an `.asrc` entry are its descriptor, never audio data.
    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        let s = self.0.find(path).ok_or_else(|| not_found(path))?;
        let all = Bytes::from(s.descriptor());
        let end = (range.end as usize).min(all.len());
        Ok(all.slice((range.start as usize).min(end)..end))
    }

    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        Ok(self.0.list(dir))
    }
}

/// An opened bank: `Data` + `Container`.
#[derive(Debug)]
pub struct MbnkData {
    format: FormatId,
    version: Version,
    key: ValueKey,
    mount: String,
    volume: Operator,
    host: Operator,
    dir: String,
    bank: Arc<Bank>,
    heads: StreamHeads,
}

impl MbnkData {
    /// The parsed bank.
    pub fn bank(&self) -> &Bank {
        &self.bank
    }
}

#[async_trait]
impl Container for MbnkData {
    fn volume(&self) -> Operator {
        self.volume.clone()
    }

    fn mount_path(&self) -> &str {
        &self.mount
    }

    /// Hands out an `asrc` ([`AsrcData`]) for a source path. Opening it reads the `.mstr`
    /// header and the source's codec header, so a missing stream is reported here.
    async fn entry(&self, path: &str) -> Result<Value> {
        let key = ValueKey::file(format!("{}/{path}", self.mount));
        if let Some(source) = self.bank.find(path) {
            let asrc = AsrcData::open(
                &self.host,
                &self.dir,
                &self.bank,
                source,
                key,
                self.version,
                &self.heads,
            )
            .await?;
            return Ok(Arc::new(asrc));
        }
        if path.is_empty() || self.bank.is_dir(path) {
            return Ok(Arc::new(FileData::new(
                self.volume.clone(),
                path,
                format!("{}/{path}", self.mount),
                None,
            )));
        }
        Err(not_found(path))
    }
}

impl Data for MbnkData {
    fn format(&self) -> &FormatId {
        &self.format
    }

    fn key(&self) -> &ValueKey {
        &self.key
    }

    fn as_container(&self) -> Option<&dyn Container> {
        Some(self)
    }
}

/// Read edge `file -> mbnk`.
pub struct MbnkRead;

#[async_trait]
impl Detect for MbnkRead {
    /// Magic `CBNK` = 10; a known version = +2; an unknown one is `UnsupportedVersion`; a
    /// `fileSize` field equal to the length = +2. `Version { major: bank version, minor: 0 }`.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        if !magic(input.header, 0, MAGIC) {
            return Ok(None);
        }
        let Some(version) = u32le(input.header, 4) else {
            return Ok(None);
        };
        let version = version as i32;
        if BankLayout::for_version(version).is_none() {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "mbnk".into(),
                version: version.to_string(),
            }));
        }
        let mut score = 12;
        if input
            .len
            .is_some_and(|len| u32le(input.header, 8).map(u64::from) == Some(len))
        {
            score += 2;
        }
        Ok(Some(Detected {
            score,
            version: Some(Version::new(version as u32)),
        }))
    }

    fn header_len(&self) -> usize {
        MBNK_HEADER_LEN
    }
}

#[async_trait]
impl ReadHandler for MbnkRead {
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let file = source.downcast_ref::<FileData>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "mbnk must be read from a file".into(),
            })
        })?;
        let read = file.op.read_with(&file.path);
        let bytes = match &file.range {
            Some(r) => read.range(r.clone()).await?,
            None => read.await?,
        }
        .to_bytes();
        let bank = Arc::new(Bank::parse(&bytes)?);
        let mount = source.key().to_string();
        let volume = volume_operator(MbnkSource(bank.clone()), &mount);
        let dir = file
            .path
            .rsplit_once('/')
            .map_or(String::new(), |(d, _)| format!("{d}/"));
        let version = Version::new(bank.layout.version as u32);
        let format = FormatId::new("mbnk").with_version(version);
        Ok(Arc::new(MbnkData {
            format,
            version,
            key: source.key().clone(),
            mount,
            volume,
            host: file.op.clone(),
            dir,
            bank,
            heads: StreamHeads::default(),
        }))
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Synthetic banks for every layout, written with the same field table the reader uses (so
    //! these tests prove the reader is self-consistent; real banks are the check of the table).

    use super::*;

    /// A source to put in a synthetic bank.
    #[derive(Clone)]
    pub(crate) struct Fx {
        pub name: &'static str,
        pub language: u16,
        pub patch: u16,
        pub rate: u16,
        pub frames: u64,
        pub header_off: u64,
        pub header_size: u32,
        pub data_off: u64,
    }

    impl Fx {
        pub(crate) fn new(name: &'static str) -> Self {
            Self {
                name,
                language: 0xFFFF,
                patch: 0,
                rate: 48000,
                frames: 1000,
                header_off: 0x100,
                header_size: 24,
                data_off: 0,
            }
        }
    }

    fn put(b: &mut [u8], off: usize, v: &[u8]) {
        b[off..off + v.len()].copy_from_slice(v);
    }

    fn write_entry(source: u8, e: &mut [u8], f: &Fx, name_off: u64) {
        match source {
            13 => {
                put(e, 0x10, &(name_off as u32).to_le_bytes());
                put(e, 0x14, &f.rate.to_le_bytes());
                put(e, 0x30, &f.header_size.to_le_bytes());
                put(e, 0x34, &(f.frames as u32).to_le_bytes());
                put(e, 0x38, &f.header_off.to_le_bytes());
                put(e, 0x40, &f.data_off.to_le_bytes());
                put(e, 0x50, &f.language.to_le_bytes());
                put(e, 0x52, &f.patch.to_le_bytes());
            }
            28 => {
                put(e, 0x0C, &f.language.to_le_bytes());
                put(e, 0x0E, &f.patch.to_le_bytes());
                put(e, 0x10, &(name_off as u32).to_le_bytes());
                put(e, 0x14, &f.rate.to_le_bytes());
                put(e, 0x30, &f.header_size.to_le_bytes());
                put(e, 0x34, &(f.frames as u32).to_le_bytes());
                put(e, 0x38, &f.header_off.to_le_bytes());
                put(e, 0x40, &f.data_off.to_le_bytes());
            }
            39 => {
                put(e, 0, &(name_off as u32).to_le_bytes());
                put(e, 4, &f.language.to_le_bytes());
                put(e, 6, &f.patch.to_le_bytes());
                put(e, 0x0C, &f.rate.to_le_bytes());
                put(e, 0x20, &f.header_size.to_le_bytes());
                put(e, 0x24, &(f.frames as u32).to_le_bytes());
                put(e, 0x28, &f.header_off.to_le_bytes());
                put(e, 0x30, &f.data_off.to_le_bytes());
            }
            _ => {
                put(e, 0, &name_off.to_le_bytes());
                put(e, 8, &f.language.to_le_bytes());
                put(e, 0x0A, &f.patch.to_le_bytes());
                put(e, 0x10, &f.rate.to_le_bytes());
                put(e, 0x24, &f.header_size.to_le_bytes());
                put(e, 0x28, &f.frames.to_le_bytes());
                put(e, 0x30, &f.header_off.to_le_bytes());
                put(e, 0x38, &f.data_off.to_le_bytes());
            }
        }
    }

    /// A bank of `version` with `sources` as its whole entry array; `unlocalised` and `localised`
    /// are the two header counts.
    pub(crate) fn bank(
        version: i32,
        build_tag: u32,
        stem: &str,
        sources: &[Fx],
        unlocalised: u32,
        localised: u32,
    ) -> Vec<u8> {
        let layout = BankLayout::for_version(version).expect("a known version");
        let h = header_fields(layout.header);
        let size = layout.entry_size() as usize;
        let table_at = h.len;
        let table_end = table_at + size * sources.len();
        let mut strings = format!("{stem}\0").into_bytes();
        let mut name_block = Vec::new();
        let mut entries = vec![0u8; size * sources.len()];
        for (i, f) in sources.iter().enumerate() {
            let name_off = if layout.source == 48 {
                // The first name sits at an arbitrary base: readers must use the difference.
                let off = 0x4000 + name_block.len() as u64;
                name_block.extend(format!("{}\0", f.name).bytes());
                off
            } else {
                let off = strings.len() as u64;
                strings.extend(format!("{}\0", f.name).bytes());
                off
            };
            write_entry(
                layout.source,
                &mut entries[i * size..(i + 1) * size],
                f,
                name_off,
            );
        }
        let strings_at = table_end + name_block.len();
        let mut b = vec![0u8; h.len];
        put(&mut b, 0, MAGIC);
        put(&mut b, 4, &version.to_le_bytes());
        put(&mut b, h.build_tag, &build_tag.to_le_bytes());
        if h.sources_wide {
            put(&mut b, h.sources, &(table_at as u64).to_le_bytes());
        } else {
            put(&mut b, h.sources, &(table_at as u32).to_le_bytes());
        }
        put(&mut b, h.strings, &(strings_at as u64).to_le_bytes());
        put(&mut b, h.unlocalised, &unlocalised.to_le_bytes());
        put(&mut b, h.localised, &localised.to_le_bytes());
        b.extend(entries);
        b.extend(name_block);
        b.extend(strings);
        let len = b.len() as u32;
        put(&mut b, 8, &len.to_le_bytes());
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    /// `sourceCount`/`localisedSourceCount` for one plain source plus one localised across all
    /// languages (`1 + 1 * (languages - 1)` entries; the localised source's `english` entry is
    /// counted in `unlocalised`).
    fn mixed(version: i32) -> Vec<u8> {
        let layout = BankLayout::for_version(version).unwrap();
        let mut sources = vec![Fx::new("music/theme")];
        for (i, _) in layout.languages.iter().enumerate() {
            let mut f = Fx::new("vo/hello");
            f.language = i as u16;
            f.patch = (i % 2) as u16;
            sources.push(f);
        }
        bank(version, 0xBEEF, "general", &sources, 2, 1)
    }

    #[test]
    fn every_layout_gives_the_same_sources() {
        for version in [13, 28, 38, 39, 45, 46, 48, 49] {
            let b = Bank::parse(&mixed(version)).unwrap_or_else(|e| panic!("v{version}: {e}"));
            let langs = b.layout().languages;
            assert_eq!(b.sources().len(), 1 + langs.len(), "v{version}");
            assert_eq!((b.stem(), b.build_tag()), ("general", 0xBEEF), "v{version}");
            let theme = &b.sources()[0];
            assert_eq!(
                (
                    theme.name.as_str(),
                    theme.language.as_deref(),
                    theme.sample_rate,
                    theme.sample_count
                ),
                ("music/theme", None, 48000, 1000),
                "v{version}"
            );
            assert_eq!(theme.header_range, 0x100..0x118, "v{version}");
            let last = b.sources().last().unwrap();
            assert_eq!(
                last.language.as_deref(),
                langs.last().copied(),
                "v{version}"
            );
            assert_eq!(
                Bank::volume_name(last),
                format!("vo/hello.{}.asrc", langs.last().unwrap()),
                "v{version}"
            );
            assert_eq!(last.patch_idx, ((langs.len() - 1) % 2) as u16, "v{version}");
        }
    }

    #[test]
    fn the_language_tables_have_the_documented_sizes() {
        let n = |v| BankLayout::for_version(v).unwrap().languages.len();
        assert_eq!((n(13), n(28), n(45), n(49)), (11, 9, 10, 10));
        for v in [11, 29, 31, 35, 37, 41, 47, 50, -1] {
            assert!(BankLayout::for_version(v).is_none(), "v{v}");
        }
    }

    #[test]
    fn the_entry_count_follows_the_header_counts() {
        // 2 plain + 1 localised * (10 - 1) = 11 entries in a v49 bank
        let sources: Vec<Fx> = (0..11).map(|_| Fx::new("a")).collect();
        let b = Bank::parse(&bank(49, 1, "general", &sources, 2, 1)).unwrap();
        assert_eq!(b.sources().len(), 11);
    }

    #[test]
    fn an_unknown_version_and_bad_magic() {
        let mut bytes = bank(49, 1, "general", &[Fx::new("a")], 1, 0);
        bytes[4..8].copy_from_slice(&47i32.to_le_bytes());
        assert!(matches!(
            Bank::parse(&bytes).unwrap_err().kind(),
            ErrorKind::UnsupportedVersion { .. }
        ));
        bytes[0] = b'X';
        assert!(matches!(
            Bank::parse(&bytes).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
    }

    #[test]
    fn damaged_banks_are_corrupt_never_a_panic() {
        let good = bank(49, 1, "general", &[Fx::new("a")], 1, 0);
        // an entry array past the end of the file
        let mut b = good.clone();
        b[0x38..0x3C].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(matches!(
            Bank::parse(&b).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ));
        // counts that overflow
        let mut b = good.clone();
        b[0x3C..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Bank::parse(&b).is_err());
        // a string offset out of range
        let mut b = good.clone();
        b[0x70..0x78].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(Bank::parse(&b).is_err());
        // truncations at every length
        for len in 0..good.len() {
            let _ = Bank::parse(&good[..len]);
        }
        assert!(Bank::parse(&good[..0x20]).is_err());
    }

    #[test]
    fn duplicate_names_get_distinct_paths() {
        let sources = [Fx::new("same"), Fx::new("same")];
        let b = Bank::parse(&bank(49, 1, "general", &sources, 2, 0)).unwrap();
        assert!(b.find("same.asrc").is_some());
        assert!(b.find("same~1.asrc").is_some());
    }

    #[test]
    fn the_volume_lists_files_and_folders() {
        let b = Bank::parse(&mixed(49)).unwrap();
        let root = b.list("");
        let names: Vec<_> = root.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
        assert_eq!(names, [("music", true), ("vo", true)]);
        let vo = b.list("vo");
        assert_eq!(vo.len(), 10);
        assert!(vo
            .iter()
            .all(|e| !e.is_dir && e.name.starts_with("hello.") && e.len > 0));
        assert!(b.is_dir("music") && !b.is_dir("music/theme.asrc") && !b.is_dir("nope"));
        assert!(b.find("music/theme.asrc").is_some() && b.find("vo/hello.korean.asrc").is_some());
    }

    #[tokio::test]
    async fn volume_stat_read_and_not_found() {
        let src = MbnkSource(Arc::new(Bank::parse(&mixed(49)).unwrap()));
        assert!(src.stat("").await.unwrap().is_dir);
        assert!(src.stat("vo").await.unwrap().is_dir);
        let stat = src.stat("music/theme.asrc").await.unwrap();
        assert!(!stat.is_dir);
        let all = src.read("music/theme.asrc", 0..stat.len).await.unwrap();
        assert_eq!(all.len() as u64, stat.len);
        assert!(std::str::from_utf8(&all)
            .unwrap()
            .starts_with("name=music/theme\n"));
        assert_eq!(src.read("music/theme.asrc", 5..9).await.unwrap().len(), 4);
        assert!(matches!(
            src.stat("music/missing.asrc").await.unwrap_err().kind(),
            ErrorKind::NotFound { .. }
        ));
        assert!(src.read("nope", 0..1).await.is_err());
    }
}
