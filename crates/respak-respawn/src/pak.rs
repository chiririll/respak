//! `.rpak` header and tables (versions 6, 7 and 8).
//!
//! Ported from rsx `src/game/rtech/cpakfile.{h,cpp}` (`PakHdr_v6_t`/`v7`/`v8`, `PakAsset_v6_t`/`v8`,
//! `PakSegmentHdr_t`, `PakPageHdr_t`, `PagePtr_t`, `LoadNonPatched`, `GetHeaderSize`). See the
//! rsx review in `docs/tasks/07-slice-rpak-listing.md` for what was kept and what was changed.
//!
//! # File layout
//! The header is never compressed (0x58 bytes for v6/v7, 0x80 for v8). Everything after it, the
//! **body**, is stored as is, or RTech-compressed when flag `1 << 8` is set (Oodle `1 << 9` and
//! zstd `1 << 15` are recognised but not supported). The body starts with the tables, in this
//! order, followed by the page data:
//!
//! | table | entry | count |
//! |---|---|---|
//! | starpak paths | NUL-separated strings | `streamingFilesBufSize` bytes |
//! | optional starpak paths (v8) | same | `optStreamingFilesBufSize` bytes |
//! | segments | `i32 flags, u32 align, i64 size` | `numSegments` |
//! | pages | `i32 segment, i32 align, u32 size` | `numPages` |
//! | pointers | `i32 page, i32 offset` | `numPointers` |
//! | assets | 0x48 bytes (v6, v7) or 0x50 (v8) | `numAssets` |
//! | guid refs | `i32 page, i32 offset` | `numGuidRefs` |
//! | dependents | `i32` | `numDependencies` |
//! | external refs | `i32` offsets, then `externalAssetRefsSize` bytes | `numExternalAssetRefs` |
//! | unknown data (v8) | bytes | `unkDataSize_74 + unkDataSize_78` |
//!
//! Pages follow back to back, in page-header order, with no padding.
//!
//! # Patched paks
//! A pak with `patchCount > 0` (v7, v8) is the top of a chain: `common(03).rpak` lists the base
//! `common.rpak` and the patches below it. Its body starts with a patch block, ahead of the
//! starpak paths: `i32 patchDataStreamSize, i32 patchPageCount`, then `i64 cmpSize, i64 dcmpSize`
//! per patch file, then a `u16` file index per patch file (0 = the base file, n = `(nn)`). The
//! tables that follow are the complete, final tables of the patched pak. After them comes the edit
//! stream (`patchDataStreamSize` bytes), then the data of pages `patchPageCount..` only; the
//! earlier pages are rebuilt from the lower files by the edit stream (see `patch.rs`).

use respak_core::{Error, ErrorKind, Result};

/// Flag: the body is RTech-compressed.
pub const FLAG_RTECH: u16 = 1 << 8;
/// Flag: the body is Oodle-compressed.
pub const FLAG_OODLE: u16 = 1 << 9;
/// Flag: the body is zstd-compressed.
pub const FLAG_ZSTD: u16 = 1 << 15;

const MAGIC: &[u8; 4] = b"RPak";
const PAGE_PTR_LEN: u64 = 8;
const SEGMENT_LEN: u64 = 0x10;
const PAGE_LEN: u64 = 0xC;
const PATCH_DATA_HDR_LEN: u64 = 8;
const PATCH_FILE_HDR_LEN: u64 = 16;

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: context.into(),
    })
}

