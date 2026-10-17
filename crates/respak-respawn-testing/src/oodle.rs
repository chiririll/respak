use crate::bytes::put;

/// An Oodle stream of stored (uncompressed) Kraken blocks. A block starts a new segment (restart
/// bit) when its index is a multiple of `restart_blocks`; 0 means only the first one does.
pub fn oodle_stored(data: &[u8], restart_blocks: usize) -> Vec<u8> {
    const BLOCK: usize = 0x40000;
    let mut out = Vec::new();
    for (i, chunk) in data.chunks(BLOCK).enumerate() {
        let restart = i == 0 || (restart_blocks != 0 && i % restart_blocks == 0);
        out.push(0x4C | if restart { 0x80 } else { 0 });
        out.push(0x06);
        out.extend_from_slice(chunk);
    }
    out
}

/// Turns a pak built uncompressed into an Oodle one: the body becomes stored blocks and the
/// header says so (`compressed size` is the body alone). The body is padded with zeros to
/// `pad_to` bytes first, to make several blocks.
pub fn oodle_pak(plain: &[u8], restart_blocks: usize, pad_to: usize) -> Vec<u8> {
    let version = u16::from_le_bytes([plain[4], plain[5]]);
    let header_len = if version == 8 { 0x80 } else { 0x58 };
    let mut body = plain[header_len..].to_vec();
    if body.len() < pad_to {
        body.resize(pad_to, 0);
    }
    let stream = oodle_stored(&body, restart_blocks);
    let mut out = plain[..header_len].to_vec();
    let flags = u16::from_le_bytes([out[6], out[7]]) | (1 << 9);
    put(&mut out, 6, &flags.to_le_bytes());
    put(&mut out, 0x18, &(stream.len() as i64).to_le_bytes());
    let decompressed = (header_len + body.len()) as i64;
    put(
        &mut out,
        if version == 8 { 0x30 } else { 0x28 },
        &decompressed.to_le_bytes(),
    );
    out.extend_from_slice(&stream);
    out
}
