/// A starpak file: the entries at their offsets (zero-filled between), then the table of
/// contents and its count.
pub fn build_starpak(entries: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (offset, bytes) in entries {
        out.resize(*offset as usize, 0);
        out.extend_from_slice(bytes);
    }
    while out.len() % 4096 != 0 {
        out.push(0);
    }
    for (offset, bytes) in entries {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    }
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    out
}
