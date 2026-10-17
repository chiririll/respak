//! Decoding sequences through the pak.

use super::*;

// ---- decoding through the pak ------------------------------------------------------------------

#[tokio::test]
async fn tracks_decode_in_the_4_bit_and_6_bit_variants() {
    for (version, six) in [(12, false), (13, true), (14, true)] {
        let env = common::env();
        let blob = two_bone_section(six);
        let (cx, value) = loaded(
            &env,
            pak_with(version, &one_anim(4), &[(DATA, blob)], None, true),
        )
        .await;
        let animation = value.as_animation().unwrap();
        assert_eq!(animation.blend_count(), 1);
        let info = animation.info(0).unwrap();
        assert_eq!(
            (info.name.as_str(), info.fps, info.frame_count),
            ("walk_anim", 30.0, 4)
        );
        assert!(!info.looping && !info.additive && !info.has_scale);
        let names: Vec<_> = animation.bones().iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["root", "arm"]);

        let root = animation.track(0, 0, &cx).await.unwrap();
        let xs: Vec<f32> = dense(&root.translation).iter().map(|t| t[0]).collect();
        near(&xs, &[6.0, 11.0, 16.0, 21.0]);
        assert!(
            dense(&root.translation)
                .iter()
                .all(|t| t[1] == 2.0 && t[2] == 3.0),
            "the rest pose fills the other axes"
        );
        let expected: Vec<[f32; 4]> = [1000.0f32, 2000.0, 3000.0, 4000.0]
            .iter()
            .map(|v| {
                let half = v * ROT * 0.5;
                [0.0, half.sin(), 0.0, half.cos()]
            })
            .collect();
        for (got, want) in dense(&root.rotation).iter().zip(&expected) {
            near(got, want);
        }
        assert_eq!(constant(&root.scale), [1.0; 3]);

        let arm = animation.track(0, 1, &cx).await.unwrap();
        near(&constant(&arm.rotation), &[0.5, 0.5, 0.5, 0.5]);
        assert_eq!(
            constant(&arm.translation),
            [2.0, 2.0, 3.0],
            "no position data: the rest pose"
        );
        assert_eq!(constant(&arm.scale), [1.0; 3]);

        for frame in 0..4u32 {
            let s = animation.sample(0, 0, frame, &cx).await.unwrap();
            near(&s.translation, &dense(&root.translation)[frame as usize]);
            near(&s.rotation, &expected[frame as usize]);
        }
        assert!(animation.sample(0, 0, 4, &cx).await.is_err());
        assert!(animation.track(0, 2, &cx).await.is_err());
        assert!(animation.track(1, 0, &cx).await.is_err());
        assert!(animation.info(1).is_err());
    }
}

#[tokio::test]
async fn constant_channels_use_vector48_and_quaternion64() {
    let env = common::env();
    let mut data = aseq::vec48([1.5, -2.0, 0.25]).to_vec();
    data.extend(aseq::quat64([
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ]));
    data.extend(aseq::vec48([2.0, 2.0, 0.5]));
    let section = aseq::section(true, &[(7, aseq::bone(0, &data)), (0, vec![])]);
    let mut seq = one_anim(3);
    seq.flags = 0x20000;
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[(DATA, section)], None, true)).await;
    let animation = value.as_animation().unwrap();
    assert!(animation.info(0).unwrap().has_scale);
    let t = animation.track(0, 0, &cx).await.unwrap();
    assert_eq!(
        constant(&t.translation),
        [1.5, -2.0, 0.25],
        "a constant replaces the rest pose"
    );
    near(
        &constant(&t.rotation),
        &[
            0.0,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        ],
    );
    assert_eq!(constant(&t.scale), [2.0, 2.0, 0.5]);
    let rest = animation.track(0, 1, &cx).await.unwrap();
    assert_eq!(constant(&rest.translation), [2.0, 2.0, 3.0]);
}

