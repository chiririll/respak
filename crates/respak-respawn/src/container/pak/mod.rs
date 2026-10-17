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

mod tables;
pub use tables::*;

use respak_core::reader::{Cursor, Reader};
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

/// A non-negative count from a signed header field.
fn count(v: i64, what: &str) -> Result<u64> {
    u64::try_from(v).map_err(|_| Error::corrupt(format!("rpak header: negative {what}")))
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
        let d = Reader::new("rpak", data);
        let version = d.u16(4)?;
        let unsupported = |what: String| {
            Error::new(ErrorKind::UnsupportedVersion {
                format: "rpak".into(),
                version: what,
            })
        };
        let header_len = Self::len_for(version).ok_or_else(|| unsupported(version.to_string()))?;
        if data.len() < header_len {
            return Err(Error::corrupt("rpak file is shorter than its header"));
        }
        let flags = d.u16(6)?;
        let c = |v: i32, w: &str| count(i64::from(v), w);

        let mut h = match version {
            6 => {
                let size = count(d.i64(0x18)?, "size")?;
                Self {
                    version,
                    flags,
                    header_len,
                    compressed_size: size,
                    decompressed_size: size,
                    starpak_buf: c(d.i32(0x30)?, "starpak buffer size")?,
                    opt_starpak_buf: 0,
                    num_segments: c(d.i32(0x34)?, "segment count")?,
                    num_pages: c(d.i32(0x38)?, "page count")?,
                    patch_count: 0,
                    num_pointers: c(d.i32(0x3C)?, "pointer count")?,
                    num_assets: c(d.i32(0x40)?, "asset count")?,
                    num_guid_refs: c(d.i32(0x44)?, "guid ref count")?,
                    num_dependencies: c(d.i32(0x48)?, "dependency count")?,
                    num_external_refs: c(d.i32(0x4C)?, "external ref count")?,
                    external_refs_size: c(d.i32(0x50)?, "external refs size")?,
                    unknown_data: 0,
                    created_time: d.i64(0x08)? as u64,
                }
            }
            7 => Self {
                version,
                flags,
                header_len,
                compressed_size: count(d.i64(0x18)?, "compressed size")?,
                decompressed_size: count(d.i64(0x28)?, "decompressed size")?,
                starpak_buf: c(i32::from(d.i16(0x38)?), "starpak buffer size")?,
                opt_starpak_buf: 0,
                num_segments: u64::from(d.u16(0x3A)?),
                num_pages: u64::from(d.u16(0x3C)?),
                patch_count: c(i32::from(d.i16(0x3E)?), "patch count")?,
                num_pointers: c(d.i32(0x40)?, "pointer count")?,
                num_assets: c(d.i32(0x44)?, "asset count")?,
                num_guid_refs: c(d.i32(0x48)?, "guid ref count")?,
                num_dependencies: c(d.i32(0x4C)?, "dependency count")?,
                num_external_refs: c(d.i32(0x50)?, "external ref count")?,
                external_refs_size: c(d.i32(0x54)?, "external refs size")?,
                unknown_data: 0,
                created_time: d.i64(0x08)? as u64,
            },
            _ => Self {
                version,
                flags,
                header_len,
                compressed_size: count(d.i64(0x18)?, "compressed size")?,
                decompressed_size: count(d.i64(0x30)?, "decompressed size")?,
                starpak_buf: c(i32::from(d.i16(0x48)?), "starpak buffer size")?,
                opt_starpak_buf: c(i32::from(d.i16(0x4A)?), "optional starpak buffer size")?,
                num_segments: u64::from(d.u16(0x4C)?),
                num_pages: u64::from(d.u16(0x4E)?),
                patch_count: c(i32::from(d.i16(0x50)?), "patch count")?,
                num_pointers: c(d.i32(0x54)?, "pointer count")?,
                num_assets: c(d.i32(0x58)?, "asset count")?,
                num_guid_refs: c(d.i32(0x5C)?, "guid ref count")?,
                num_dependencies: c(d.i32(0x60)?, "dependency count")?,
                num_external_refs: c(d.i32(0x64)?, "external ref count")?,
                external_refs_size: c(d.i32(0x68)?, "external refs size")?,
                unknown_data: u64::from(d.i32(0x74)? as u32) + u64::from(d.i32(0x78)? as u32),
                created_time: d.i64(0x08)? as u64,
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
            return Err(Error::corrupt(
                "rpak: decompressed size is smaller than the header",
            ));
        }
        let tables = h.tables_len();
        if tables > h.body_len() {
            return Err(Error::corrupt("rpak: tables are larger than the file"));
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
