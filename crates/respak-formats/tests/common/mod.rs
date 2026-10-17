//! Fixtures shared by the format tests: an engine over memory operators and a PNG decoder.
#![allow(dead_code)]

use opendal::{services::Memory, Operator};
use respak_core::Engine;
use respak_formats::FormatsPlugin;

/// An engine with [`FormatsPlugin`] over fresh in-memory input and output.
pub fn env() -> (Engine, Operator, Operator) {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output.clone())
        .plugin(FormatsPlugin)
        .build()
        .unwrap();
    (engine, input, output)
}

/// Decodes a PNG into `(width, height, bytes)`.
pub fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}
