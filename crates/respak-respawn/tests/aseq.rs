//! `aseq`, `asqd` and `arig` on synthetic paks (slice 19): versions, sections, tracks, rig lookup
//! and errors.

mod common;

use common::*;
use respak_core::capability::Track;
use respak_core::{Cx, ErrorKind, Registry, Value, Version};
use respak_respawn::{
    aseq_variant, studio_variant, AnimDesc, ArigHeader, AseqAsset, AseqHeader, AsqdAsset,
    AsqdHeader, RespawnPlugin, RigSelect, SectionData, SeqDesc, StudioLayout, StudioSelect,
    Weights,
};

const SEQ: u64 = 0x10;
const DATA: u64 = 0x20;
const RIG: u64 = 0x50;
/// The pak creation time from which rsx calls an aseq of asset version 12 "12.1".
const CREATED_12_1: u64 = 0x01DC_1DF8_05C2_8000;
const ROT: f32 = 0.000_191_753_45;

fn studio() -> Vec<u8> {
    rmdl::studio(18, false, 0)
}

/// A pak `p.rpak` with a sequence (asset `version`) and the `asqd` blobs, plus a rig listing it.
fn pak_with(
    version: u32,
    seq: &aseq::Seq,
    blobs: &[(u64, Vec<u8>)],
    extra: Option<&[u8]>,
    rig: bool,
) -> Vec<u8> {
    let mut p = PakBuilder::new();
    if rig {
        p.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    }
    for (guid, blob) in blobs {
        p.asqd(*guid, blob);
    }
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        version,
        &aseq::seq_block(seq),
        &[0x77],
        extra,
    );
    let mut bytes = p.build(8);
    if version == 12 {
        bytes[8..16].copy_from_slice(&CREATED_12_1.to_le_bytes());
    }
    bytes
}

