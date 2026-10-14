//! The Bink decoding side of NihAV. Everything NihAV returns is converted to respak's own
//! types here; its errors become `Corrupt` / `UnsupportedVersion`, and a panic inside the
//! decoder is caught and becomes `Corrupt`.
//!
//! NihAV's demuxer borrows its reader, so none of its objects can be kept between calls. Each
//! function therefore builds the reader, demuxer and decoder, runs to the end and drops them; the
//! caller ([`crate::bik`]) runs it on a blocking thread and receives the results through the
//! `sink` callback (which returns `false` to stop early).
//!
//! Decoders run on the whole file, in memory, front to back (frames depend on each other).

use std::panic::{catch_unwind, AssertUnwindSafe};

use bytes::Bytes;
use nihav_core::codecs::{NADecoderSupport, RegisteredDecoders};
use nihav_core::demuxers::{create_demuxer, Demuxer, DemuxerError, RegisteredDemuxers};
use nihav_core::frame::{NACodecTypeInfo, NAVideoBufferRef};
use nihav_core::io::byteio::MemoryReader;
use respak_core::capability::{AudioChunk, AudioTrackInfo, FrameFormat, VideoFrame};
use respak_core::{Error, ErrorKind, Result};

fn corrupt(context: impl Into<String>) -> Error {
    Error::new(ErrorKind::Corrupt { context: context.into() })
}

fn unsupported(version: impl Into<String>) -> Error {
    Error::new(ErrorKind::UnsupportedVersion { format: "bik".into(), version: version.into() })
}

/// What the container says about its streams.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Probed {
    /// Width and height of the first video stream.
    pub size: (u32, u32),
    /// The audio tracks, in order.
    pub audio: Vec<AudioTrackInfo>,
}

/// Builds the reader and demuxer on `file` and runs `f` with them. Panics inside NihAV become
/// `Corrupt`.
fn with_demuxer<T>(file: &[u8], f: impl FnOnce(&mut Demuxer<'_>, &RegisteredDecoders) -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut demuxers = RegisteredDemuxers::new();
        nihav_rad::rad_register_all_demuxers(&mut demuxers);
        let mut decoders = RegisteredDecoders::new();
        nihav_rad::rad_register_all_decoders(&mut decoders);
        let creator = demuxers.find_demuxer("bink").ok_or_else(|| corrupt("bik: the Bink demuxer is missing"))?;
        let mut reader = MemoryReader::new_read(file);
        let mut demuxer = create_demuxer(creator, &mut reader).map_err(|e| corrupt(format!("bik: container: {e:?}")))?;
        f(&mut demuxer, &decoders)
    }))
    .unwrap_or_else(|_| Err(corrupt("bik: the decoder panicked")))
}

/// Opens the container and lists its streams.
pub(crate) fn probe(file: &[u8]) -> Result<Probed> {
    with_demuxer(file, |dmx, _| {
        let mut size = None;
        let mut audio = Vec::new();
        for i in 0..dmx.get_num_streams() {
            let Some(stream) = dmx.get_stream(i) else { continue };
            match stream.get_info().get_properties() {
                NACodecTypeInfo::Video(v) if size.is_none() => size = Some((v.get_width() as u32, v.get_height() as u32)),
                NACodecTypeInfo::Audio(a) => {
                    audio.push(AudioTrackInfo { channels: u16::from(a.get_channels()), sample_rate: a.get_sample_rate() })
                }
                _ => {}
            }
        }
        let size = size.ok_or_else(|| corrupt("bik: no video stream"))?;
        Ok(Probed { size, audio })
    })
}

/// Decodes the first video stream, handing each frame (converted to `format`) to `sink`. A
/// failure (damage, truncation, an unsupported layout) is handed to `sink` as the last item.
pub(crate) fn decode_video(file: &[u8], format: FrameFormat, sink: &mut dyn FnMut(Result<VideoFrame>) -> bool) {
    let done = with_demuxer(file, |dmx, decoders| {
        let mut found = None;
        for i in 0..dmx.get_num_streams() {
            if let Some(stream) = dmx.get_stream(i).filter(|s| s.get_info().is_video()) {
                found = Some((stream.get_num(), stream.get_info()));
                break;
            }
        }
        let (stream, info) = found.ok_or_else(|| corrupt("bik: no video stream"))?;
        let create = decoders.find_decoder(info.get_name()).ok_or_else(|| unsupported(info.get_name()))?;
        let mut decoder = create();
        let mut support = Box::new(NADecoderSupport::new());
        decoder.init(&mut support, info).map_err(|e| corrupt(format!("bik: video decoder: {e:?}")))?;

        let mut index = 0u32;
        loop {
            let packet = match dmx.get_frame() {
                Ok(p) => p,
                Err(DemuxerError::EOF) => return Ok(()),
                Err(e) => return Err(corrupt(format!("bik: frame {index}: {e:?}"))),
            };
            if packet.get_stream().get_num() != stream {
                continue;
            }
            let frame = decoder
                .decode(&mut support, &packet)
                .map_err(|e| corrupt(format!("bik: frame {index}: {e:?}")))?;
            let buffer = frame.get_buffer().get_vbuf().ok_or_else(|| unsupported("video that is not 8 bit"))?;
            let out = convert(&buffer, index, format)?;
            index += 1;
            if !sink(Ok(out)) {
                return Ok(());
            }
        }
    });
    if let Err(e) = done {
        sink(Err(e));
    }
}

