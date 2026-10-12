//! Decoding pixel data to 8-bit RGBA, for encoders (PNG) that need plain pixels.
//!
//! Block-compressed formats use the `bcdec_rs` crate. Conventions: grey formats (BC4, R8, R16)
//! become `(v, v, v, 255)`; two-channel formats (BC5, RG8) become `(r, g, 0, 255)`; float
//! formats are clamped to 0..1 and scaled (HDR range is lost); signed formats are shifted to
//! unsigned. sRGB and linear variants decode identically: the PNG writer does not tag colour
//! spaces, so the stored values pass through unchanged.

use respak_core::capability::PixelFormat;
use respak_core::{Error, ErrorKind, Result};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

/// `format` cannot be turned into RGBA8 here.
fn unsupported(format: PixelFormat) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: "png".into(), version: format!("pixel format {format:?}") })
}

/// Converts an IEEE half float to `f32`.
fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1F);
    let mant = f32::from(h & 0x03FF);
    match exp {
        0 => sign * mant * 2f32.powi(-24),
        31 => {
            if mant == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + mant / 1024.0) * 2f32.powi(exp - 15),
    }
}

fn unorm8(v: f32) -> u8 {
    if v.is_nan() {
        0
    } else {
        (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    }
}

/// Decodes one mip (`data`, tightly packed in `format`) to `width * height * 4` bytes of RGBA8.
/// A wrong `data` length is `Corrupt`; a format without a conversion is `UnsupportedVersion`.
pub fn decode_rgba8(format: PixelFormat, width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if data.len() as u64 != format.mip_len(width, height) {
        return Err(corrupt(format!("{format:?} mip of {width}x{height} has {} bytes", data.len())));
    }
    let mut out = vec![255u8; w * h * 4];
    if format.is_block_compressed() {
        decode_blocks(format, w, h, data, &mut out)?;
        return Ok(out);
    }
    let px = |i: usize| i * 4;
    match format {
        PixelFormat::Rgba8 | PixelFormat::Rgba8Srgb => out.copy_from_slice(data),
        PixelFormat::Bgra8 => {
            out.copy_from_slice(data);
            for p in out.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
        }
        PixelFormat::R8 => {
            for (i, &v) in data.iter().enumerate() {
                out[px(i)..px(i) + 3].fill(v);
            }
        }
        PixelFormat::Rg8 => {
            for (i, p) in data.chunks_exact(2).enumerate() {
                out[px(i)..px(i) + 3].copy_from_slice(&[p[0], p[1], 0]);
            }
        }
        PixelFormat::R16 => {
            for (i, p) in data.chunks_exact(2).enumerate() {
                out[px(i)..px(i) + 3].fill(p[1]);
            }
        }
        PixelFormat::R16F => {
            for (i, p) in data.chunks_exact(2).enumerate() {
                let v = unorm8(half_to_f32(u16::from_le_bytes([p[0], p[1]])));
                out[px(i)..px(i) + 3].fill(v);
            }
        }
        PixelFormat::R32F => {
            for (i, p) in data.chunks_exact(4).enumerate() {
                let v = unorm8(f32::from_le_bytes([p[0], p[1], p[2], p[3]]));
                out[px(i)..px(i) + 3].fill(v);
            }
        }
        PixelFormat::Rgba16F => {
            for (i, p) in data.chunks_exact(8).enumerate() {
                for c in 0..4 {
                    out[px(i) + c] = unorm8(half_to_f32(u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])));
                }
            }
        }
        PixelFormat::Rgba32F => {
            for (i, p) in data.chunks_exact(16).enumerate() {
                for c in 0..4 {
                    let v = f32::from_le_bytes([p[c * 4], p[c * 4 + 1], p[c * 4 + 2], p[c * 4 + 3]]);
                    out[px(i) + c] = unorm8(v);
                }
            }
        }
        other => return Err(unsupported(other)),
    }
    Ok(out)
}