fn context(env: &Env, plugin: &RespawnPlugin) -> Cx {
    let mut r = Registry::new();
    r.add(plugin);
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

/// Reads asset `guid` of `kind` in `pak` through the mediator.
async fn read_with(
    env: &Env,
    plugin: &RespawnPlugin,
    pak: &str,
    guid: u64,
    kind: &str,
) -> (Cx, respak_core::Result<Value>) {
    let cx = context(env, plugin);
    let value = match cx
        .mediator
        .open(&format!("{pak}/0x{guid:016X}.{kind}"), &cx)
        .await
    {
        Ok(asset) => cx.mediator.identify(asset, &cx).await,
        Err(e) => Err(e),
    };
    (cx, value)
}

async fn load(env: &Env, bytes: Vec<u8>) -> (Cx, respak_core::Result<Value>) {
    env.input.write("p.rpak", bytes).await.unwrap();
    read_with(env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await
}

async fn loaded(env: &Env, bytes: Vec<u8>) -> (Cx, Value) {
    let (cx, value) = load(env, bytes).await;
    (cx, value.unwrap())
}

fn near(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len(), "{a:?} != {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-4, "{a:?} != {b:?}");
    }
}

fn dense<T: Clone>(t: &Track<T>) -> Vec<T> {
    match t {
        Track::Dense(v) => v.clone(),
        Track::Constant(_) => panic!("expected a dense track"),
    }
}

fn constant<T: Clone>(t: &Track<T>) -> T {
    match t {
        Track::Constant(v) => v.clone(),
        Track::Dense(_) => panic!("expected a constant track"),
    }
}

/// A position track of one axis (X) with `scale`: the bone block with a tracked position.
fn tracked_pos(scale: f32, track: Vec<u8>) -> Vec<u8> {
    let mut data = scale.to_le_bytes().to_vec();
    data.extend(aseq::value_ptr(4, 0b100, 0, 0));
    data.extend(track);
    aseq::bone(0x4, &data)
}

/// Bone 0 with a tracked position (X) and rotation (pitch), bone 1 with a constant rotation.
fn two_bone_section(six: bool) -> Vec<u8> {
    let mut data = 0.5f32.to_le_bytes().to_vec();
    data.extend(aseq::value_ptr(8, 0b100, 0, 0));
    data.extend(aseq::value_ptr(14, 0b010, 0, 0));
    data.extend(aseq::raw_track(&[10, 20, 30, 40]));
    data.extend(aseq::raw_track(&[1000, 2000, 3000, 4000]));
    aseq::section(
        six,
        &[
            (3, aseq::bone(0x6, &data)),
            (2, aseq::bone(0, &aseq::quat64([0.5; 4]))),
        ],
    )
}

fn one_anim(frames: i32) -> aseq::Seq {
    aseq::Seq::new("walk", vec![aseq::Anim::new("walk_anim", frames, DATA)])
}

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

// ---- asqd -----------------------------------------------------------------------------------------

#[tokio::test]
async fn an_asqd_reads_ranges_of_its_blob() {
    let env = common::env();
    let mut p = PakBuilder::new();
    p.asqd(DATA, &[1, 2, 3, 4, 5, 6, 7, 8]);
    p.asqd_without_data(0x21);
    env.input.write("p.rpak", p.build(8)).await.unwrap();

    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", DATA, "asqd").await;
    let value = value.unwrap();
    let m = value.as_metadata().unwrap();
    assert_eq!(
        (m.kind(), m.name(), m.guid()),
        (Some("asqd"), None, Some(DATA))
    );
    assert!(m.dependencies().is_empty());
    let asqd = value.downcast_ref::<AsqdAsset>().unwrap();
    assert_eq!(&asqd.read(2, 3).await.unwrap()[..], [3, 4, 5]);
    assert_eq!(
        &asqd.read(0, 8).await.unwrap()[..],
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    for (offset, len) in [(0, 1 << 40), (u64::MAX - 1, 4), (1 << 40, 1)] {
        let e = asqd.read(offset, len).await.unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::Corrupt { .. }),
            "{offset} {len}: {e}"
        );
    }

    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", 0x21, "asqd").await;
    let e = value
        .unwrap()
        .downcast_ref::<AsqdAsset>()
        .unwrap()
        .read(0, 1)
        .await
        .unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::MissingDependency { .. }),
        "{e}"
    );
}

#[test]
fn the_asqd_header_is_8_bytes() {
    assert_eq!(
        AsqdHeader::parse(&0x40u64.to_le_bytes())
            .unwrap()
            .data_offset,
        Some(0x40)
    );
    assert_eq!(AsqdHeader::parse(&[0; 8]).unwrap().data_offset, None);
    assert!(AsqdHeader::parse(&[0; 7]).is_err());
    assert!(AsqdHeader::parse(&[0; 16]).is_err());
}

#[tokio::test]
async fn other_asqd_versions_are_unsupported() {
    let env = common::env();
    let mut p = PakBuilder::new();
    p.asset(DATA, b"asqd", 2, vec![0; 8], &[], None);
    env.input.write("p.rpak", p.build(8)).await.unwrap();
    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", DATA, "asqd").await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
}

// ---- arig -----------------------------------------------------------------------------------------

#[test]
fn the_arig_header_counts_are_4_or_2_bytes_wide() {
    let mut v4 = vec![0u8; 0x28];
    v4[0..8].copy_from_slice(&0x100u64.to_le_bytes());
    v4[0x14..0x18].copy_from_slice(&70_000i32.to_le_bytes());
    v4[0x18..0x20].copy_from_slice(&0x200u64.to_le_bytes());
    let h = ArigHeader::parse(4, &v4).unwrap();
    assert_eq!(
        (h.studio_offset, h.anim_seqs),
        (0x100, (Some(0x200), 70_000))
    );

    let mut v5 = vec![0u8; 0x28];
    v5[0..8].copy_from_slice(&0x100u64.to_le_bytes());
    v5[0x12..0x14].copy_from_slice(&2i16.to_le_bytes());
    v5[0x14..0x18].copy_from_slice(&99i32.to_le_bytes());
    for version in [5, 6, 7] {
        let h = ArigHeader::parse(version, &v5).unwrap();
        assert_eq!(
            (h.anim_seqs.1, h.anim_seqs.0, h.name_offset),
            (2, None, None),
            "v{version}"
        );
    }
    for version in [0, 3, 8] {
        let e = ArigHeader::parse(version, &v5).unwrap_err();
        assert!(
            matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
            "v{version}"
        );
    }
    assert!(ArigHeader::parse(5, &v5[..0x20]).is_err());
    v5[0..8].fill(0);
    assert!(
        matches!(
            ArigHeader::parse(5, &v5).unwrap_err().kind(),
            ErrorKind::Corrupt { .. }
        ),
        "no studio"
    );
}

