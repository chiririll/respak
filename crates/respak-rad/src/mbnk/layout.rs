//! Header, source-entry and language-table layouts of the bank versions.

/// Bytes of the largest bank header (v45), enough to detect and pick a layout.
pub const MBNK_HEADER_LEN: usize = 0xC8;

/// `'CBNK'` as it lies on disk.
pub(super) const MAGIC: &[u8] = b"KNBC";

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
    pub(super) fn entry_size(&self) -> u64 {
        match self.source {
            13 | 28 => 0x58,
            39 => 0x48,
            _ => 0x50,
        }
    }
}

/// Where the fields the reader needs sit in a header layout.
#[derive(Debug, Clone, Copy)]
pub(super) struct HeaderFields {
    pub(super) len: usize,
    /// Offset of the `sources` pointer, and whether it is a 4-byte one (v49) or an 8-byte one.
    pub(super) sources: usize,
    pub(super) sources_wide: bool,
    pub(super) strings: usize,
    pub(super) unlocalised: usize,
    pub(super) localised: usize,
    pub(super) build_tag: usize,
}

pub(super) fn header_fields(header: u8) -> HeaderFields {
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
