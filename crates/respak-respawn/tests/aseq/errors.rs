//! Errors and dependency metadata.

use super::*;

// ---- errors ------------------------------------------------------------------------------------

#[tokio::test]
async fn a_sequence_without_a_rig_is_a_missing_dependency() {
    let env = common::env();
    let (_, value) = load(&env, pak_with(14, &one_anim(1), &[], None, false)).await;
    let e = value.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn a_missing_asqd_fails_the_decode_not_the_sequence() {
    let env = common::env();
    let (cx, value) = loaded(&env, pak_with(14, &one_anim(2), &[], None, true)).await;
    let animation = value.as_animation().unwrap();
    assert_eq!(animation.info(0).unwrap().frame_count, 2);
    let e = animation.track(0, 0, &cx).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );
    let e = animation.sample(0, 0, 0, &cx).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );

    // an asqd without a data pointer
    let env = common::env();
    let mut p = PakBuilder::new();
    p.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    p.asqd_without_data(DATA);
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(2)),
        &[],
        None,
    );
    env.input.write("p.rpak", p.build(8)).await.unwrap();
    let (cx, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await;
    let e = value
        .unwrap()
        .as_animation()
        .unwrap()
        .track(0, 0, &cx)
        .await
        .unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn datapoint_animations_are_unsupported() {
    let env = common::env();
    let mut anim = aseq::Anim::new("dp", 2, DATA);
    anim.flags |= 0x200000;
    let seq = aseq::Seq::new("dp", vec![anim]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[(DATA, vec![0; 16])], None, true)).await;
    let animation = value.as_animation().unwrap();
    assert!(animation.info(0).is_ok());
    let e = animation.track(0, 0, &cx).await.unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn sanity_check_failures_and_empty_grids_are_corrupt() {
    for mutate in [
        (|s: &mut aseq::Seq| s.anims[0].fps = 4096.0) as fn(&mut aseq::Seq),
        |s| s.anims[0].frames = 0x20001,
        |s| s.group = Some([0, 1]),
    ] {
        let env = common::env();
        let mut seq = one_anim(2);
        mutate(&mut seq);
        let mut p = PakBuilder::new();
        p.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
        p.aseq(SEQ, "anim/walk.rseq", 14, &aseq::seq_block(&seq), &[], None);
        env.input.write("p.rpak", p.build(8)).await.unwrap();
        let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await;
        let e = value.unwrap_err();
        assert!(matches!(e.kind(), ErrorKind::Corrupt { .. }), "{e}");
    }
}

#[tokio::test]
async fn a_section_that_runs_past_its_data_is_corrupt() {
    let env = common::env();
    // the bone says it has 0x1000 bytes of data, the pak ends first
    let mut block = aseq::section(true, &[(1, vec![]), (0, vec![])]);
    block.extend(0x1000u16.to_le_bytes());
    let (cx, value) = loaded(
        &env,
        pak_with(14, &one_anim(2), &[(DATA, block)], None, true),
    )
    .await;
    let e = value
        .as_animation()
        .unwrap()
        .track(0, 0, &cx)
        .await
        .unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::Corrupt { .. }), "{e}");
}

#[tokio::test]
async fn random_asqd_bytes_never_panic() {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..40 {
        let env = common::env();
        let blob: Vec<u8> = (0..(8 + round * 11)).map(|_| next() as u8).collect();
        let mut anim = aseq::Anim::new("noise", 6, DATA);
        if round % 2 == 0 {
            anim.stall = 2;
            anim.section_frames = 2;
            anim.sections = vec![(next() % 40) as i32, (next() % 40) as i32, !0];
        }
        let (cx, value) = loaded(
            &env,
            pak_with(
                if round % 3 == 0 { 13 } else { 14 },
                &aseq::Seq::new("n", vec![anim]),
                &[(DATA, blob)],
                Some(&[1, 2, 3]),
                true,
            ),
        )
        .await;
        let animation = value.as_animation().unwrap();
        for bone in 0..2 {
            let _ = animation.track(0, bone, &cx).await;
            let _ = animation.sample(0, bone, (next() % 6) as u32, &cx).await;
        }
    }
}

#[test]
fn random_headers_and_blocks_never_panic() {
    let mut seed = 0xDEAD_BEEF_CAFE_F00Du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..500 {
        let h: Vec<u8> = (0..64).map(|_| next() as u8).collect();
        let _ = AseqHeader::parse(Version::new(14), &h);
        let _ = ArigHeader::parse(4 + (next() % 5) as u32, &h[..0x28]);
        let _ = AsqdHeader::parse(&h[..8]);
        let block: Vec<u8> = (0..0x600).map(|_| next() as u8).collect();
        let at = (next() % 0x700) as usize;
        let _ = SeqDesc::parse(&block, at, (next() % 5) as usize);
        let _ = AnimDesc::parse(&block, at);
    }
}

// ---- metadata and dependencies -------------------------------------------------------------------

#[tokio::test]
async fn metadata_lists_the_files_a_sequence_needs() {
    let env = common::env();
    let mut seq = aseq::Seq::new(
        "walk",
        vec![
            aseq::Anim::new("a", 1, DATA),
            aseq::Anim::new("b", 1, 0x21),
            aseq::Anim::new("c", 1, 0),
        ],
    );
    seq.autolayers = vec![0xAA];
    let (_, value) = loaded(&env, pak_with(14, &seq, &[], None, true)).await;
    let m = value.as_metadata().unwrap();
    assert_eq!(m.name(), Some("anim/walk.rseq"));
    assert_eq!(m.kind(), Some("aseq"));
    assert_eq!(m.guid(), Some(SEQ));
    let deps: Vec<u64> = m.dependencies().iter().map(|d| d.guid).collect();
    assert_eq!(deps, [DATA, 0x21, 0xAA, 0x77]);
    let asset = value.downcast_ref::<AseqAsset>().unwrap();
    assert_eq!(asset.rig(), RIG);
    assert_eq!(asset.anims().len(), 3);
    assert_eq!(asset.seq().label, "walk");
    assert_eq!(asset.header().version.major, 14);
}
