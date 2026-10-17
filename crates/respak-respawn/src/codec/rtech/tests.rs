use super::*;

const UI_RPAK: &str = r"C:\Users\chiririll\Projects\rsx\rsx_steam_cache\rpak\1172471_6197768190074397522\paks\win64\ui.rpak";

/// Hand-made stream: size 4, one literal run of 4 bytes.
fn tiny() -> Vec<u8> {
    let mut v = vec![0x02, 0x00, 0x40, 0, 0, 0, 0, 0, 0, 0];
    v.extend_from_slice(b"ABCD");
    v
}

#[test]
fn tiny_stream_roundtrip() {
    let out = decompress(&tiny(), 0, Some(4)).unwrap();
    assert_eq!(out, b"ABCD");
    assert!(matches!(
        decompress(&tiny(), 0, Some(5)),
        Err(RtechError::SizeMismatch { .. })
    ));
}

#[test]
fn truncated_prefixes_do_not_panic() {
    let t = tiny();
    for len in 0..=t.len() {
        let _ = decompress(&t[..len], 0, None);
        let _ = decompress(&t[..len], 8, None);
    }
    assert!(decompress(&t[..12], 0, Some(4)).is_err());
}

#[test]
fn garbage_does_not_panic() {
    let mut s: u64 = 0x1234_5678_9ABC_DEF0;
    for round in 0..3000 {
        let len = (round % 300) + 1;
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            buf.push((s >> 24) as u8);
        }
        for hl in [0usize, 0x58, 0x80] {
            // Keep the announced size small so the test stays fast.
            if let Some(b) = buf.get_mut(hl) {
                *b = (*b & 0xC0) | (round as u8 % 14);
            }
            let _ = decompress(&buf, hl, None);
        }
    }
}

#[test]
#[ignore = "needs local game files"]
fn ui_rpak() {
    let Ok(file) = std::fs::read(UI_RPAK) else {
        eprintln!("ui.rpak not found, skipping");
        return;
    };
    let le32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let le64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let dcmp = le64(&file, 0x30);
    let t = std::time::Instant::now();
    let out = decompress(&file, 0x80, Some(dcmp - 0x80)).unwrap();
    eprintln!("decompressed {} bytes in {:?}", out.len(), t.elapsed());
    assert_eq!(out.len() as u64, dcmp - 0x80);

    // v8 table region (header fields: numSegments / numPages at 0x3C).
    let num_segments = u16::from_le_bytes([file[0x4C], file[0x4D]]) as usize;
    let num_pages = u16::from_le_bytes([file[0x4E], file[0x4F]]) as usize;
    assert_eq!((num_segments, num_pages), (6, 63));

    // Segment headers: { flags: u32, align: u32, size: u64 }.
    let mut seg_sizes = Vec::new();
    for s in 0..num_segments {
        let o = s * 16;
        let (align, size) = (le32(&out, o + 4), le64(&out, o + 8));
        assert!(
            align.is_power_of_two() && align <= 0x1000,
            "segment {s} align {align}"
        );
        assert!(size > 0 && size < dcmp, "segment {s} size {size}");
        seg_sizes.push(size);
    }
    // Page headers: { segment: u32, align: u32, size: u32 }.
    let mut per_seg = vec![0u64; num_segments];
    let mut pages_total = 0u64;
    for p in 0..num_pages {
        let o = num_segments * 16 + p * 12;
        let (seg, align, size) = (le32(&out, o) as usize, le32(&out, o + 4), le32(&out, o + 8));
        assert!(seg < num_segments && align.is_power_of_two(), "page {p}");
        per_seg[seg] += u64::from(size);
        pages_total += u64::from(size);
    }
    eprintln!("segment sizes {seg_sizes:x?}, page sums {per_seg:x?}");
    for (s, (&seg, &pg)) in seg_sizes.iter().zip(&per_seg).enumerate() {
        // Segment size is the page sizes plus alignment padding.
        assert!(
            seg >= pg && seg - pg < pg / 8 + 4096 * 64,
            "segment {s}: {seg:#x} vs pages {pg:#x}"
        );
    }
    let tables = (num_segments * 16 + num_pages * 12) as u64;
    eprintln!(
        "tables {tables:#x}, pages {pages_total:#x}, out {:#x}",
        out.len()
    );
    assert!(tables + pages_total <= out.len() as u64);
    assert!(out.len() as u64 - tables - pages_total < out.len() as u64 / 4);
}

#[test]
fn a_partial_input_asks_for_more_instead_of_guessing() {
    let t = tiny();
    for len in 0..t.len() {
        match decompress_prefix(&t[..len], false, 0, Some(4), u64::MAX) {
            Err(RtechError::NeedInput { at_least }) => {
                assert!(at_least > len as u64, "{len}: {at_least}")
            }
            other => panic!("{len} bytes of input: {other:?}"),
        }
    }
    let whole = decompress_prefix(&t, true, 0, Some(4), u64::MAX).unwrap();
    assert_eq!(
        (whole.body.as_slice(), whole.complete),
        (&b"ABCD"[..], true)
    );
}

/// Prefixes of a real v8 pak: equal to the start of the whole body, and a small prefix needs only
/// a small part of the file.
#[test]
#[ignore = "needs local game files"]
fn ui_rpak_prefixes() {
    let Ok(file) = std::fs::read(UI_RPAK) else {
        eprintln!("ui.rpak not found, skipping");
        return;
    };
    let dcmp = u64::from_le_bytes(file[0x30..0x38].try_into().unwrap());
    let len = dcmp - 0x80;
    let whole = decompress(&file, 0x80, Some(len)).unwrap();
    for want in [1u64, 4096, 1 << 20, 10 << 20, len] {
        let mut input = 1usize << 20;
        let prefix = loop {
            let part = &file[..input.min(file.len())];
            match decompress_prefix(part, part.len() == file.len(), 0x80, Some(len), want) {
                Ok(p) => break p,
                Err(RtechError::NeedInput { at_least }) => {
                    input = (at_least as usize).max(input * 2)
                }
                Err(e) => panic!("want {want}: {e}"),
            }
        };
        assert!(
            prefix.body.len() as u64 >= want.min(len),
            "want {want}, got {}",
            prefix.body.len()
        );
        assert_eq!(prefix.body, whole[..prefix.body.len()], "want {want}");
        assert_eq!(prefix.complete, prefix.body.len() as u64 == len);
        eprintln!(
            "want {want}: {} bytes decoded from {} of {} input bytes",
            prefix.body.len(),
            input.min(file.len()),
            file.len()
        );
        if want <= 1 << 20 {
            assert!(
                input < file.len() / 4,
                "a small prefix should not need most of the file"
            );
        }
    }
}
