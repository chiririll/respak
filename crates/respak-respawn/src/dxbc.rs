//! Texture slot names from a DXBC shader blob.
//!
//! Ported from rsx `shader.cpp` (`ResourceBindingFromDXBlob`), with bounds checks everywhere:
//! rsx trusts every offset in the blob.
//!
//! Layout used (little-endian): `"DXBC"`, 16-byte hash, version, total size, chunk count
//! (u32 at 0x1C), then one u32 offset per chunk; a chunk is `{fourcc, size, data}`. The first
//! `RDEF` chunk holds the resource table: `resCount` at 8, `resOffset` at 0xC (both relative to
//! the chunk data); an entry is 32 bytes `{nameOffset, type, returnType, dimension, numSamples,
//! bindPoint, bindCount, flags}`. Type 2 is a texture. SM5.1 blobs (40-byte entries) are not
//! handled.

use std::collections::BTreeMap;

use respak_core::reader::Reader;
use respak_core::{Error, ErrorKind, Result};

const SIT_TEXTURE: u32 = 2;
const ENTRY_SIZE: usize = 32;

fn corrupt(what: &str) -> Error {
    Error::new(ErrorKind::Corrupt {
        context: format!("DXBC: {what}"),
    })
}

fn u32_at(buf: &[u8], offset: usize) -> Result<u32> {
    Reader::new("DXBC", buf).u32(offset)
}

/// Bind point -> variable name of every texture the blob's RDEF chunk declares.
///
/// A bind point declared twice keeps the first name (as rsx does). Errors (`Corrupt`) if the
/// blob is not DXBC, has no `RDEF` chunk, or any offset leaves the blob.
pub fn texture_slots(blob: &[u8]) -> Result<BTreeMap<u32, String>> {
    if blob.get(..4) != Some(b"DXBC") {
        return Err(corrupt("missing magic"));
    }
    let chunks = u32_at(blob, 0x1C)? as usize;
    let table_end = chunks
        .checked_mul(4)
        .and_then(|n| n.checked_add(0x20))
        .ok_or_else(|| corrupt("chunk count"))?;
    if table_end > blob.len() {
        return Err(corrupt("chunk table past the end"));
    }
    let mut rdef = None;
    let mut seen = Vec::new();
    for i in 0..chunks {
        let at = u32_at(blob, 0x20 + i * 4)? as usize;
        let size = u32_at(
            blob,
            at.checked_add(4).ok_or_else(|| corrupt("chunk offset"))?,
        )? as usize;
        let start = at + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| corrupt("chunk size"))?;
        let data = blob
            .get(start..end)
            .ok_or_else(|| corrupt("chunk past the end"))?;
        if blob.get(at..at + 4) == Some(b"RDEF") {
            rdef = Some(data);
            break;
        }
        seen.push(String::from_utf8_lossy(&blob[at..at + 4]).into_owned());
    }
    let rdef = rdef.ok_or_else(|| {
        corrupt(&format!(
            "no RDEF chunk ({} bytes, chunks: {})",
            blob.len(),
            seen.join(",")
        ))
    })?;

    let count = u32_at(rdef, 8)? as usize;
    let table = u32_at(rdef, 0xC)? as usize;
    let mut slots = BTreeMap::new();
    for i in 0..count {
        let entry = i
            .checked_mul(ENTRY_SIZE)
            .and_then(|n| n.checked_add(table))
            .ok_or_else(|| corrupt("resource table"))?;
        if u32_at(rdef, entry + 4)? != SIT_TEXTURE {
            continue;
        }
        let name_at = u32_at(rdef, entry)? as usize;
        let tail = rdef
            .get(name_at..)
            .ok_or_else(|| corrupt("name past the end"))?;
        let len = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| corrupt("unterminated name"))?;
        let name = std::str::from_utf8(&tail[..len]).map_err(|_| corrupt("name is not UTF-8"))?;
        slots
            .entry(u32_at(rdef, entry + 20)?)
            .or_insert_with(|| name.to_owned());
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::dxbc_blob as blob;

    #[test]
    fn maps_texture_bind_points_to_names_and_skips_other_resources() {
        let b = blob(&[
            ("albedoTexture", 2, 0),
            ("sampler0", 3, 0),
            ("normalTexture", 2, 1),
            ("cb", 0, 2),
        ]);
        let slots = texture_slots(&b).unwrap();
        assert_eq!(
            slots.into_iter().collect::<Vec<_>>(),
            [
                (0, "albedoTexture".to_owned()),
                (1, "normalTexture".to_owned())
            ]
        );
    }

    #[test]
    fn a_duplicate_bind_point_keeps_the_first_name() {
        let slots = texture_slots(&blob(&[("first", 2, 4), ("second", 2, 4)])).unwrap();
        assert_eq!(slots[&4], "first");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(texture_slots(b"").is_err());
        assert!(texture_slots(b"NOPE0000").is_err());
        let good = blob(&[("albedoTexture", 2, 0)]);
        // Every truncation fails cleanly; a byte flip anywhere never panics.
        for n in 0..good.len() {
            let _ = texture_slots(&good[..n]);
        }
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            let _ = texture_slots(&bad);
        }
        let mut huge = good.clone();
        huge[0x1C..0x20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(texture_slots(&huge).is_err());
    }

    #[test]
    fn a_blob_without_rdef_is_an_error() {
        let mut b = blob(&[("t", 2, 0)]);
        let at = b.windows(4).position(|w| w == b"RDEF").unwrap();
        b[at..at + 4].copy_from_slice(b"XXXX");
        assert!(texture_slots(&b).is_err());
    }
}
