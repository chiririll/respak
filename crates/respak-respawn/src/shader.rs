//! `shds` and `shdr` just far enough to name a material's texture slots.
//!
//! Ported from rsx `shaderset.{h,cpp}` and `shader.{h,cpp}`; offsets were derived from the rsx
//! structs. A shader set (`shds`) names its pixel shader by guid; the pixel shader (`shdr`)
//! keeps its DXBC bytecode in the CPU page as a list of `{pointer, i32 size}` entries (16
//! bytes each; a negative size repeats entry `!size`). Only entry 0, the first variant, is
//! read, as rsx does.
//!
//! | `shds` asset version | pixel shader guid at |
//! |---|---|
//! | 8 | 0x50 |
//! | 11 | 0x38 |
//! | 12 | 0x48 |
//! | 13 | 0x68 (0x48 when the header is 80 bytes: the v12 layout) |
//! | 14 | 0x38 |
//! | 15 | 0x48 (rsx never resolves it) |

use bytes::Bytes;
use respak_core::{Error, ErrorKind, Result};

use crate::rpak::{RpakAsset, MEM};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// Biggest shader blob accepted; real ones are a few kilobytes.
const MAX_BYTECODE: u64 = 16 << 20;
const ENTRY_SIZE: u64 = 16;
/// A reference may point this far into the entry list.
const MAX_ENTRIES: i32 = 1024;

/// The pixel shader guid in a shader set header (`version` is the `shds` asset version).
pub fn pixel_shader_guid(version: u32, header: &[u8]) -> Result<u64> {
    let at = match (version, header.len()) {
        (8, _) => 0x50,
        (11, _) => 0x38,
        (12, _) | (13, 80) => 0x48,
        (13, _) => 0x68,
        (14, _) => 0x38,
        (15, _) => 0x48,
        (v, n) => {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "shds".into(),
                version: format!("{v} (header of {n} bytes)"),
            }))
        }
    };
    let bytes = header.get(at..at + 8).ok_or_else(|| corrupt("shds header is too short"))?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("8 bytes")))
}

/// The DXBC bytecode of the first variant of a `shdr` asset.
pub async fn bytecode(asset: &RpakAsset) -> Result<Bytes> {
    let data = asset.data_mem_offset.ok_or_else(|| corrupt("shdr has no data page"))?;
    let mem_len = asset.volume.stat(MEM).await?.content_length();

    let read_entry = |index: u64| async move {
        let at = data.checked_add(index * ENTRY_SIZE).filter(|at| at + ENTRY_SIZE <= mem_len);
        let at = at.ok_or_else(|| corrupt("shdr entry is outside the pak's pages"))?;
        let raw = asset.read_mem(at, ENTRY_SIZE).await?;
        let slot: [u8; 8] = raw[..8].try_into().expect("8 bytes");
        let size = i32::from_le_bytes(raw[8..12].try_into().expect("4 bytes"));
        Ok::<_, Error>((slot, size))
    };

    let (mut slot, mut size) = read_entry(0).await?;
    if size < 0 {
        let index = !size;
        if index == 0 || index > MAX_ENTRIES {
            return Err(corrupt("shdr entry 0 refers to an impossible entry"));
        }
        (slot, size) = read_entry(index as u64).await?;
    }
    if size <= 0 {
        return Err(corrupt("shdr has no bytecode"));
    }
    let len = size as u64;
    let at = asset.mem_offset_of(slot).ok_or_else(|| corrupt("shdr bytecode pointer is null"))?;
    if len > MAX_BYTECODE || at.checked_add(len).is_none_or(|end| end > mem_len) {
        return Err(corrupt("shdr bytecode runs past the end of the pak's pages"));
    }
    asset.read_mem(at, len).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pixel_shader_guid_is_read_at_the_version_s_offset() {
        for (version, len, at) in [(8, 88, 0x50), (11, 64, 0x38), (12, 80, 0x48), (13, 112, 0x68), (13, 80, 0x48), (14, 64, 0x38), (15, 80, 0x48)] {
            let mut h = vec![0u8; len];
            h[at..at + 8].copy_from_slice(&0xFEEDu64.to_le_bytes());
            assert_eq!(pixel_shader_guid(version, &h).unwrap(), 0xFEED, "v{version}");
        }
    }

    #[test]
    fn unknown_versions_and_short_headers_are_errors() {
        assert!(pixel_shader_guid(9, &[0; 100]).is_err());
        assert!(pixel_shader_guid(14, &[0; 0x30]).is_err());
    }
}
