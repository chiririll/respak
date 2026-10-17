#[derive(Default)]
pub struct EditScript {
    bits: Vec<bool>,
    replacement: Vec<u8>,
}

impl EditScript {
    pub fn new() -> Self {
        Self::default()
    }

    fn put(&mut self, value: u64, n: u32) {
        for i in 0..n {
            self.bits.push(value >> i & 1 == 1);
        }
    }

    fn code(&mut self, value: u8) {
        // The 3-bit code of `value`, stored bit-reversed because the stream is read LSB first.
        let reversed = (0..3).fold(0u64, |r, i| r | u64::from(value >> i & 1) << (2 - i));
        self.put(reversed, 3);
    }

    fn counted(&mut self, command: u8, n: u64) -> &mut Self {
        assert!((1..256).contains(&n));
        let e = 63 - n.leading_zeros() as u8;
        self.code(command);
        self.code(e);
        self.put(n - (1 << e), u32::from(e));
        self
    }

    /// Command 0: copy `n` bytes of the files.
    pub fn copy(&mut self, n: u64) -> &mut Self {
        self.counted(0, n)
    }

    /// Command 1: skip `n` bytes of the files.
    pub fn skip(&mut self, n: u64) -> &mut Self {
        self.counted(1, n)
    }

    /// Command 2: insert bytes (the files stay where they are).
    pub fn insert(&mut self, bytes: &[u8]) -> &mut Self {
        self.replacement.extend_from_slice(bytes);
        self.counted(2, bytes.len() as u64)
    }

    /// Command 3: bytes that replace the same number of bytes of the files.
    pub fn replace(&mut self, bytes: &[u8]) -> &mut Self {
        self.replacement.extend_from_slice(bytes);
        self.counted(3, bytes.len() as u64)
    }

    /// Command 4, 5 or 6: one byte (two for 6) of replacement, then a copy of 3, 7 or 6 bytes.
    pub fn patched_copy(&mut self, command: u8, replacement: &[u8]) -> &mut Self {
        assert!(
            (4..=6).contains(&command) && replacement.len() == if command == 6 { 2 } else { 1 }
        );
        self.replacement.extend_from_slice(replacement);
        self.code(command);
        self
    }

    /// A command by number, with no count (for streams that must be rejected).
    pub fn raw_command(&mut self, command: u8) -> &mut Self {
        self.code(command);
        self
    }

    /// The stream: the two tables, the offset word, the bits, the replacement data.
    pub fn finish(&self) -> Vec<u8> {
        // Entry: symbol << 3 | (code length - 1). Symbols 0 to 7 in order get the codes 0 to 7.
        let table: Vec<u8> = (0..8u8).map(|symbol| symbol << 3 | 2).collect();
        let mut word = vec![false; 24];
        word.extend(&self.bits);
        let mut bits: Vec<u8> = word
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0u8, |b, (i, &set)| b | u8::from(set) << i)
            })
            .collect();
        while bits.len() < 8 {
            bits.push(0);
        }
        let offset = bits.len() as u32;
        bits[..3].copy_from_slice(&offset.to_le_bytes()[..3]);
        let mut out = table.clone();
        out.extend(&table);
        out.extend(&bits);
        out.extend(&self.replacement);
        out
    }
}
