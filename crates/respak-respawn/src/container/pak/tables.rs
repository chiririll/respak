//! The parsed tables of a pak.

use super::*;

/// The parsed tables of a pak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tables {
    /// The patch block, for a patched pak.
    pub patch: Option<PatchBlock>,
    /// Starpak file paths; the index of an entry is the starpak index.
    pub starpaks: Vec<String>,
    /// Optional starpak file paths.
    pub opt_starpaks: Vec<String>,
    /// Segment headers.
    pub segments: Vec<Segment>,
    /// Page headers.
    pub pages: Vec<Page>,
    /// The pointer table: locations of 8-byte slots that hold a `PagePtr`.
    pub pointers: Vec<PagePtr>,
    /// The assets.
    pub assets: Vec<AssetEntry>,
    /// The guid reference table.
    pub guid_refs: Vec<PagePtr>,
    /// Offset of the page data stored in this file from the start of the body. For a patched pak
    /// the edit stream sits before it.
    pub page_data_offset: u64,
    /// Offset of each page from the start of the page data (the `.mem` offset of the page). In a
    /// patched pak the pages before `patch_pages` are not stored in the file.
    pub page_bases: Vec<u64>,
    /// The pointer table sorted by (page, offset), for lookups by location.
    sorted_pointers: Vec<PagePtr>,
}

