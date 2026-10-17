//! Versions, headers, descriptions and sections.

use super::*;

// ---- versions and headers ---------------------------------------------------------------------

#[test]
fn only_12_1_to_14_are_supported() {
    let v = |major, created| aseq_variant(major, created).map(|v| (v.major, v.minor));
    assert_eq!(v(12, CREATED_12_1).unwrap(), (12, 1));
    assert_eq!(v(12, u64::MAX).unwrap(), (12, 1));
    assert_eq!(v(13, 0).unwrap(), (13, 0));
    assert_eq!(v(14, 0).unwrap(), (14, 0));
    for (major, created) in [
        (12, CREATED_12_1 - 1),
        (12, 0),
        (7, 0),
        (8, 0),
        (9, 0),
        (10, 0),
        (11, 0),
        (15, 0),
        (0, 0),
    ] {
        let e = v(major, created).unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "{major}: {e}"
        );
    }
}

#[test]
fn the_asset_header_is_64_bytes() {
    let mut h = vec![0u8; 64];
    h[0..8].copy_from_slice(&0x1000u64.to_le_bytes());
    h[8..16].copy_from_slice(&0x2000u64.to_le_bytes());
    h[0x18..0x1A].copy_from_slice(&3u16.to_le_bytes());
    h[0x1A..0x1C].copy_from_slice(&2u16.to_le_bytes());
    h[0x1C..0x20].copy_from_slice(&0x500u32.to_le_bytes());
    h[0x20..0x28].copy_from_slice(&0x3000u64.to_le_bytes());
    h[0x28..0x30].copy_from_slice(&0x3100u64.to_le_bytes());
    h[0x30..0x38].copy_from_slice(&0x3200u64.to_le_bytes());
    h[0x38..0x40].copy_from_slice(&u64::MAX.to_le_bytes());
    let parsed = AseqHeader::parse(Version::new(14), &h).unwrap();
    assert_eq!(parsed.data_offset, 0x1000);
    assert_eq!(parsed.name_offset, Some(0x2000));
    assert_eq!(parsed.models, (Some(0x3000), 3));
    assert_eq!(parsed.effects, Some(0x3100));
    assert_eq!(parsed.settings, (Some(0x3200), 2));
    assert_eq!(parsed.extra, (None, 0x500), "an all-ones pointer is null");

    assert!(matches!(
        AseqHeader::parse(Version::new(14), &h[..56])
            .unwrap_err()
            .kind(),
        ErrorKind::Corrupt { .. }
    ));
    h[0..8].fill(0);
    assert!(
        matches!(
            AseqHeader::parse(Version::new(14), &h).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ),
        "no data"
    );
    assert!(AseqHeader::parse(Version::new(11), &[0; 64]).is_err());
}

#[tokio::test]
async fn unsupported_asset_versions_are_reported_when_read() {
    for version in [7, 9, 11, 15] {
        let env = common::env();
        let (_, value) = load(&env, pak_with(version, &one_anim(1), &[], None, true)).await;
        let e = value.unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "version {version}: {e}"
        );
    }
    // 12 in a pak older than the cut-off is "12.0"
    let env = common::env();
    let mut bytes = pak_with(12, &one_anim(1), &[], None, true);
    bytes[8..16].copy_from_slice(&(CREATED_12_1 - 1).to_le_bytes());
    let (_, value) = load(&env, bytes).await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
}

#[tokio::test]
async fn an_asset_is_detected_as_its_own_kind_only() {
    let env = common::env();
    let mut p = PakBuilder::new();
    p.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    p.asqd(DATA, &[1, 2, 3, 4]);
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(1)),
        &[],
        None,
    );
    env.input.write("p.rpak", p.build(8)).await.unwrap();
    let cx = context(&env, &RespawnPlugin::new());
    for (guid, kind) in [(DATA, "asqd"), (RIG, "arig"), (SEQ, "aseq")] {
        let asset = cx
            .mediator
            .open(&format!("p.rpak/0x{guid:016X}.{kind}"), &cx)
            .await
            .unwrap();
        let hits = cx.mediator.detect(&asset, &cx).await.unwrap();
        let names: Vec<_> = hits.iter().map(|h| h.format.name().to_owned()).collect();
        assert!(names.iter().any(|n| n == kind), "{kind}: {names:?}");
        for other in ["asqd", "arig", "aseq"].into_iter().filter(|o| *o != kind) {
            assert!(
                !names.iter().any(|n| n == other),
                "{kind} also scored as {other}"
            );
        }
    }
}

// ---- sequence and animation descriptions --------------------------------------------------------

