//! Little-endian writers into a byte buffer.

pub fn put(b: &mut [u8], at: usize, bytes: &[u8]) {
    b[at..at + bytes.len()].copy_from_slice(bytes);
}

pub fn put16(b: &mut [u8], at: usize, v: u16) {
    put(b, at, &v.to_le_bytes());
}

pub fn put32(b: &mut [u8], at: usize, v: u32) {
    put(b, at, &v.to_le_bytes());
}

pub fn put_f(b: &mut [u8], at: usize, v: &[f32]) {
    for (i, f) in v.iter().enumerate() {
        put(b, at + 4 * i, &f.to_le_bytes());
    }
}

pub fn cstr(b: &mut [u8], at: usize, s: &str) {
    put(b, at, s.as_bytes());
}
