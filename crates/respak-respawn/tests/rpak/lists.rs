//! Detecting and listing paks.

use super::*;

#[tokio::test]
async fn every_version_is_detected_and_listed() {
    for version in [6u16, 7, 8] {
        let env = env();
        env.input
            .write("common.rpak", build_pak(version, &assets()))
            .await
            .unwrap();

        let d0 = env
            .engine
            .discover(
                "common.rpak",
                &depth(0),
                &respak_core::CancellationToken::new(),
            )
            .await
            .unwrap();
        let e = &d0.entries[0];
        assert_eq!(
            (e.format.as_deref(), e.score),
            (Some("rpak"), 13),
            "v{version}"
        );
        assert_eq!(e.version.as_deref(), Some(version.to_string().as_str()));

        let d1 = env
            .engine
            .discover(
                "common.rpak",
                &depth(1),
                &respak_core::CancellationToken::new(),
            )
            .await
            .unwrap();
        let listed: Vec<_> = d1
            .entries
            .iter()
            .map(|e| {
                (
                    e.path.as_str(),
                    e.format.as_deref(),
                    e.version.as_deref(),
                    e.kind.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (
                    "common.rpak/0x00000000000000FF.ui",
                    Some("rpak.asset"),
                    Some("2"),
                    Some("ui")
                ),
                (
                    "common.rpak/0x1111222233334444.txtr",
                    Some("txtr"),
                    Some("10"),
                    Some("txtr")
                ),
                (
                    "common.rpak/0xAAAABBBBCCCCDDDD.matl",
                    Some("matl"),
                    Some("15"),
                    Some("matl")
                ),
            ],
            "v{version}"
        );
    }
}

#[tokio::test]
async fn an_asset_path_resolves_and_its_header_is_served_with_ranged_reads() {
    let env = env();
    env.input
        .write("p/common.rpak", build_pak(8, &assets()))
        .await
        .unwrap();

    let d = env
        .engine
        .discover(
            "p/common.rpak/0x1111222233334444.txtr",
            &depth(0),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(d.entries[0].format.as_deref(), Some("txtr"));
    assert_eq!(d.entries[0].version.as_deref(), Some("10"));

    // The volume itself: header bytes of the asset, whole and in a range.
    let cx = respak_core::testing::cx(
        {
            let mut r = respak_core::Registry::new();
            r.add(&respak_respawn::RespawnPlugin::new());
            r
        },
        env.input.clone(),
        env.input.clone(),
    );
    let pak = cx.mediator.open("p/common.rpak", &cx).await.unwrap();
    assert!(
        pak.as_container().is_none(),
        "a plain file until it is mounted"
    );
    let pak = cx.mediator.mount(pak, &cx).await.unwrap();
    let volume = pak.as_container().unwrap().volume();
    assert_eq!(
        volume
            .read("0x1111222233334444.txtr")
            .await
            .unwrap()
            .to_vec(),
        b"TEXTURE-HEADER-BYTES"
    );
    assert_eq!(
        volume
            .read_with("0x1111222233334444.txtr")
            .range(8..14)
            .await
            .unwrap()
            .to_vec(),
        b"HEADER"
    );
    assert_eq!(
        volume
            .stat("0xAAAABBBBCCCCDDDD.matl")
            .await
            .unwrap()
            .content_length(),
        15
    );
}

#[tokio::test]
async fn oodle_paks_list_their_assets_and_serve_them() {
    // Body-only compressed size, several blocks, a restart in the middle.
    for (version, restart_blocks) in [(7u16, 0usize), (8, 0), (8, 1), (7, 2)] {
        let env = env();
        let pak = oodle_pak(&build_pak(version, &assets()), restart_blocks, 700_000);
        env.input.write("p/common.rpak", pak).await.unwrap();

        let d = env
            .engine
            .discover(
                "p/common.rpak",
                &depth(1),
                &respak_core::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(d.entries.len(), 3, "v{version}/{restart_blocks}");
        let cx = respak_core::testing::cx(
            {
                let mut r = respak_core::Registry::new();
                r.add(&respak_respawn::RespawnPlugin::new());
                r
            },
            env.input.clone(),
            env.input.clone(),
        );
        let pak = cx.mediator.open("p/common.rpak", &cx).await.unwrap();
        let pak = cx.mediator.mount(pak, &cx).await.unwrap();
        let volume = pak.as_container().unwrap().volume();
        assert_eq!(
            volume
                .read("0x1111222233334444.txtr")
                .await
                .unwrap()
                .to_vec(),
            b"TEXTURE-HEADER-BYTES"
        );
        assert_eq!(
            volume
                .read("0xAAAABBBBCCCCDDDD.matl")
                .await
                .unwrap()
                .to_vec(),
            b"material header"
        );
    }
}

#[tokio::test]
async fn oodle_paks_with_a_short_file_or_bad_stream_are_corrupt() {
    let env = env();
    let pak = oodle_pak(&build_pak(8, &assets()), 0, 700_000);
    env.input
        .write("short.rpak", pak[..pak.len() - 10].to_vec())
        .await
        .unwrap();
    let err = env
        .engine
        .discover(
            "short.rpak",
            &depth(1),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");

    let mut bad = pak.clone();
    bad[0x80] = 0xFF; // the first block header
    env.input.write("bad.rpak", bad).await.unwrap();
    let err = env
        .engine
        .discover(
            "bad.rpak",
            &depth(1),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("oodle"), "{err}");
}