pub(super) fn strings(buf: &[u8]) -> Vec<String> {
    // Entries are NUL-terminated and the list is padded with NULs; an empty string is padding.
    buf.split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

impl Tables {
    /// Parses the tables from the start of the body (`body` must hold at least
    /// `header.tables_len()` bytes).
    pub fn parse(header: &PakHeader, body: &[u8]) -> Result<Self> {
        let len = usize::try_from(header.tables_len())
            .map_err(|_| Error::corrupt("rpak tables are too large"))?;
        let mut r = Cursor::new(
            "rpak",
            body.get(..len)
                .ok_or_else(|| Error::corrupt("rpak tables run past the data"))?,
        );
        let patch = if header.patch_count > 0 {
            let (stream_size, patch_pages) = (r.i32()?, r.i32()?);
            let stream_size = count(i64::from(stream_size), "patch stream size")?;
            let patch_pages = count(i64::from(patch_pages), "patch page count")?;
            let n = header.patch_count as usize;
            let files = (0..n)
                .map(|_| Ok((r.i64()?, r.i64()?)))
                .collect::<Result<Vec<_>>>()?;
            let indices = (0..n).map(|_| r.u16()).collect::<Result<Vec<_>>>()?;
            if patch_pages > header.num_pages {
                return Err(Error::corrupt(
                    "rpak: the patch block claims more patched pages than the pak has",
                ));
            }
            Some(PatchBlock {
                stream_size,
                patch_pages,
                files,
                indices,
            })
        } else {
            None
        };
        let starpaks = strings(r.take(header.starpak_buf as usize)?);
        let opt_starpaks = strings(r.take(header.opt_starpak_buf as usize)?);

        let mut segments = Vec::with_capacity(header.num_segments as usize);
        for _ in 0..header.num_segments {
            segments.push(Segment {
                flags: r.i32()?,
                align: r.u32()?,
                size: r.i64()?,
            });
        }
        let mut pages = Vec::with_capacity(header.num_pages as usize);
        for _ in 0..header.num_pages {
            pages.push(Page {
                segment: r.i32()?,
                align: r.i32()?,
                size: r.u32()?,
            });
        }
        let page_ptr = |r: &mut Cursor<'_>| -> Result<PagePtr> {
            Ok(PagePtr {
                page: r.u32()?,
                offset: r.u32()?,
            })
        };
        let mut pointers = Vec::with_capacity(header.num_pointers as usize);
        for _ in 0..header.num_pointers {
            pointers.push(page_ptr(&mut r)?);
        }

        let mut assets = Vec::with_capacity(header.num_assets as usize);
        for _ in 0..header.num_assets {
            let entry = r.take(header.asset_entry_len() as usize)?;
            assets.push(Self::asset(header.version, entry)?);
        }
        let mut guid_refs = Vec::with_capacity(header.num_guid_refs as usize);
        for _ in 0..header.num_guid_refs {
            guid_refs.push(page_ptr(&mut r)?);
        }
        // Dependents, external refs and the unknown block are skipped: nothing reads them yet.
        r.skip(header.num_dependencies as usize * 4)?;
        r.skip(header.num_external_refs as usize * 4)?;
        r.skip(header.external_refs_size as usize)?;
        r.skip(header.unknown_data as usize)?;

        let mut page_bases = Vec::with_capacity(pages.len());
        let mut at = 0u64;
        for p in &pages {
            page_bases.push(at);
            at += u64::from(p.size);
        }
        // Only the pages from `patch_pages` on are stored in the file.
        let stored_pages = patch.as_ref().map_or(0, |p| p.patch_pages as usize);
        let stored: u64 = pages[stored_pages..]
            .iter()
            .map(|p| u64::from(p.size))
            .sum();
        let page_data_offset = header
            .tables_len()
            .saturating_add(patch.as_ref().map_or(0, |p| p.stream_size));
        if page_data_offset.saturating_add(stored) > header.body_len() {
            return Err(Error::corrupt("rpak pages run past the end of the data"));
        }
        for a in &assets {
            if !a.head.is_null() && a.head.page as usize >= pages.len() {
                return Err(Error::corrupt(format!(
                    "asset {:#018x}: header page {} does not exist",
                    a.guid, a.head.page
                )));
            }
        }
        let mut sorted_pointers = pointers.clone();
        sorted_pointers.sort_by_key(|p| (p.page, p.offset));
        Ok(Self {
            patch,
            starpaks,
            opt_starpaks,
            segments,
            pages,
            pointers,
            assets,
            guid_refs,
            page_data_offset,
            page_bases,
            sorted_pointers,
        })
    }

    /// Reads one asset entry. `e` is exactly the entry size of `version`.
    fn asset(version: u16, e: &[u8]) -> Result<AssetEntry> {
        let e = Reader::new("rpak", e);
        let ptr_at = |at: usize| -> Result<PagePtr> {
            Ok(PagePtr {
                page: e.u32(at)?,
                offset: e.u32(at + 4)?,
            })
        };
        let kind = |at: usize| -> Result<[u8; 4]> { e.array(at) };
        Ok(if version >= 8 {
            AssetEntry {
                guid: e.u64(0)?,
                head: ptr_at(0x10)?,
                data: ptr_at(0x18)?,
                starpak: e.i64(0x20)?,
                opt_starpak: e.i64(0x28)?,
                dependencies_index: e.u32(0x38)?,
                dependencies_count: u32::from(e.u16(0x40)?),
                header_size: e.u32(0x44)?,
                version: u32::from(e.u8(0x48)?),
                kind: kind(0x4C)?,
            }
        } else {
            AssetEntry {
                guid: e.u64(0)?,
                head: ptr_at(0x10)?,
                data: ptr_at(0x18)?,
                starpak: e.i64(0x20)?,
                opt_starpak: -1,
                dependencies_index: e.u32(0x30)?,
                dependencies_count: e.u32(0x38)?,
                header_size: e.u32(0x3C)?,
                version: e.u32(0x40)?,
                kind: kind(0x44)?,
            }
        })
    }

    /// Total size of the pages: the length of the flat `.mem` file.
    pub fn mem_len(&self) -> u64 {
        self.pages.iter().map(|p| u64::from(p.size)).sum()
    }

    /// The offset in `.mem` (all pages back to back) of a page location, if it lies in a page.
    /// The end of a page is a valid location (a pointer just past the data).
    pub fn mem_offset(&self, ptr: PagePtr) -> Option<u64> {
        let page = self.pages.get(ptr.page as usize)?;
        (ptr.offset <= page.size)
            .then(|| self.page_bases[ptr.page as usize] + u64::from(ptr.offset))
    }

    /// Rewrites every pointer slot inside an asset header into a `.mem` offset (little-endian
    /// u64; `u64::MAX` for a null target). `raw` is the header as stored, `asset.header_size`
    /// bytes. A slot is an 8-byte location listed in the pointer table whose content is a page
    /// location (`i32 page, i32 offset`); only slots wholly inside the header are rewritten.
    pub fn relocate_header(&self, asset: &AssetEntry, raw: &mut [u8]) -> Result<()> {
        if asset.head.is_null() {
            return Ok(());
        }
        let (page, base) = (asset.head.page, asset.head.offset);
        let end = u64::from(base) + raw.len() as u64;
        let first = self
            .sorted_pointers
            .partition_point(|p| (p.page, p.offset) < (page, base));
        for slot in &self.sorted_pointers[first..] {
            if slot.page != page || u64::from(slot.offset) + 8 > end {
                break;
            }
            let at = (slot.offset - base) as usize;
            let slot_bytes = Reader::new("rpak", raw);
            let target = PagePtr {
                page: slot_bytes.u32(at)?,
                offset: slot_bytes.u32(at + 4)?,
            };
            let value = if target.is_null() {
                u64::MAX
            } else {
                self.mem_offset(target).ok_or_else(|| {
                    Error::corrupt(format!(
                        "asset {:#018x}: pointer at +{at:#x} leads to page {} offset {}, which does not exist",
                        asset.guid, target.page, target.offset
                    ))
                })?
            };
            raw[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }

    /// Where in `.mem` the header of `asset` starts, and its length. `None` if the asset has no
    /// header pointer or the header does not fit its page.
    pub fn header_range(&self, asset: &AssetEntry) -> Option<std::ops::Range<u64>> {
        if asset.head.is_null() {
            return None;
        }
        let page = self.pages.get(asset.head.page as usize)?;
        let end_in_page = u64::from(asset.head.offset).checked_add(u64::from(asset.header_size))?;
        if end_in_page > u64::from(page.size) {
            return None;
        }
        let start = self.page_bases[asset.head.page as usize] + u64::from(asset.head.offset);
        Some(start..start + u64::from(asset.header_size))
    }
}