#[tokio::test]
async fn stall_and_external_sections_are_read_from_their_places() {
    let env = common::env();
    let section = |values: &[i16]| {
        aseq::section(
            true,
            &[(1, tracked_pos(1.0, aseq::raw_track(values))), (0, vec![])],
        )
    };
    let (stall, regular, trailing) = (
        section(&[0, 10, 20, 30]),
        section(&[40, 50, 60, 70]),
        section(&[99, 0, 0, 0]),
    );
    let external = section(&[80, 90, 0, 0]);
    let regular_at = stall.len() as i32;
    let trailing_at = regular_at + regular.len() as i32;
    let mut anim = aseq::Anim::new("long", 10, DATA);
    anim.stall = 4;
    anim.section_frames = 4;
    anim.sections = vec![regular_at, !0, trailing_at];
    let blob: Vec<u8> = [stall, regular, trailing].concat();
    let seq = aseq::Seq::new("long", vec![anim]);
    let (cx, value) = loaded(
        &env,
        pak_with(14, &seq, &[(DATA, blob)], Some(&external), true),
    )
    .await;
    let animation = value.as_animation().unwrap();
    let t = animation.track(0, 0, &cx).await.unwrap();
    let xs: Vec<f32> = dense(&t.translation).iter().map(|t| t[0]).collect();
    near(
        &xs,
        &[1.0, 11.0, 21.0, 31.0, 41.0, 51.0, 61.0, 71.0, 81.0, 91.0],
    );
    // a single frame in the middle of an external section
    let s = animation.sample(0, 0, 8, &cx).await.unwrap();
    assert_eq!(s.translation[0], 81.0);
    let s = animation.sample(0, 0, 9, &cx).await.unwrap();
    assert_eq!(
        s.translation[0], 91.0,
        "the trailing section is not used for the last frame"
    );
}

