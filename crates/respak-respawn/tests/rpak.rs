//! The rpak container on synthetic paks (versions 6, 7, 8).

mod common;

use common::*;
use respak_core::{DiscoverOptions, ErrorKind};
use respak_respawn::{string_to_guid, PakHeader, Tables};

fn assets() -> Vec<TestAsset> {
    vec![
        TestAsset::new(0x1111_2222_3333_4444, b"txtr", 10, b"TEXTURE-HEADER-BYTES"),
        TestAsset::new(0xAAAA_BBBB_CCCC_DDDD, b"matl", 15, b"material header"),
        TestAsset::new(0x0000_0000_0000_00FF, b"ui\0\0", 2, b"ui!"),
    ]
}

fn depth(n: u32) -> DiscoverOptions {
    DiscoverOptions {
        depth: n,
        ..Default::default()
    }
}

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

#[tokio::test]
async fn a_name_resolves_through_its_hashed_guid() {
    let env = env();
    let guid = string_to_guid("texture/foo");
    env.input
        .write(
            "c.rpak",
            build_pak(8, &[TestAsset::new(guid, b"txtr", 8, b"named")]),
        )
        .await
        .unwrap();

    let d = env
        .engine
        .discover(
            "c.rpak/texture/foo.txtr",
            &depth(0),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(d.entries[0].format.as_deref(), Some("txtr"));
    // Case and slash direction do not matter to the hash.
    env.engine
        .discover(
            "c.rpak/Texture\\Foo.txtr",
            &depth(0),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
    env.engine
        .discover(
            "c.rpak/TEXTURE/FOO.TXTR",
            &depth(0),
            &respak_core::CancellationToken::new(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_wrong_kind_or_missing_asset_is_not_found() {
    let env = env();
    env.input
        .write("c.rpak", build_pak(8, &assets()))
        .await
        .unwrap();
    for path in [
        "c.rpak/0x1111222233334444.matl",
        "c.rpak/0x1.txtr",
        "c.rpak/nothing",
        "c.rpak/0x1111222233334444",
    ] {
        let err = env
            .engine
            .discover(path, &depth(0), &respak_core::CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::NotFound { .. }),
            "{path}: {err}"
        );
    }
}

#[tokio::test]
async fn duplicate_guids_are_listed_once() {
    let env = env();
    let twice = [
        TestAsset::new(7, b"txtr", 8, b"first"),
        TestAsset::new(7, b"txtr", 8, b"second"),
    ];
    env.input
        .write("c.rpak", build_pak(7, &twice))
        .await
        .unwrap();
    let d = env
        .engine
        .discover("c.rpak", &depth(1), &respak_core::CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(d.entries.len(), 1);
}

#[tokio::test]
async fn unsupported_paks_say_why() {
    let env = env();
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        (
            "v9.rpak",
            {
                let mut b = build_pak(8, &assets());
                b[4] = 9;
                b
            },
            "9",
        ),
        (
            "zstd.rpak",
            build_pak_with(
                7,
                &assets(),
                &Options {
                    flags: 1 << 15,
                    ..Default::default()
                },
            ),
            "zstd",
        ),
    ];
    for (name, bytes, why) in cases {
        env.input.write(name, bytes).await.unwrap();
        let err = env
            .engine
            .discover(name, &depth(1), &respak_core::CancellationToken::new())
            .await
            .unwrap_err();
        match err.kind() {
            ErrorKind::UnsupportedVersion { format, version } => {
                assert_eq!(format, "rpak", "{name}");
                assert!(version.contains(why), "{name}: {version}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[tokio::test]
async fn broken_paks_are_corrupt_not_a_panic() {
    let env = env();
    let good = build_pak(8, &assets());
    let mut huge = good.clone();
    huge[0x58..0x5C].copy_from_slice(&i32::MAX.to_le_bytes()); // numAssets
    let mut negative = good.clone();
    negative[0x58..0x5C].copy_from_slice(&(-5i32).to_le_bytes());
    let cases = [
        ("cut-header.rpak", good[..0x40].to_vec()),
        ("cut-body.rpak", good[..good.len() - 10].to_vec()),
        ("huge.rpak", huge),
        ("negative.rpak", negative),
    ];
    for (name, bytes) in cases {
        env.input.write(name, bytes).await.unwrap();
        let err = env
            .engine
            .discover(name, &depth(1), &respak_core::CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(err.kind(), ErrorKind::Corrupt { .. }),
            "{name}: {err}"
        );
    }
}

#[tokio::test]
async fn a_header_that_does_not_fit_its_page_is_corrupt_when_read() {
    let env = env();
    let bad = build_pak_with(
        8,
        &assets(),
        &Options {
            header_size_extra: 1000,
            ..Default::default()
        },
    );
    env.input.write("c.rpak", bad).await.unwrap();
    let cx = respak_core::testing::cx(
        {
            let mut r = respak_core::Registry::new();
            r.add(&respak_respawn::RespawnPlugin::new());
            r
        },
        env.input.clone(),
        env.input.clone(),
    );
    let pak = cx.mediator.open("c.rpak", &cx).await.unwrap();
    let pak = cx.mediator.mount(pak, &cx).await.unwrap();
    let err = pak
        .as_container()
        .unwrap()
        .volume()
        .read("0x1111222233334444.txtr")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("does not fit"), "{err}");
}

#[test]
fn tables_expose_starpaks_pages_and_asset_fields() {
    let opts = Options {
        starpaks: vec!["paks\\Win64\\a.starpak".into(), "b.starpak".into()],
        ..Default::default()
    };
    let file = build_pak_with(8, &assets(), &opts);
    let header = PakHeader::parse(&file).unwrap();
    assert_eq!(
        (header.version, header.header_len, header.num_assets),
        (8, 0x80, 3)
    );
    let tables = Tables::parse(&header, &file[header.header_len..]).unwrap();
    assert_eq!(tables.starpaks, ["paks\\Win64\\a.starpak", "b.starpak"]);
    assert_eq!(tables.pages.len(), 1);
    let a = &tables.assets[1];
    assert_eq!(
        (a.guid, a.version, a.header_size, a.kind_string().as_str()),
        (0xAAAA_BBBB_CCCC_DDDD, 15, 15, "matl")
    );
    assert_eq!(a.starpak, -1);
    assert!(a.data.is_null());
    assert_eq!(tables.header_range(a).map(|r| r.end - r.start), Some(15));
}

#[test]
fn not_an_rpak_is_unknown_format() {
    let err = PakHeader::parse(b"nope, not a pak at all").unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::UnknownFormat));
}

/// Local-only checks against real paks from rsx's cache. Skipped when the files are missing.
mod real {
    use super::*;
    use std::path::Path;

    const UI_MAIN_MENU: &str = r"C:\Users\chiririll\Projects\rsx\rsx_steam_cache\rpak\1172471_6021239266545804912\paks\win64\ui_main_menu.rpak";

    #[tokio::test]
    async fn ui_main_menu_has_one_uiia_asset() {
        if !Path::new(UI_MAIN_MENU).exists() {
            return;
        }
        let env = env();
        env.input
            .write("ui_main_menu.rpak", std::fs::read(UI_MAIN_MENU).unwrap())
            .await
            .unwrap();
        let d = env
            .engine
            .discover(
                "ui_main_menu.rpak",
                &depth(1),
                &respak_core::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(d.entries.len(), 1);
        let e = &d.entries[0];
        assert_eq!(e.path, "ui_main_menu.rpak/0x35492C0539B480BC.uiia");
        assert_eq!(
            (e.version.as_deref(), e.kind.as_deref()),
            (Some("2"), Some("uiia"))
        );
    }

    const UI: &str = r"C:\Users\chiririll\Projects\rsx\rsx_steam_cache\rpak\1172471_6197768190074397522\paks\win64\ui.rpak";

    /// The name in a texture header hashes to the asset's GUID (checks the GUID hash port).
    #[tokio::test]
    #[ignore]
    async fn texture_names_hash_to_their_guids() {
        if !Path::new(UI).exists() {
            return;
        }
        let env = env();
        env.input
            .write("ui.rpak", std::fs::read(UI).unwrap())
            .await
            .unwrap();
        let mut r = respak_core::Registry::new();
        r.add(&respak_respawn::RespawnPlugin::new());
        let cx = respak_core::testing::cx(
            r,
            env.input.clone(),
            env.input.clone(),
        );
        let d = env
            .engine
            .discover("ui.rpak", &depth(1), &respak_core::CancellationToken::new())
            .await
            .unwrap();
        let (mut named, mut matched) = (0, 0);
        for e in d
            .entries
            .iter()
            .filter(|e| e.kind.as_deref() == Some("txtr"))
        {
            let asset = cx.mediator.open(&e.path, &cx).await.unwrap();
            let tex = cx.mediator.identify(asset, &cx).await.unwrap();
            let meta = tex.as_metadata().unwrap();
            let (Some(name), Some(guid)) = (meta.name(), meta.guid()) else {
                continue;
            };
            named += 1;
            let candidates = [
                name.to_owned(),
                format!("{name}.rpak"),
                name.trim_start_matches("texture/").to_owned(),
            ];
            let hit = candidates.iter().find(|c| string_to_guid(c) == guid);
            eprintln!("{name} {guid:#x} -> {hit:?}");
            matched += usize::from(hit.is_some());
        }
        // The UI textures of the cached paks have no names (null name pointers), so this only
        // checks something on paks that do.
        eprintln!("{named} named textures, {matched} matched");
        assert_eq!(named, matched);
    }

    /// Apex `common.rpak` with `common(01).rpak` and `patch_master.rpak`, downloaded by
    /// `respak --steam 1172470 --depot 1172471 copy` into `out/patches` (never committed).
    const PATCHED_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..\out\patches");

    /// Reads all of the patched `.mem` and every asset header. Slow: `-- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn patched_common_rpak_reads_all_its_pages() {
        if !Path::new(PATCHED_DIR)
            .join("paks/Win64/common(01).rpak")
            .exists()
        {
            return;
        }
        let input =
            opendal::Operator::new(opendal::services::Fs::default().root(PATCHED_DIR)).unwrap();
        let mut r = respak_core::Registry::new();
        r.add(&respak_respawn::RespawnPlugin::new());
        let cx = respak_core::testing::cx(r, input.clone(), input.clone());
        let pak = cx
            .mediator
            .open("paks/Win64/common.rpak", &cx)
            .await
            .unwrap();
        let pak = cx.mediator.mount(pak, &cx).await.unwrap();
        let container = pak.as_container().unwrap();
        let volume = container.volume();
        let mem_len = volume.stat(".mem").await.unwrap().content_length();
        let started = std::time::Instant::now();
        let chunk = 64 << 20;
        let mut at = 0;
        let mut sum = 0u64;
        while at < mem_len {
            let end = (at + chunk).min(mem_len);
            let bytes = volume
                .read_with(".mem")
                .range(at..end)
                .await
                .unwrap()
                .to_bytes();
            sum = bytes.iter().fold(sum, |s, &b| s.wrapping_add(u64::from(b)));
            at = end;
        }
        eprintln!(
            ".mem: {mem_len} bytes, byte sum {sum}, {:?}",
            started.elapsed()
        );

        let entries = volume.list("").await.unwrap();
        let mut failed = 0;
        for e in &entries {
            if let Err(err) = volume.read(e.name()).await {
                failed += 1;
                if failed < 10 {
                    eprintln!("{}: {err}", e.name());
                }
            }
        }
        eprintln!(
            "{} headers, {failed} failed, {:?}",
            entries.len(),
            started.elapsed()
        );
        assert_eq!(failed, 0);
    }

    /// Slow (decompresses 268 MB): `cargo test -p respak-respawn -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn ui_rpak_is_decompressed_and_listed() {
        if !Path::new(UI).exists() {
            return;
        }
        let env = env();
        env.input
            .write("ui.rpak", std::fs::read(UI).unwrap())
            .await
            .unwrap();
        let d = env
            .engine
            .discover("ui.rpak", &depth(1), &respak_core::CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(d.entries.len(), 1665);
        // Textures are recognised as `txtr`; other kinds have no reader yet.
        let textures = d
            .entries
            .iter()
            .filter(|e| e.format.as_deref() == Some("txtr"))
            .count();
        assert_eq!(textures, 11);
        assert!(d
            .entries
            .iter()
            .all(|e| matches!(e.format.as_deref(), Some("rpak.asset" | "txtr"))));
        let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
        for e in &d.entries {
            *kinds.entry(e.kind.as_deref().unwrap()).or_default() += 1;
        }
        eprintln!("{kinds:?}");
    }
}

#[test]
fn every_truncation_of_a_pak_is_an_error_or_ok_never_a_panic() {
    for version in [6u16, 7, 8] {
        let pak = build_pak(version, &assets());
        for n in 0..pak.len() {
            let Ok(header) = PakHeader::parse(&pak[..n]) else {
                continue;
            };
            let body = &pak[header.header_len.min(n)..n];
            let _ = Tables::parse(&header, body);
        }
    }
}
