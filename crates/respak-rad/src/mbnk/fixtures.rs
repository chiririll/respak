//! Synthetic banks for every layout, written with the same field table the reader uses (so
//! these tests prove the reader is self-consistent; real banks are the check of the table).

use super::layout::{header_fields, BankLayout, MAGIC};

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