/// A reader over a byte slice that fails with `Corrupt` instead of panicking.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len());
        let end = end.ok_or_else(|| corrupt("rpak table runs past the end of the data"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(self.u64()? as i64)
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
}

fn le_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn le_i16(b: &[u8], at: usize) -> i16 {
    le_u16(b, at) as i16
}
fn le_i32(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}
fn le_i64(b: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

/// A non-negative count from a signed header field.
fn count(v: i64, what: &str) -> Result<u64> {
    u64::try_from(v).map_err(|_| corrupt(format!("rpak header: negative {what}")))
}

/// The pak header, normalised across versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakHeader {
    /// 6, 7 or 8.
    pub version: u16,
    /// Raw flags (see `FLAG_*`).
    pub flags: u16,
    /// Header length in bytes: 0x58 (v6, v7) or 0x80 (v8).
    pub header_len: usize,
    /// Size of the (compressed) file as the header states it. Includes the header.
    pub compressed_size: u64,
    /// Size of the decompressed file image. Includes the header.
    pub decompressed_size: u64,
    /// Bytes of the starpak path list.
    pub starpak_buf: u64,
    /// Bytes of the optional starpak path list (v8).
    pub opt_starpak_buf: u64,
    /// Number of segments.
    pub num_segments: u64,
    /// Number of pages.
    pub num_pages: u64,
    /// Patch file count: 0 for a pak that is not a patch.
    pub patch_count: u64,
    /// Number of pointer table entries.
    pub num_pointers: u64,
    /// Number of assets.
    pub num_assets: u64,
    /// Number of guid reference entries.
    pub num_guid_refs: u64,
    /// Number of dependents entries.
    pub num_dependencies: u64,
    /// Number of external asset refs.
    pub num_external_refs: u64,
    /// Bytes of external asset ref strings.
    pub external_refs_size: u64,
    /// Bytes of unknown data after the external refs (v8).
    pub unknown_data: u64,
    /// When the pak was built, a Windows FILETIME (100 ns since 1601) at header offset 0x08.
    /// Models of some asset versions are told apart by it.
    pub created_time: u64,
}

impl PakHeader {
    /// The bytes needed to parse any supported header (the v8 header length).
    pub const MAX_LEN: usize = 0x80;

    /// Header length for a pak version, if supported.
    pub fn len_for(version: u16) -> Option<usize> {
        match version {
            6 | 7 => Some(0x58),
            8 => Some(0x80),
            _ => None,
        }
    }

    /// Parses the header from the start of the file. `data` may be longer than the header.
    ///
    /// - Not an rpak → `UnknownFormat`.
    /// - A version other than 6, 7 or 8 → `UnsupportedVersion`.
    /// - A truncated or inconsistent header → `Corrupt`.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 8 || &data[..4] != MAGIC {
            return Err(Error::new(ErrorKind::UnknownFormat));
        }
        let version = le_u16(data, 4);
        let unsupported = |what: String| {
            Error::new(ErrorKind::UnsupportedVersion {
                format: "rpak".into(),
                version: what,
            })
        };
        let header_len = Self::len_for(version).ok_or_else(|| unsupported(version.to_string()))?;
        if data.len() < header_len {
            return Err(corrupt("rpak file is shorter than its header"));
        }
        let flags = le_u16(data, 6);
        let c = |v: i32, w: &str| count(i64::from(v), w);

        let mut h = match version {
            6 => {
                let size = count(le_i64(data, 0x18), "size")?;
                Self {
                    version,
                    flags,
                    header_len,
                    compressed_size: size,
                    decompressed_size: size,
                    starpak_buf: c(le_i32(data, 0x30), "starpak buffer size")?,
                    opt_starpak_buf: 0,
                    num_segments: c(le_i32(data, 0x34), "segment count")?,
                    num_pages: c(le_i32(data, 0x38), "page count")?,
                    patch_count: 0,
                    num_pointers: c(le_i32(data, 0x3C), "pointer count")?,
                    num_assets: c(le_i32(data, 0x40), "asset count")?,
                    num_guid_refs: c(le_i32(data, 0x44), "guid ref count")?,
                    num_dependencies: c(le_i32(data, 0x48), "dependency count")?,
                    num_external_refs: c(le_i32(data, 0x4C), "external ref count")?,
                    external_refs_size: c(le_i32(data, 0x50), "external refs size")?,
                    unknown_data: 0,
                    created_time: le_i64(data, 0x08) as u64,
                }
            }
            7 => Self {
                version,
                flags,
                header_len,
                compressed_size: count(le_i64(data, 0x18), "compressed size")?,
                decompressed_size: count(le_i64(data, 0x28), "decompressed size")?,
                starpak_buf: c(i32::from(le_i16(data, 0x38)), "starpak buffer size")?,
                opt_starpak_buf: 0,
                num_segments: u64::from(le_u16(data, 0x3A)),
                num_pages: u64::from(le_u16(data, 0x3C)),
                patch_count: c(i32::from(le_i16(data, 0x3E)), "patch count")?,
                num_pointers: c(le_i32(data, 0x40), "pointer count")?,
                num_assets: c(le_i32(data, 0x44), "asset count")?,
                num_guid_refs: c(le_i32(data, 0x48), "guid ref count")?,
                num_dependencies: c(le_i32(data, 0x4C), "dependency count")?,
                num_external_refs: c(le_i32(data, 0x50), "external ref count")?,
                external_refs_size: c(le_i32(data, 0x54), "external refs size")?,
                unknown_data: 0,
                created_time: le_i64(data, 0x08) as u64,
            },
            _ => Self {
                version,
                flags,
                header_len,
                compressed_size: count(le_i64(data, 0x18), "compressed size")?,
                decompressed_size: count(le_i64(data, 0x30), "decompressed size")?,
                starpak_buf: c(i32::from(le_i16(data, 0x48)), "starpak buffer size")?,
                opt_starpak_buf: c(
                    i32::from(le_i16(data, 0x4A)),
                    "optional starpak buffer size",
                )?,
                num_segments: u64::from(le_u16(data, 0x4C)),
                num_pages: u64::from(le_u16(data, 0x4E)),
                patch_count: c(i32::from(le_i16(data, 0x50)), "patch count")?,
                num_pointers: c(le_i32(data, 0x54), "pointer count")?,
                num_assets: c(le_i32(data, 0x58), "asset count")?,
                num_guid_refs: c(le_i32(data, 0x5C), "guid ref count")?,
                num_dependencies: c(le_i32(data, 0x60), "dependency count")?,
                num_external_refs: c(le_i32(data, 0x64), "external ref count")?,
                external_refs_size: c(le_i32(data, 0x68), "external refs size")?,
                unknown_data: u64::from(le_i32(data, 0x74) as u32)
                    + u64::from(le_i32(data, 0x78) as u32),
                created_time: le_i64(data, 0x08) as u64,
            },
        };
        if h.flags & FLAG_ZSTD != 0 {
            return Err(unsupported(format!("{version} (zstd compression)")));
        }
        if version == 6 {
            // v6 has no compression.
            h.flags &= !(FLAG_RTECH | FLAG_OODLE);
        }
        if h.decompressed_size < h.header_len as u64 {
            return Err(corrupt(
                "rpak: decompressed size is smaller than the header",
            ));
        }
        let tables = h.tables_len();
        if tables > h.body_len() {
            return Err(corrupt("rpak: tables are larger than the file"));
        }
        Ok(h)
    }

    /// Whether the body is Oodle-compressed. Then `compressed_size` is the size of the compressed
    /// body alone, without the header.
    pub fn is_oodle(&self) -> bool {
        self.flags & FLAG_OODLE != 0
    }

    /// Whether the body is RTech-compressed. Then `compressed_size` includes the header.
    pub fn is_rtech(&self) -> bool {
        self.flags & FLAG_RTECH != 0 && !self.is_oodle()
    }

    /// Size of the (decompressed) body, i.e. the image minus the header.
    pub fn body_len(&self) -> u64 {
        self.decompressed_size - self.header_len as u64
    }

    /// Entry size of the asset table.
    pub fn asset_entry_len(&self) -> u64 {
        if self.version >= 8 {
            0x50
        } else {
            0x48
        }
    }

    /// Bytes of the patch block at the start of the body of a patched pak, 0 otherwise.
    pub fn patch_block_len(&self) -> u64 {
        if self.patch_count == 0 {
            0
        } else {
            PATCH_DATA_HDR_LEN
                .saturating_add(self.patch_count.saturating_mul(PATCH_FILE_HDR_LEN + 2))
        }
    }

    /// Bytes from the start of the body to the end of the tables (the patch block and the
    /// tables, but not the edit stream). Saturating, so a hostile header can not overflow.
    pub fn tables_len(&self) -> u64 {
        [
            self.patch_block_len(),
            self.starpak_buf,
            self.opt_starpak_buf,
            self.num_segments.saturating_mul(SEGMENT_LEN),
            self.num_pages.saturating_mul(PAGE_LEN),
            self.num_pointers.saturating_mul(PAGE_PTR_LEN),
            self.num_assets.saturating_mul(self.asset_entry_len()),
            self.num_guid_refs.saturating_mul(PAGE_PTR_LEN),
            self.num_dependencies.saturating_mul(4),
            self.num_external_refs.saturating_mul(4),
            self.external_refs_size,
            self.unknown_data,
        ]
        .into_iter()
        .fold(0u64, u64::saturating_add)
    }
}

