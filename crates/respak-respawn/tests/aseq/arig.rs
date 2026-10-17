//! `asqd`, `arig` and rig selection.

use super::*;

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
