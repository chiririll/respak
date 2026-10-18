use crate::bytes::put;
use crate::raw::{build_raw_with, RawAsset};

/// A cell of [`PakBuilder::dtbl`]; `Str` is any string-typed cell, `Null` a null string pointer.
pub enum DtblCell<'a> {
    Bool(bool),
    Int(i32),
    Float(f32),
    Vector([f32; 3]),
    Str(&'a str),
    Null,
}
/// Builds a pak with arbitrary assets: page 0 holds the headers, page 1 everything else.
#[derive(Default)]
pub struct PakBuilder {
    heads: Vec<u8>,
    misc: Vec<u8>,
    pointers: Vec<(u32, u32)>,
    assets: Vec<RawAsset>,
    starpaks: Vec<String>,
    opt_starpaks: Vec<String>,
}

impl PakBuilder {
    /// An empty pak.
    pub fn new() -> Self {
        Self::default()
    }

    fn align(&mut self) {
        while !self.misc.len().is_multiple_of(16) {
            self.misc.push(0);
        }
    }

    /// Appends `bytes` to page 1 (16-byte aligned); returns their offset in the page.
    pub fn data(&mut self, bytes: &[u8]) -> u32 {
        self.align();
        let at = self.misc.len() as u32;
        self.misc.extend_from_slice(bytes);
        self.align();
        at
    }

    /// Appends a NUL-terminated string to page 1; returns its offset in the page.
    pub fn string(&mut self, s: &str) -> u32 {
        self.data(&[s.as_bytes(), &[0]].concat())
    }

    /// Makes the 8 bytes at page-1 offset `slot` a pointer to page-1 offset `target`.
    pub fn data_pointer(&mut self, slot: u32, target: u32) {
        self.misc[slot as usize..slot as usize + 4].copy_from_slice(&1u32.to_le_bytes());
        self.misc[slot as usize + 4..slot as usize + 8].copy_from_slice(&target.to_le_bytes());
        self.pointers.push((1, slot));
    }

    /// Adds an asset. `pointers` are `(slot offset in header, target offset in page 1)`; `data`
    /// is the page-1 offset of the asset's CPU data.
    pub fn asset(
        &mut self,
        guid: u64,
        kind: &[u8; 4],
        version: u32,
        mut header: Vec<u8>,
        pointers: &[(usize, u32)],
        data: Option<u32>,
    ) {
        for &(slot, target) in pointers {
            header[slot..slot + 4].copy_from_slice(&1u32.to_le_bytes());
            header[slot + 4..slot + 8].copy_from_slice(&target.to_le_bytes());
        }
        let head_offset = self.heads.len() as u32;
        for &(slot, _) in pointers {
            self.pointers.push((0, head_offset + slot as u32));
        }
        let header_size = header.len() as u32;
        self.heads.extend_from_slice(&header);
        while !self.heads.len().is_multiple_of(16) {
            self.heads.push(0);
        }
        self.assets.push(RawAsset {
            guid,
            kind: *kind,
            version,
            head: (0, head_offset),
            data: data.map(|d| (1, d)),
            header_size,
            starpak: -1,
            opt_starpak: -1,
        });
    }

    /// A `matl` of asset version 16 (240-byte header) with the given texture slots (0 = empty).
    pub fn matl(&mut self, guid: u64, name: &str, shader_set: u64, textures: &[u64]) {
        let mut h = vec![0u8; 240];
        put(&mut h, 0x10, &guid.to_le_bytes());
        put(&mut h, 0x58, &shader_set.to_le_bytes());
        put(&mut h, 0x72, &512u16.to_le_bytes());
        put(&mut h, 0x74, &256u16.to_le_bytes());
        put(&mut h, 0x76, &1u16.to_le_bytes());
        let name_at = self.string(name);
        let array: Vec<u8> = textures
            .iter()
            .flat_map(|g| g.to_le_bytes())
            .chain([0; 8])
            .collect();
        let array_at = self.data(&array);
        self.asset(
            guid,
            b"matl",
            16,
            h,
            &[
                (0x18, name_at),
                (0x60, array_at),
                (0x68, array_at + 8 * textures.len() as u32),
            ],
            None,
        );
    }

