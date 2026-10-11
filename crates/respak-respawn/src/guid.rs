//! Asset GUIDs: the 64-bit hash of an asset path.
//!
//! Ported from rsx `src/game/rtech/utils/utils.cpp` `RTech::StringToGuid` (the two SWAR
//! variants, aligned and unaligned, compute the same thing; this is the byte-wise reading of it).

const MUL_WORD: u64 = 0x0FB8_C4D9_6501;
const MUL_STATE: u64 = 0x0633_D5F1;
const LENGTH_TERM: u64 = 0x00AE_5028_12AA_7333;

/// Hashes an asset path to its GUID.
///
/// The string is read as little-endian 32-bit words, zero-padded after its end. Within each word
/// a backslash becomes a slash and every byte is ANDed with `0xDF`; that clears bit 5, which makes
/// ASCII letters case-insensitive (it also changes some non-letters, which is part of the hash).
pub fn string_to_guid(s: &str) -> u64 {
    let bytes = s.as_bytes();
    // Only bytes before the first NUL take part.
    let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
    let len = bytes.len() as u64;

    let mut h: u64 = 0;
    let mut i = 0;
    loop {
        let mut word = [0u8; 4];
        let mut has_nul = false;
        for (k, slot) in word.iter_mut().enumerate() {
            match bytes.get(i + k) {
                Some(&b) => {
                    let b = if b == b'\\' { b'/' } else { b };
                    *slot = b & 0xDF;
                }
                None => has_nul = true,
            }
        }
        let v = (u64::from(u32::from_le_bytes(word)).wrapping_mul(MUL_WORD)) >> 24;
        if has_nul {
            return h.wrapping_mul(MUL_STATE).wrapping_add(v).wrapping_sub(LENGTH_TERM.wrapping_mul(len));
        }
        h = h.wrapping_mul(MUL_STATE).wrapping_add(v);
        h ^= h >> 61;
        i += 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_and_slashes_do_not_matter() {
        assert_eq!(string_to_guid("Texture/Foo"), string_to_guid("texture/foo"));
        assert_eq!(string_to_guid("texture\\foo"), string_to_guid("texture/foo"));
        assert_ne!(string_to_guid("texture/foo"), string_to_guid("texture/fop"));
    }

    #[test]
    fn length_is_part_of_the_hash() {
        assert_ne!(string_to_guid("abcd"), string_to_guid("abcdabcd"));
        assert_ne!(string_to_guid(""), string_to_guid("a"));
    }
}