/// Copies a decoded buffer into tightly packed planes in the requested format.
fn convert(buffer: &NAVideoBufferRef<u8>, index: u32, format: FrameFormat) -> Result<VideoFrame> {
    let (width, height) = buffer.get_dimensions(0);
    if buffer.get_info().get_format().get_num_comp() < 3 {
        return Err(unsupported("video without chroma"));
    }
    let chroma = (width.div_ceil(2), height.div_ceil(2));
    if buffer.get_dimensions(1) != chroma || buffer.get_dimensions(2) != chroma {
        return Err(unsupported("video that is not YUV 4:2:0"));
    }
    let data = buffer.get_data();
    let mut planes = Vec::with_capacity(3);
    for plane in 0..3 {
        let (w, h) = buffer.get_dimensions(plane);
        let (offset, stride) = (buffer.get_offset(plane), buffer.get_stride(plane));
        let mut out = Vec::with_capacity(w * h);
        for row in 0..h {
            let start = offset + row * stride;
            out.extend_from_slice(data.get(start..start + w).ok_or_else(|| corrupt("bik: frame buffer is too short"))?);
        }
        planes.push(Bytes::from(out));
    }
    let strides = vec![width, chroma.0, chroma.0];
    match format {
        FrameFormat::Yuv420 => Ok(VideoFrame { index, format, planes, strides }),
        FrameFormat::Rgba8 => {
            let rgba = yuv420_to_rgba8(&planes, &strides, width as u32, height as u32);
            Ok(VideoFrame { index, format, planes: vec![Bytes::from(rgba)], strides: vec![width * 4] })
        }
        _ => Err(unsupported("this frame format")),
    }
}

/// Decodes audio track `track` (counting audio streams from 0), handing the samples of each
/// packet to `sink`. A failure is handed to `sink` as the last item.
pub(crate) fn decode_audio(file: &[u8], track: u32, sink: &mut dyn FnMut(Result<AudioChunk>) -> bool) {
    let done = with_demuxer(file, |dmx, decoders| {
        let mut found = None;
        let mut seen = 0;
        for i in 0..dmx.get_num_streams() {
            if let Some(stream) = dmx.get_stream(i).filter(|s| s.get_info().is_audio()) {
                if seen == track {
                    found = Some((stream.get_num(), stream.get_info()));
                    break;
                }
                seen += 1;
            }
        }
        let (stream, info) = found.ok_or_else(|| corrupt(format!("bik: no audio track {track}")))?;
        let create = decoders.find_decoder(info.get_name()).ok_or_else(|| unsupported(info.get_name()))?;
        let mut decoder = create();
        let mut support = Box::new(NADecoderSupport::new());
        decoder.init(&mut support, info).map_err(|e| corrupt(format!("bik: audio decoder: {e:?}")))?;

        let mut packets = 0u32;
        loop {
            let packet = match dmx.get_frame() {
                Ok(p) => p,
                Err(DemuxerError::EOF) => return Ok(()),
                Err(e) => return Err(corrupt(format!("bik: audio packet {packets}: {e:?}"))),
            };
            if packet.get_stream().get_num() != stream {
                continue;
            }
            let frame = decoder
                .decode(&mut support, &packet)
                .map_err(|e| corrupt(format!("bik: audio packet {packets}: {e:?}")))?;
            packets += 1;
            let buffer = frame.get_buffer();
            let chunk = if let Some(a) = buffer.get_abuf_f32() {
                let channels = usize::from(a.get_info().get_channels());
                let data = a.get_data();
                let mut samples = Vec::with_capacity(a.get_length() * channels);
                for i in 0..a.get_length() {
                    for c in 0..channels {
                        let at = a.get_offset(c) + i;
                        samples.push(*data.get(at).ok_or_else(|| corrupt("bik: audio buffer is too short"))?);
                    }
                }
                AudioChunk { samples }
            } else {
                return Err(unsupported("audio that is not 32-bit float"));
            };
            if !sink(Ok(chunk)) {
                return Ok(());
            }
        }
    });
    if let Err(e) = done {
        sink(Err(e));
    }
}