async fn rig(
    env: &Env,
    plugin: &RespawnPlugin,
    version: u32,
    studio: &[u8],
    seqs: &[u64],
) -> (Cx, respak_core::Result<Value>) {
    let mut p = PakBuilder::new();
    p.arig(RIG, "anim/rig", version, studio, seqs);
    env.input.write("r.rpak", p.build(8)).await.unwrap();
    read_with(env, plugin, "r.rpak", RIG, "arig").await
}

#[tokio::test]
async fn an_arig_gives_the_skeleton_the_model_path_gives() {
    for version in [4, 5, 6, 7] {
        let env = common::env();
        let (_, value) = rig(
            &env,
            &RespawnPlugin::new(),
            version,
            &studio(),
            &[0x11, 0x12],
        )
        .await;
        let value = value.unwrap();
        let m = value.as_metadata().unwrap();
        assert_eq!(
            (m.kind(), m.name(), m.guid()),
            (Some("arig"), Some("anim/rig"), Some(RIG))
        );
        assert_eq!(
            m.dependencies().iter().map(|d| d.guid).collect::<Vec<_>>(),
            [0x11, 0x12]
        );
        let bones = value.as_skeleton().unwrap().bones().to_vec();
        assert_eq!(bones.len(), 2);

        // the same studio block read as a model gives the same bones
        let mut p = PakBuilder::new();
        p.rmdl_without_vertex_data(0x100, "mdl/x.rmdl", 18, &studio());
        env.input.write("m.rpak", p.build(8)).await.unwrap();
        let (_, model) = read_with(&env, &RespawnPlugin::new(), "m.rpak", 0x100, "mdl_").await;
        assert_eq!(
            model.unwrap().as_skeleton().unwrap().bones(),
            &bones[..],
            "v{version}"
        );
    }
}

#[tokio::test]
async fn an_arig_with_no_sequences_or_a_null_list() {
    let env = common::env();
    let (_, value) = rig(&env, &RespawnPlugin::new(), 5, &studio(), &[]).await;
    assert!(value
        .unwrap()
        .as_metadata()
        .unwrap()
        .dependencies()
        .is_empty());
}

#[tokio::test]
async fn an_arig_with_local_sequences_or_a_short_studio_is_corrupt() {
    let env = common::env();
    let mut s = studio();
    s[0x7A] = 1;
    let (_, value) = rig(&env, &RespawnPlugin::new(), 5, &s, &[]).await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));

    let env = common::env();
    let (_, value) = rig(&env, &RespawnPlugin::new(), 3, &studio(), &[]).await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));

    // a studio block that does not reach its own bone table
    let env = common::env();
    let (_, value) = rig(&env, &RespawnPlugin::new(), 5, &studio()[..0x80], &[]).await;
    assert!(value.is_err());
}

