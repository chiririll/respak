//! Lookup, unsupported and broken paks.

use super::*;

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