    /// A `shds` (asset version 12, 80-byte header) whose pixel shader is `pixel`.
    pub fn shds(&mut self, guid: u64, pixel: u64) {
        let mut h = vec![0u8; 80];
        put(&mut h, 0x48, &pixel.to_le_bytes());
        self.asset(guid, b"shds", 12, h, &[], None);
    }

    /// A `shdr` (asset version 12) whose first variant is `bytecode`.
    pub fn shdr(&mut self, guid: u64, bytecode: &[u8]) {
        let code_at = self.data(bytecode);
        let entry = vec![0u8; 16];
        let entry_at = self.data(&entry);
        self.data_pointer(entry_at, code_at);
        self.misc[entry_at as usize + 8..entry_at as usize + 12]
            .copy_from_slice(&(bytecode.len() as i32).to_le_bytes());
        self.asset(guid, b"shdr", 12, vec![0u8; 32], &[], Some(entry_at));
    }

    /// A `dtbl` of asset `version` (0 or 1); `wide` uses the V1.1 column record (0x18 bytes).
    /// Columns are `(name, type)`; cells are packed in column order, the row stride is their sum.
    pub fn dtbl(
        &mut self,
        guid: u64,
        version: u32,
        wide: bool,
        columns: &[(&str, i32)],
        rows: &[Vec<DtblCell>],
    ) {
        let size = |ty: i32| match ty {
            0 => 1,
            1 | 2 => 4,
            3 => 12,
            _ => 8,
        };
        let offsets: Vec<u32> = columns
            .iter()
            .scan(0, |at, &(_, ty)| {
                Some(std::mem::replace(at, *at + size(ty)))
            })
            .collect();
        let stride = columns.iter().map(|&(_, ty)| size(ty)).sum::<u32>();
        let record = if wide { 0x18 } else { 0x10 };

        let names: Vec<u32> = columns.iter().map(|(name, _)| self.string(name)).collect();
        let mut records = vec![0u8; columns.len() * record];
        for (i, &(_, ty)) in columns.iter().enumerate() {
            let r = &mut records[i * record..(i + 1) * record];
            let at = if wide { 0x10 } else { 0x08 };
            put(r, at, &ty.to_le_bytes());
            put(r, at + 4, &offsets[i].to_le_bytes());
        }
        let records_at = self.data(&records);
        for (i, &name) in names.iter().enumerate() {
            self.data_pointer(records_at + (i * record) as u32, name);
        }

        let mut block = vec![0u8; rows.len() * stride as usize];
        let mut strings = Vec::new();
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let at = r * stride as usize + offsets[c] as usize;
                match cell {
                    DtblCell::Bool(b) => block[at] = u8::from(*b),
                    DtblCell::Int(n) => put(&mut block, at, &n.to_le_bytes()),
                    DtblCell::Float(f) => put(&mut block, at, &f.to_le_bytes()),
                    DtblCell::Vector(v) => {
                        for (k, f) in v.iter().enumerate() {
                            put(&mut block, at + 4 * k, &f.to_le_bytes());
                        }
                    }
                    DtblCell::Str(s) => strings.push((at as u32, self.string(s))),
                    DtblCell::Null => {}
                }
            }
        }
        let rows_at = self.data(&block);
        for (at, target) in strings {
            self.data_pointer(rows_at + at, target);
        }

        let mut h = vec![0u8; if version == 0 { 0x20 } else { 0x28 }];
        put(&mut h, 0, &(columns.len() as i32).to_le_bytes());
        put(&mut h, 4, &(rows.len() as i32).to_le_bytes());
        put(
            &mut h,
            if version == 0 { 0x18 } else { 0x20 },
            &stride.to_le_bytes(),
        );
        self.asset(
            guid,
            b"dtbl",
            version,
            h,
            &[(0x08, records_at), (0x10, rows_at)],
            None,
        );
    }

    /// The pak's starpak path lists (mandatory, optional), as the pak stores them.
    pub fn starpak_files(&mut self, starpaks: &[&str], opt_starpaks: &[&str]) {
        self.starpaks = starpaks.iter().map(|s| (*s).to_owned()).collect();
        self.opt_starpaks = opt_starpaks.iter().map(|s| (*s).to_owned()).collect();
    }

    /// Points the last asset added at entries of starpak 0 and optional starpak 0, by their
    /// 4096-aligned offsets.
    pub fn stream_last(&mut self, starpak: Option<u64>, opt: Option<u64>) {
        let last = self.assets.last_mut().expect("an asset");
        last.starpak = starpak.map_or(-1, |o| o as i64);
        last.opt_starpak = opt.map_or(-1, |o| o as i64);
    }

    /// A `wrap` of asset version 7 (0x28-byte header). `path` is the stored path, `path_size` the
    /// length of the real file name's end in it, `data` the stored bytes (`None`: not in the pak,
    /// the flags must say streamed).
    #[allow(clippy::too_many_arguments)]
    pub fn wrap_v7(
        &mut self,
        guid: u64,
        path: &str,
        path_size: u16,
        skip_first_folder: u16,
        flags: u16,
        cmp_size: u32,
        dcmp_size: u32,
        data: Option<&[u8]>,
    ) {
        let path_at = self.string(path);
        let data_at = data.map(|d| self.data(d));
        let mut h = vec![0u8; 0x28];
        put(&mut h, 0x14, &cmp_size.to_le_bytes());
        put(&mut h, 0x18, &dcmp_size.to_le_bytes());
        put(&mut h, 0x1C, &path_size.to_le_bytes());
        put(&mut h, 0x1E, &skip_first_folder.to_le_bytes());
        put(&mut h, 0x22, &flags.to_le_bytes());
        let mut pointers = vec![(0, path_at)];
        pointers.extend(data_at.map(|d| (8, d)));
        self.asset(guid, b"wrap", 7, h, &pointers, None);
    }

    /// A `wrap` of asset version 1 (0x10-byte header) holding `data`.
    pub fn wrap_v1(&mut self, guid: u64, data: &[u8]) {
        let data_at = self.data(data);
        let mut h = vec![0u8; 0x10];
        put(&mut h, 0, &(data.len() as u32).to_le_bytes());
        self.asset(guid, b"wrap", 1, h, &[(8, data_at)], None);
    }

    /// A `subt` of asset `version` (0 or 1) with one slot per line: `(hash, text)`. A hash of 0
    /// makes an empty slot (its text is not stored).
    pub fn subt(&mut self, guid: u64, version: u32, lines: &[(u32, &str)]) {
        let mut strings = Vec::new();
        let mut entries = Vec::new();
        for &(hash, text) in lines {
            let offset = if hash == 0 { 0 } else { strings.len() as i32 };
            if hash != 0 {
                strings.extend_from_slice(text.as_bytes());
                strings.push(0);
            }
            entries.extend_from_slice(&hash.to_le_bytes());
            entries.extend_from_slice(&offset.to_le_bytes());
        }
        let occupied = lines.iter().filter(|l| l.0 != 0).count() as i32;
        self.subt_raw(
            guid,
            version,
            occupied,
            lines.len() as i32,
            Some(&entries),
            Some(&strings),
        );
    }

    /// A `subt` with the header fields and blocks given as they are; `None` leaves a pointer null.
    pub fn subt_raw(
        &mut self,
        guid: u64,
        version: u32,
        string_count: i32,
        entry_count: i32,
        entries: Option<&[u8]>,
        strings: Option<&[u8]>,
    ) {
        let entries_at = entries.map(|e| self.data(e));
        let strings_at = strings.map(|s| self.data(s));
        let mut h = vec![0u8; 0x18];
        put(&mut h, 0x00, &string_count.to_le_bytes());
        put(&mut h, 0x04, &entry_count.to_le_bytes());
        let mut pointers = Vec::new();
        pointers.extend(entries_at.map(|a| (0x08, a)));
        pointers.extend(strings_at.map(|a| (0x10, a)));
        self.asset(guid, b"subt", version, h, &pointers, None);
    }

    /// The pak file.
    pub fn build(self, pak_version: u16) -> Vec<u8> {
        let starpaks: Vec<&str> = self.starpaks.iter().map(String::as_str).collect();
        let opt: Vec<&str> = self.opt_starpaks.iter().map(String::as_str).collect();
        build_raw_with(
            pak_version,
            &[self.heads, self.misc],
            &self.pointers,
            &self.assets,
            &starpaks,
            &opt,
        )
    }
}