#[test]
fn the_studio_layout_comes_from_the_option_a_sibling_or_the_block() {
    let split = rmdl::studio(18, false, 0);
    let linear = rmdl::studio(19, false, 0);
    let mut wide = rmdl::studio(20, true, 0);
    wide[0xD8..0xDA].copy_from_slice(&244u16.to_le_bytes());
    let auto = StudioSelect::Auto;

    // explicit layout names
    assert_eq!(
        studio_variant(&StudioSelect::Layout("v16".into()), None, &split).unwrap(),
        (16, StudioLayout::V16)
    );
    assert_eq!(
        studio_variant(&StudioSelect::Layout("v17".into()), None, &split).unwrap(),
        (18, StudioLayout::V17)
    );
    assert_eq!(
        studio_variant(&StudioSelect::Layout("v19_2".into()), None, &wide).unwrap(),
        (20, StudioLayout::V19_2)
    );
    let e = studio_variant(&StudioSelect::Layout("nope".into()), None, &split).unwrap_err();
    assert!(matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }));

    // a sibling model wins over the block
    let sibling = Version::with_minor(19, 3);
    assert_eq!(
        studio_variant(&auto, Some(sibling), &split).unwrap(),
        (19, StudioLayout::V19_2)
    );
    assert!(studio_variant(&auto, Some(Version::new(12)), &split).is_err());

    // the heuristic
    assert_eq!(
        studio_variant(&auto, None, &wide).unwrap(),
        (20, StudioLayout::V19_2)
    );
    assert_eq!(
        studio_variant(&auto, None, &linear).unwrap(),
        (19, StudioLayout::V17)
    );
    assert_eq!(
        studio_variant(&auto, None, &split).unwrap(),
        (18, StudioLayout::V17)
    );
    let e = studio_variant(&auto, None, &[0; 0x400]).unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{e}"
    );
    let e = studio_variant(&auto, None, &[0; 16]).unwrap_err();
    assert!(
        matches!(e.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{e}"
    );
}

#[tokio::test]
async fn an_arig_reads_with_the_heuristic_a_sibling_model_or_an_explicit_layout() {
    // 19.2/20 block without the 244 marker: the heuristic cannot tell, a sibling model can
    let wide = rmdl::studio(20, true, 0);
    let env = common::env();
    let mut p = PakBuilder::new();
    p.arig(RIG, "anim/rig", 5, &wide, &[]);
    p.rmdl_without_vertex_data(0x100, "mdl/x.rmdl", 20, &wide);
    env.input.write("r.rpak", p.build(8)).await.unwrap();
    let (_, value) = read_with(&env, &RespawnPlugin::new(), "r.rpak", RIG, "arig").await;
    let names: Vec<_> = value
        .unwrap()
        .as_skeleton()
        .unwrap()
        .bones()
        .iter()
        .map(|b| b.name.clone())
        .collect();
    assert_eq!(names, ["root", "arm"]);

    // alone: the option names the layout
    let env = common::env();
    let plugin = RespawnPlugin::new().studio("v19_2".to_owned());
    let (_, value) = rig(&env, &plugin, 5, &wide, &[]).await;
    assert_eq!(value.unwrap().as_skeleton().unwrap().bones().len(), 2);

    // alone and unnamed: no variant
    let env = common::env();
    let (_, value) = rig(&env, &RespawnPlugin::new(), 5, &[0u8; 0x400], &[]).await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
}

// ---- which rig a sequence gets ----------------------------------------------------------------------

fn renamed_studio(first: &str) -> Vec<u8> {
    let mut s = studio();
    s[0x3A0..0x3A0 + first.len() + 1].copy_from_slice(&[first.as_bytes(), &[0]].concat());
    s
}

#[tokio::test]
async fn the_first_arig_that_lists_the_sequence_wins_over_a_model() {
    let env = common::env();
    let mut p = PakBuilder::new();
    p.rmdl_with_sequences(0x100, "mdl/x.rmdl", 18, &renamed_studio("frommdl"), &[SEQ]);
    p.arig(RIG, "anim/rig", 5, &renamed_studio("fromrig"), &[SEQ]);
    p.arig(RIG + 1, "anim/rig2", 5, &renamed_studio("fromrg2"), &[SEQ]);
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(1)),
        &[],
        None,
    );
    env.input.write("p.rpak", p.build(8)).await.unwrap();
    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await;
    let value = value.unwrap();
    assert_eq!(value.as_animation().unwrap().bones()[0].name, "fromrig");
    assert_eq!(value.downcast_ref::<AseqAsset>().unwrap().rig(), RIG);
}

