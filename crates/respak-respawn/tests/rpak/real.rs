//! Local-only checks against real paks from rsx's cache. Skipped when the files are missing.

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
    let cx = respak_core::testing::cx(r, env.input.clone(), env.input.clone());
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
    let input = opendal::Operator::new(opendal::services::Fs::default().root(PATCHED_DIR)).unwrap();
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
