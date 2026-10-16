//! A file is a list of chunks sorted by offset; a range read needs only the chunks that overlap it.

use std::ops::Range;

use bytes::{Bytes, BytesMut};

/// One chunk of a file, as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// SHA-1 of the decompressed chunk; also its name on the CDN and in the cache.
    pub sha: [u8; 20],
    /// Offset in the file.
    pub offset: u64,
    /// Decompressed size.
    pub len: u32,
    /// Steam's Adler32 of the decompressed data.
    pub adler: u32,
}

impl ChunkRef {
    /// Offset of the first byte after the chunk.
    pub fn end(&self) -> u64 {
        self.offset + u64::from(self.len)
    }

    /// The SHA-1 as lowercase hex (the chunk's name).
    pub fn hex(&self) -> String {
        self.sha.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Indices of the chunks overlapping `range` (binary search). `range` is clamped to the file;
/// empty for an empty range, an empty file, or a range past EOF.
pub fn covering(chunks: &[ChunkRef], range: Range<u64>) -> Range<usize> {
    if range.start >= range.end {
        return 0..0;
    }
    let first = chunks.partition_point(|c| c.end() <= range.start);
    let end = chunks.partition_point(|c| c.offset < range.end);
    if first >= end {
        0..0
    } else {
        first..end
    }
}

/// Cuts `range` out of the decompressed chunks `data`, which are `chunks[first..first + data.len()]`
/// (what `covering` returned, in order). The range is clamped to the chunks.
pub fn assemble(chunks: &[ChunkRef], first: usize, data: &[Bytes], range: Range<u64>) -> Bytes {
    if data.is_empty() {
        return Bytes::new();
    }
    let base = chunks[first].offset;
    let start = range.start.max(base);
    let end = range.end.min(chunks[first + data.len() - 1].end());
    if start >= end {
        return Bytes::new();
    }
    let (start, end) = ((start - base) as usize, (end - base) as usize);
    if data.len() == 1 {
        return data[0].slice(start..end);
    }
    let mut out = BytesMut::with_capacity(end - start);
    let mut pos = 0usize;
    for part in data {
        let (lo, hi) = (pos.max(start), (pos + part.len()).min(end));
        if lo < hi {
            out.extend_from_slice(&part[lo - pos..hi - pos]);
        }
        pos += part.len();
    }
    out.freeze()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic file of `sizes`-long chunks and its content.
    pub(crate) fn file(sizes: &[u32]) -> (Vec<ChunkRef>, Vec<Bytes>, Vec<u8>) {
        let mut chunks = Vec::new();
        let mut data = Vec::new();
        let mut all = Vec::new();
        let mut offset = 0u64;
        for (i, &len) in sizes.iter().enumerate() {
            let body: Vec<u8> = (0..len).map(|j| (i as u32 * 31 + j) as u8).collect();
            let mut sha = [0u8; 20];
            sha[0] = i as u8;
            chunks.push(ChunkRef {
                sha,
                offset,
                len,
                adler: 0,
            });
            all.extend_from_slice(&body);
            data.push(Bytes::from(body));
            offset += u64::from(len);
        }
        (chunks, data, all)
    }

    fn read(chunks: &[ChunkRef], data: &[Bytes], range: Range<u64>) -> Bytes {
        let c = covering(chunks, range.clone());
        assemble(chunks, c.start, &data[c.clone()], range)
    }

    #[test]
    fn inside_one_chunk() {
        let (chunks, data, all) = file(&[100, 100, 100]);
        assert_eq!(covering(&chunks, 110..120), 1..2);
        assert_eq!(&read(&chunks, &data, 110..120)[..], &all[110..120]);
    }

    #[test]
    fn across_a_boundary() {
        let (chunks, data, all) = file(&[100, 100, 100]);
        assert_eq!(covering(&chunks, 90..210), 0..3);
        assert_eq!(&read(&chunks, &data, 90..210)[..], &all[90..210]);
        assert_eq!(covering(&chunks, 99..101), 0..2);
        assert_eq!(covering(&chunks, 100..101), 1..2);
        assert_eq!(covering(&chunks, 0..100), 0..1);
    }

    #[test]
    fn last_chunk_and_past_eof() {
        let (chunks, data, all) = file(&[100, 100, 37]);
        assert_eq!(covering(&chunks, 230..237), 2..3);
        assert_eq!(&read(&chunks, &data, 230..1000)[..], &all[230..]);
        assert_eq!(covering(&chunks, 237..300), 0..0);
        assert_eq!(covering(&chunks, 500..600), 0..0);
        assert!(read(&chunks, &data, 237..300).is_empty());
    }

    #[test]
    fn empty_file_and_empty_range() {
        assert_eq!(covering(&[], 0..10), 0..0);
        assert!(assemble(&[], 0, &[], 0..10).is_empty());
        let (chunks, data, _) = file(&[100]);
        assert_eq!(covering(&chunks, 50..50), 0..0);
        assert!(read(&chunks, &data, 50..50).is_empty());
    }

    #[test]
    fn random_ranges_match_the_file() {
        let (chunks, data, all) = file(&[64, 1, 200, 33, 128, 7]);
        let total = all.len() as u64;
        let mut x = 12345u64;
        for _ in 0..500 {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let a = (x >> 33) % (total + 10);
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let b = a + (x >> 33) % 300;
            let expect = &all[(a.min(total) as usize)..(b.min(total) as usize)];
            assert_eq!(&read(&chunks, &data, a..b)[..], expect, "{a}..{b}");
        }
    }
}