#[tokio::test]
async fn a_model_that_lists_the_sequence_is_a_rig_too() {
    let env = common::env();
    let mut p = PakBuilder::new();
    p.rmdl_with_sequences(
        0x100,
        "mdl/x.rmdl",
        18,
        &renamed_studio("frommdl"),
        &[0x999, SEQ],
    );
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(1)),
        &[],
        None,
    );
    env.input.write("p.rpak", p.build(8)).await.unwrap();
    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await;
    let value = value.unwrap();
    assert_eq!(value.as_animation().unwrap().bones()[0].name, "frommdl");
    assert_eq!(value.downcast_ref::<AseqAsset>().unwrap().rig(), 0x100);
}

#[tokio::test]
async fn the_rig_may_live_in_the_common_pak() {
    let env = common::env();
    let mut common = PakBuilder::new();
    common.arig(RIG, "anim/rig", 5, &renamed_studio("common"), &[SEQ]);
    env.input
        .write("paks/common.rpak", common.build(8))
        .await
        .unwrap();
    let mut p = PakBuilder::new();
    p.asqd(DATA, &two_bone_section(true));
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(4)),
        &[],
        None,
    );
    env.input.write("paks/p.rpak", p.build(8)).await.unwrap();
    let (cx, value) = read_with(&env, &RespawnPlugin::new(), "paks/p.rpak", SEQ, "aseq").await;
    let value = value.unwrap();
    let animation = value.as_animation().unwrap();
    assert_eq!(animation.bones()[0].name, "common");
    assert!(animation.track(0, 0, &cx).await.is_ok());
}

#[tokio::test]
async fn an_explicit_rig_is_found_by_guid_or_by_path() {
    let path = "anim/rig_by_path";
    let guid = respak_respawn::string_to_guid(path);
    let env = common::env();
    let mut p = PakBuilder::new();
    // lists nothing: only the option can bring the sequence to this rig
    p.arig(guid, path, 5, &renamed_studio("explct"), &[]);
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        14,
        &aseq::seq_block(&one_anim(1)),
        &[],
        None,
    );
    env.input.write("p.rpak", p.build(8)).await.unwrap();

    for select in [format!("0x{guid:016X}"), path.to_owned()] {
        let plugin = RespawnPlugin::new().rig(RigSelect::from(select.clone()));
        let (_, value) = read_with(&env, &plugin, "p.rpak", SEQ, "aseq").await;
        assert_eq!(
            value.unwrap().as_animation().unwrap().bones()[0].name,
            "explct",
            "{select}"
        );
    }
    let plugin = RespawnPlugin::new().rig(RigSelect::from("0x1234".to_owned()));
    let (_, value) = read_with(&env, &plugin, "p.rpak", SEQ, "aseq").await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::MissingDependency { .. }
    ));
    // auto finds nothing here
    let (_, value) = read_with(&env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await;
    assert!(matches!(
        value.unwrap_err().kind(),
        ErrorKind::MissingDependency { .. }
    ));
}

#[test]
fn rig_and_studio_options_are_plain_strings() {
    assert_eq!(RigSelect::from("auto".to_owned()), RigSelect::Auto);
    assert_eq!(String::from(RigSelect::Explicit("a/b".into())), "a/b");
    assert_eq!(String::from(RigSelect::Auto), "auto");
    assert_eq!(serde_json::to_string(&RigSelect::Auto).unwrap(), "\"auto\"");
    assert_eq!(
        serde_json::from_str::<StudioSelect>("\"v17\"").unwrap(),
        StudioSelect::Layout("v17".into())
    );
    assert_eq!(
        serde_json::from_str::<StudioSelect>("\"auto\"").unwrap(),
        StudioSelect::Auto
    );
}