#[tokio::test]
async fn an_external_section_without_data_extra_is_a_missing_dependency() {
    let env = common::env();
    let section = aseq::section(
        true,
        &[(1, tracked_pos(1.0, aseq::raw_track(&[0; 4]))), (0, vec![])],
    );
    let mut anim = aseq::Anim::new("long", 6, DATA);
    anim.stall = 2;
    anim.section_frames = 2;
    anim.sections = vec![!0, !0, !0];
    let seq = aseq::Seq::new("long", vec![anim]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[(DATA, section)], None, true)).await;
    let e = value
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
async fn one_frame_and_flat_animations() {
    let env = common::env();
    let section = aseq::section(
        true,
        &[
            (1, aseq::bone(0, &aseq::vec48([4.0, 5.0, 6.0]))),
            (0, vec![]),
        ],
    );
    let (cx, value) = loaded(
        &env,
        pak_with(14, &one_anim(1), &[(DATA, section)], None, true),
    )
    .await;
    let t = value
        .as_animation()
        .unwrap()
        .track(0, 0, &cx)
        .await
        .unwrap();
    assert_eq!(constant(&t.translation), [4.0, 5.0, 6.0]);
    let s = value
        .as_animation()
        .unwrap()
        .sample(0, 0, 0, &cx)
        .await
        .unwrap();
    assert_eq!(s.translation, [4.0, 5.0, 6.0]);

    // sectionframes = 0 with a stall value: still one section
    let env = common::env();
    let section = aseq::section(
        true,
        &[
            (1, tracked_pos(1.0, aseq::raw_track(&[0, 1, 2, 3, 4, 5]))),
            (0, vec![]),
        ],
    );
    let mut anim = aseq::Anim::new("flat", 6, DATA);
    anim.stall = 3;
    let seq = aseq::Seq::new("flat", vec![anim]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[(DATA, section)], None, true)).await;
    let t = value
        .as_animation()
        .unwrap()
        .track(0, 0, &cx)
        .await
        .unwrap();
    let xs: Vec<f32> = dense(&t.translation).iter().map(|t| t[0]).collect();
    near(&xs, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

#[tokio::test]
async fn additive_animations_start_from_the_identity_and_carry_weights() {
    let env = common::env();
    let section = aseq::section(
        true,
        &[
            (1, tracked_pos(1.0, aseq::raw_track(&[10, 20]))),
            (0, vec![]),
        ],
    );
    let mut anim = aseq::Anim::new("delta", 2, DATA);
    anim.flags |= 0x4;
    let mut seq = aseq::Seq::new("delta", vec![anim]);
    seq.weights = aseq::Weights::List(vec![0.25, 0.75]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[(DATA, section)], None, true)).await;
    let animation = value.as_animation().unwrap();
    assert!(animation.info(0).unwrap().additive);
    assert_eq!(animation.bone_weights(), [0.25, 0.75]);
    let root = animation.track(0, 0, &cx).await.unwrap();
    assert_eq!(
        dense(&root.translation),
        [[10.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        "no rest pose is added"
    );
    assert_eq!(constant(&root.rotation), [0.0, 0.0, 0.0, 1.0]);
    let arm = animation.track(0, 1, &cx).await.unwrap();
    assert_eq!(constant(&arm.translation), [0.0; 3]);
    assert_eq!(constant(&arm.scale), [1.0; 3]);
}

#[tokio::test]
async fn special_weight_lists() {
    for (weights, expected) in [
        (aseq::Weights::Zero, vec![0.0, 0.0]),
        (aseq::Weights::One, vec![]),
    ] {
        let env = common::env();
        let mut seq = one_anim(1);
        seq.weights = weights;
        let (_, value) = loaded(&env, pak_with(14, &seq, &[], None, true)).await;
        assert_eq!(value.as_animation().unwrap().bone_weights(), expected);
    }
}

#[tokio::test]
async fn animations_without_data_hold_the_rest_pose() {
    let env = common::env();
    let mut anim = aseq::Anim::new("empty", 3, 0);
    anim.flags = 0;
    let seq = aseq::Seq::new("empty", vec![anim]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[], None, true)).await;
    let t = value
        .as_animation()
        .unwrap()
        .track(0, 1, &cx)
        .await
        .unwrap();
    assert_eq!(constant(&t.translation), [2.0, 2.0, 3.0]);
    assert_eq!(constant(&t.rotation), [0.0, 0.0, 0.0, 1.0]);

    // flagged as having data but no asqd: the same
    let mut anim = aseq::Anim::new("empty", 3, 0);
    anim.flags = 0x20000;
    let seq = aseq::Seq::new("empty", vec![anim]);
    let env = common::env();
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[], None, true)).await;
    let t = value
        .as_animation()
        .unwrap()
        .track(0, 0, &cx)
        .await
        .unwrap();
    assert_eq!(constant(&t.translation), [1.0, 2.0, 3.0]);
}

#[tokio::test]
async fn frame_movement_is_folded_into_bone_0() {
    let env = common::env();
    let movement = aseq::movement(
        [0.1, 0.0, 0.0, 1.0],
        0,
        &[[
            Some(aseq::raw_track(&[0, 10, 20, 30])),
            None,
            None,
            Some(aseq::raw_track(&[0, 45, 90, 135])),
        ]],
    );
    let mut anim = aseq::Anim::new("move", 4, 0);
    anim.flags = 0x20000 | 0x40000;
    anim.movement = Some(movement);
    let seq = aseq::Seq::new("move", vec![anim]);
    let (cx, value) = loaded(&env, pak_with(14, &seq, &[], None, true)).await;
    let animation = value.as_animation().unwrap();
    let root = animation.track(0, 0, &cx).await.unwrap();
    let xs: Vec<f32> = dense(&root.translation).iter().map(|t| t[0]).collect();
    near(&xs, &[1.0, 2.0, 3.0, 4.0]);
    let half = 90.0f32.to_radians() / 2.0;
    near(
        &dense(&root.rotation)[2],
        &[0.0, 0.0, half.sin(), half.cos()],
    );
    let s = animation.sample(0, 0, 3, &cx).await.unwrap();
    near(&s.translation, &[4.0, 2.0, 3.0]);
    // other bones are untouched
    assert!(matches!(
        animation.track(0, 1, &cx).await.unwrap().translation,
        Track::Constant(_)
    ));
}