/// Full-range BT.601 YUV 4:2:0 to tightly packed RGBA8 (alpha 255). `planes` are Y, U, V with
/// the given row `strides`; U and V cover 2x2 pixels each (rounded up for odd sizes).
pub(crate) fn yuv420_to_rgba8(planes: &[Bytes], strides: &[usize], width: u32, height: u32) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let luma = &planes[0][y * strides[0]..][..width];
        let u_row = &planes[1][(y / 2) * strides[1]..];
        let v_row = &planes[2][(y / 2) * strides[2]..];
        for (x, &luma) in luma.iter().enumerate() {
            let luma = i32::from(luma);
            let u = i32::from(u_row[x / 2]) - 128;
            let v = i32::from(v_row[x / 2]) - 128;
            // 16.16 fixed point: 1.402, 0.344136, 0.714136, 1.772.
            let r = luma + ((91_881 * v + 32_768) >> 16);
            let g = luma - ((22_554 * u + 46_802 * v + 32_768) >> 16);
            let b = luma + ((116_130 * u + 32_768) >> 16);
            out.extend_from_slice(&[r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8, 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One flat-colour 2x2 frame in YUV 4:2:0 -> its four RGBA pixels.
    fn flat(y: u8, u: u8, v: u8) -> Vec<u8> {
        let planes = [Bytes::from(vec![y; 4]), Bytes::from(vec![u]), Bytes::from(vec![v])];
        yuv420_to_rgba8(&planes, &[2, 1, 1], 2, 2)
    }

    fn near(actual: &[u8], expected: [u8; 3]) {
        for px in actual.chunks_exact(4) {
            for c in 0..3 {
                assert!(px[c].abs_diff(expected[c]) <= 2, "{px:?} vs {expected:?}");
            }
            assert_eq!(px[3], 255, "alpha");
        }
    }

    #[test]
    fn known_colours_follow_bt601_full_range() {
        near(&flat(0, 128, 128), [0, 0, 0]);
        near(&flat(255, 128, 128), [255, 255, 255]);
        near(&flat(128, 128, 128), [128, 128, 128]);
        near(&flat(76, 85, 255), [255, 0, 0]);
        near(&flat(150, 44, 21), [0, 255, 0]);
        near(&flat(29, 255, 107), [0, 0, 255]);
    }

    #[test]
    fn out_of_gamut_values_clamp() {
        // R and B overflow upwards, G does not: no wrap-around.
        near(&flat(255, 255, 255), [255, 121, 255]);
        // R and B underflow, G overflows by the same amount the other way: no wrap-around.
        near(&flat(0, 0, 0), [0, 136, 0]);
    }

    #[test]
    fn odd_sizes_round_the_chroma_up_and_strides_are_honoured() {
        // 3x3: Y is 3 wide with a stride of 5, U and V are 2x2 with a stride of 4.
        let mut y = vec![0u8; 5 * 3];
        for row in 0..3 {
            for x in 0..3 {
                y[row * 5 + x] = 128;
            }
        }
        let u = vec![128u8; 4 * 2];
        let v = vec![128u8; 4 * 2];
        let rgba = yuv420_to_rgba8(&[Bytes::from(y), Bytes::from(u), Bytes::from(v)], &[5, 4, 4], 3, 3);
        assert_eq!(rgba.len(), 3 * 3 * 4);
        near(&rgba, [128, 128, 128]);
    }

    #[test]
    fn chroma_is_shared_by_each_2x2_block() {
        // 4x2: left block red-ish chroma, right block blue-ish; luma equal.
        let y = Bytes::from(vec![100u8; 8]);
        let u = Bytes::from(vec![128u8, 200]);
        let v = Bytes::from(vec![200u8, 128]);
        let rgba = yuv420_to_rgba8(&[y, u, v], &[4, 2, 2], 4, 2);
        let px = |x: usize, y: usize| &rgba[(y * 4 + x) * 4..][..4];
        assert_eq!(px(0, 0), px(1, 1));
        assert_eq!(px(2, 0), px(3, 1));
        assert!(px(0, 0)[0] > px(0, 0)[2], "left is red-ish");
        assert!(px(2, 0)[2] > px(2, 0)[0], "right is blue-ish");
    }
}