/// A location in the pages: page index and offset in it. `{-1, -1}` (all ones) is null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagePtr {
    /// Page index.
    pub page: u32,
    /// Byte offset in the page.
    pub offset: u32,
}

impl PagePtr {
    /// Whether this is the null pointer.
    pub fn is_null(self) -> bool {
        self.page == u32::MAX && self.offset == u32::MAX
    }
}

/// One entry of the asset table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEntry {
    /// The asset's GUID.
    pub guid: u64,
    /// Where the asset header lives.
    pub head: PagePtr,
    /// Where the asset's CPU data lives, if any.
    pub data: PagePtr,
    /// Mandatory starpak reference: `-1` none, low 12 bits starpak index, rest the offset.
    pub starpak: i64,
    /// Optional starpak reference (v8), same encoding.
    pub opt_starpak: i64,
    /// Index of this asset's dependencies in the guid ref table.
    pub dependencies_index: u32,
    /// Number of guid refs this asset has.
    pub dependencies_count: u32,
    /// Size of the asset header in bytes.
    pub header_size: u32,
    /// The asset's version.
    pub version: u32,
    /// Four-character kind tag, in file order (`txtr`, `matl`, ...).
    pub kind: [u8; 4],
}

impl AssetEntry {
    /// The kind as text: the tag without trailing NULs, or hex for non-printable tags.
    pub fn kind_string(&self) -> String {
        let trimmed: Vec<u8> = self.kind.iter().copied().take_while(|&b| b != 0).collect();
        if !trimmed.is_empty() && trimmed.iter().all(|b| b.is_ascii_graphic()) {
            String::from_utf8_lossy(&trimmed).into_owned()
        } else {
            self.kind.iter().map(|b| format!("{b:02x}")).collect()
        }
    }
}