fn decode_blocks(format: PixelFormat, w: usize, h: usize, data: &[u8], out: &mut [u8]) -> Result<()> {
    use PixelFormat::*;
    let block_bytes = format.bytes_per_unit() as usize;
    let blocks_x = w.div_ceil(4);
    for (n, block) in data.chunks_exact(block_bytes).enumerate() {
        // Each block is decoded into a 4x4 tile and the visible part is copied out, so blocks
        // that stick out of the image (sizes that are not a multiple of 4) need no special case.
        let mut tile = [255u8; 4 * 4 * 4];
        match format {
            Bc1 | Bc1Srgb => bcdec_rs::bc1(block, &mut tile, 16),
            Bc2 | Bc2Srgb => bcdec_rs::bc2(block, &mut tile, 16),
            Bc3 | Bc3Srgb => bcdec_rs::bc3(block, &mut tile, 16),
            Bc7 | Bc7Srgb => bcdec_rs::bc7(block, &mut tile, 16),
            Bc4 | Bc4Snorm => {
                let mut grey = [0u8; 16];
                bcdec_rs::bc4(block, &mut grey, 4, format == Bc4Snorm);
                for (i, &v) in grey.iter().enumerate() {
                    let v = if format == Bc4Snorm { v ^ 0x80 } else { v };
                    tile[i * 4..i * 4 + 3].fill(v);
                }
            }
            Bc5 | Bc5Snorm => {
                let mut rg = [0u8; 32];
                bcdec_rs::bc5(block, &mut rg, 8, format == Bc5Snorm);
                for i in 0..16 {
                    let (r, g) = (rg[i * 2], rg[i * 2 + 1]);
                    let (r, g) = if format == Bc5Snorm { (r ^ 0x80, g ^ 0x80) } else { (r, g) };
                    tile[i * 4..i * 4 + 4].copy_from_slice(&[r, g, 0, 255]);
                }
            }
            Bc6H | Bc6HSigned => {
                let mut rgb = [0f32; 48];
                bcdec_rs::bc6h_float(block, &mut rgb, 12, format == Bc6HSigned);
                for i in 0..16 {
                    for c in 0..3 {
                        tile[i * 4 + c] = unorm8(rgb[i * 3 + c]);
                    }
                }
            }
            other => return Err(unsupported(other)),
        }
        let (bx, by) = ((n % blocks_x) * 4, (n / blocks_x) * 4);
        for row in 0..4.min(h - by) {
            let cols = 4.min(w - bx);
            let dst = ((by + row) * w + bx) * 4;
            out[dst..dst + cols * 4].copy_from_slice(&tile[row * 16..row * 16 + cols * 4]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_solid_colour() {
        // colour0 = 0xF800 (red), colour1 = 0, all indices 0
        let block = [0x00, 0xF8, 0, 0, 0, 0, 0, 0];
        let px = decode_rgba8(PixelFormat::Bc1, 4, 4, &block).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn bc2_explicit_alpha() {
        let mut block = vec![0xAA; 8]; // alpha nibbles 0xA -> 0xAA
        block.extend([0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let px = decode_rgba8(PixelFormat::Bc2, 4, 4, &block).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 0xAA]));
    }

    #[test]
    fn bc3_interpolated_alpha() {
        let mut block = vec![200, 200, 0, 0, 0, 0, 0, 0]; // alpha0 = alpha1 = 200, indices 0
        block.extend([0x00, 0xF8, 0, 0, 0, 0, 0, 0]);
        let px = decode_rgba8(PixelFormat::Bc3, 4, 4, &block).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 200]));
    }

    #[test]
    fn bc4_and_bc5_channels() {
        let r = [100, 100, 0, 0, 0, 0, 0, 0];
        let px = decode_rgba8(PixelFormat::Bc4, 4, 4, &r).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [100, 100, 100, 255]));

        let mut rg = r.to_vec();
        rg.extend([50, 50, 0, 0, 0, 0, 0, 0]);
        let px = decode_rgba8(PixelFormat::Bc5, 4, 4, &rg).unwrap();
        assert!(px.chunks_exact(4).all(|p| p == [100, 50, 0, 255]));
    }

    #[test]
    fn sizes_that_are_not_a_multiple_of_four_are_cropped() {
        // 5x3 needs 2x1 blocks
        let red = [0x00, 0xF8, 0, 0, 0, 0, 0, 0];
        let data = [red, red].concat();
        let px = decode_rgba8(PixelFormat::Bc1, 5, 3, &data).unwrap();
        assert_eq!(px.len(), 5 * 3 * 4);
        assert!(px.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn bc6h_and_bc7_do_not_panic_on_any_block() {
        for fmt in [PixelFormat::Bc6H, PixelFormat::Bc6HSigned, PixelFormat::Bc7] {
            for seed in 0u8..=255 {
                let block: Vec<u8> = (0..16).map(|i| seed.wrapping_mul(31).wrapping_add(i * 17)).collect();
                assert_eq!(decode_rgba8(fmt, 4, 4, &block).unwrap().len(), 64);
            }
        }
    }

    #[test]
    fn plain_formats() {
        assert_eq!(decode_rgba8(PixelFormat::Bgra8, 1, 1, &[1, 2, 3, 4]).unwrap(), [3, 2, 1, 4]);
        assert_eq!(decode_rgba8(PixelFormat::R8, 2, 1, &[7, 9]).unwrap(), [7, 7, 7, 255, 9, 9, 9, 255]);
        assert_eq!(decode_rgba8(PixelFormat::Rg8, 1, 1, &[7, 9]).unwrap(), [7, 9, 0, 255]);
        // half floats: 1.0 = 0x3C00, 0.5 = 0x3800
        let half = [0x00, 0x3C, 0x00, 0x38, 0x00, 0x00, 0x00, 0x3C];
        assert_eq!(decode_rgba8(PixelFormat::Rgba16F, 1, 1, &half).unwrap(), [255, 128, 0, 255]);
    }

    #[test]
    fn wrong_length_is_corrupt() {
        assert!(matches!(decode_rgba8(PixelFormat::Bc1, 4, 4, &[0; 7]).unwrap_err().kind(), ErrorKind::Corrupt { .. }));
    }
}