#[test]
fn seqdesc_and_animdesc_are_read_from_a_block() {
    let mut seq = aseq::Seq::new(
        "run",
        vec![aseq::Anim::new("a", 10, 0x99), aseq::Anim::new("b", 6, 0)],
    );
    seq.flags = 0x20000;
    seq.weights = aseq::Weights::List(vec![0.25, 0.75]);
    seq.autolayers = vec![0xAA, 0xBB];
    seq.anims[0].flags |= 0x1 | 0x4;
    seq.anims[0].fps = 60.0;
    seq.anims[0].stall = 2;
    seq.anims[0].section_frames = 3;
    seq.anims[0].sections = vec![0x10, -1, 0x30];
    let block = aseq::seq_block(&seq);

    let desc = SeqDesc::parse(&block, 0, 2).unwrap();
    assert_eq!(desc.label, "run");
    assert_eq!(desc.flags, 0x20000);
    assert_eq!(desc.group_size, [2, 1]);
    assert_eq!(desc.anim_offsets, [0x400, 0x800]);
    assert_eq!(desc.weights, Weights::List(vec![0.25, 0.75]));

    let a = AnimDesc::parse(&block, 0x400).unwrap();
    assert_eq!(
        (a.name.as_str(), a.fps, a.frame_count, a.asqd),
        ("a", 60.0, 10, 0x99)
    );
    assert_eq!(a.flags, 0x20000 | 0x5);
    assert_eq!((a.stall_frames, a.section_frames), (2, 3));
    assert_eq!(
        a.sections[..3],
        [
            SectionData::Asqd(0x10),
            SectionData::Extra(0),
            SectionData::Asqd(0x30)
        ]
    );
    let b = AnimDesc::parse(&block, 0x800).unwrap();
    assert_eq!((b.name.as_str(), b.asqd), ("b", 0));

    seq.weights = aseq::Weights::Zero;
    assert_eq!(
        SeqDesc::parse(&aseq::seq_block(&seq), 0, 2)
            .unwrap()
            .weights,
        Weights::Zero
    );
    seq.weights = aseq::Weights::One;
    assert_eq!(
        SeqDesc::parse(&aseq::seq_block(&seq), 0, 2)
            .unwrap()
            .weights,
        Weights::One
    );
}

#[test]
fn bad_descriptions_are_corrupt() {
    let seq = one_anim(5);
    let block = aseq::seq_block(&seq);
    fn corrupt<T: std::fmt::Debug>(r: respak_core::Result<T>) -> bool {
        matches!(r.unwrap_err().kind(), ErrorKind::Corrupt { .. })
    }
    // a block that ends inside the seqdesc / the animdesc
    assert!(corrupt(SeqDesc::parse(&block[..0x40], 0, 1)));
    assert!(corrupt(AnimDesc::parse(&block[..0x420], 0x400)));
    // fps and frame count out of range (rsx's sanity check)
    for (fps, frames) in [
        (4096.0, 5),
        (-1.0, 5),
        (f32::NAN, 5),
        (30.0, 0x20001),
        (30.0, -1),
    ] {
        let mut s = one_anim(frames);
        s.anims[0].fps = fps;
        let block = aseq::seq_block(&s);
        assert!(
            corrupt(AnimDesc::parse(&block, 0x400)),
            "fps {fps}, frames {frames}"
        );
    }
    // a weight list that does not fit the block
    let mut s = one_anim(5);
    s.weights = aseq::Weights::List(vec![1.0; 4]);
    let block = aseq::seq_block(&s);
    assert!(corrupt(SeqDesc::parse(&block[..0x108], 0, 4)));
    // no animations
    let mut s = one_anim(5);
    s.group = Some([0, 1]);
    assert!(corrupt(SeqDesc::parse(&aseq::seq_block(&s), 0, 1)));
}

#[test]
fn sections_are_located_per_frame() {
    let anim = |stall, section_frames, frames, sections: Vec<SectionData>| AnimDesc {
        name: String::new(),
        fps: 30.0,
        flags: 0x20000,
        frame_count: frames,
        asqd: 1,
        stall_frames: stall,
        section_frames,
        sections,
        movement: None,
    };
    let sections = vec![
        SectionData::Asqd(100),
        SectionData::Extra(0),
        SectionData::Asqd(200),
    ];
    let a = anim(4, 4, 10, sections.clone());
    let at = |f| a.locate(f).unwrap();
    assert_eq!(at(0), (SectionData::Asqd(0), 0));
    assert_eq!(at(3), (SectionData::Asqd(0), 3));
    assert_eq!(at(4), (SectionData::Asqd(100), 0));
    assert_eq!(at(7), (SectionData::Asqd(100), 3));
    assert_eq!(at(8), (SectionData::Extra(0), 0));
    assert_eq!(
        at(9),
        (SectionData::Extra(0), 1),
        "the last frame is found like any other (rsx Stall_2); the trailing section is never read"
    );
    assert!(matches!(
        a.locate(10).unwrap_err().kind(),
        ErrorKind::NotFound { .. }
    ));

    // sectionframes = 0: one section, the frame is used as it is
    let flat = anim(0, 0, 10, vec![]);
    assert_eq!(flat.locate(7).unwrap(), (SectionData::Asqd(0), 7));
    let flat = anim(4, 0, 10, vec![]);
    assert_eq!(flat.locate(9).unwrap(), (SectionData::Asqd(0), 9));

    // a single frame
    assert_eq!(
        anim(0, 4, 1, vec![]).locate(0).unwrap(),
        (SectionData::Asqd(0), 0)
    );

    // the table is too short for the frame
    let short = anim(4, 4, 10, vec![SectionData::Asqd(100)]);
    assert!(matches!(
        short.locate(9).unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));
    assert!(matches!(
        short.locate(8).unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));
}