/// A segment header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Segment flags (`SF_CPU = 1`, `SF_TEMP = 2`, ...). The type is `flags & 3`.
    pub flags: i32,
    /// Alignment.
    pub align: u32,
    /// Size including alignment padding.
    pub size: i64,
}

/// A page header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    /// Index of the segment the page belongs to.
    pub segment: i32,
    /// Alignment.
    pub align: i32,
    /// Size in bytes.
    pub size: u32,
}

/// The patch block of a patched pak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchBlock {
    /// Size of the edit stream that follows the tables.
    pub stream_size: u64,
    /// Number of leading pages whose data is not in this file but rebuilt from the lower files.
    pub patch_pages: u64,
    /// Compressed and decompressed size of each lower file, as the block states them.
    pub files: Vec<(i64, i64)>,
    /// File index of each lower file: 0 is the base `<stem>.rpak`, n is `<stem>(nn).rpak`.
    pub indices: Vec<u16>,
}

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

fn strings(buf: &[u8]) -> Vec<String> {
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
            .map_err(|_| corrupt("rpak tables are too large"))?;
        let mut r = Cursor {
            buf: body
                .get(..len)
                .ok_or_else(|| corrupt("rpak tables run past the data"))?,
            pos: 0,
        };
        let patch = if header.patch_count > 0 {
            let (stream_size, patch_pages) = (r.i32()?, r.i32()?);
            let stream_size = count(i64::from(stream_size), "patch stream size")?;
            let patch_pages = count(i64::from(patch_pages), "patch page count")?;
            let n = header.patch_count as usize;
            let files = (0..n)
                .map(|_| Ok((r.i64()?, r.i64()?)))
                .collect::<Result<Vec<_>>>()?;
            let indices = (0..n)
                .map(|_| Ok(u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes"))))
                .collect::<Result<Vec<_>>>()?;
            if patch_pages > header.num_pages {
                return Err(corrupt(
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
            assets.push(Self::asset(header.version, entry));
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
            return Err(corrupt("rpak pages run past the end of the data"));
        }
        for a in &assets {
            if !a.head.is_null() && a.head.page as usize >= pages.len() {
                return Err(corrupt(format!(
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
    fn asset(version: u16, e: &[u8]) -> AssetEntry {
        let u32_at = |at: usize| u32::from_le_bytes(e[at..at + 4].try_into().expect("4 bytes"));
        let ptr_at = |at: usize| PagePtr {
            page: u32_at(at),
            offset: u32_at(at + 4),
        };
        let kind = |at: usize| [e[at], e[at + 1], e[at + 2], e[at + 3]];
        if version >= 8 {
            AssetEntry {
                guid: u64::from_le_bytes(e[..8].try_into().expect("8 bytes")),
                head: ptr_at(0x10),
                data: ptr_at(0x18),
                starpak: le_i64(e, 0x20),
                opt_starpak: le_i64(e, 0x28),
                dependencies_index: u32_at(0x38),
                dependencies_count: u32::from(le_u16(e, 0x40)),
                header_size: u32_at(0x44),
                version: u32::from(e[0x48]),
                kind: kind(0x4C),
            }
        } else {
            AssetEntry {
                guid: u64::from_le_bytes(e[..8].try_into().expect("8 bytes")),
                head: ptr_at(0x10),
                data: ptr_at(0x18),
                starpak: le_i64(e, 0x20),
                opt_starpak: -1,
                dependencies_index: u32_at(0x30),
                dependencies_count: u32_at(0x38),
                header_size: u32_at(0x3C),
                version: u32_at(0x40),
                kind: kind(0x44),
            }
        }
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
            let target = PagePtr {
                page: u32::from_le_bytes(raw[at..at + 4].try_into().expect("4 bytes")),
                offset: u32::from_le_bytes(raw[at + 4..at + 8].try_into().expect("4 bytes")),
            };
            let value = if target.is_null() {
                u64::MAX
            } else {
                self.mem_offset(target).ok_or_else(|| {
                    corrupt(format!(
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
