//! Live check against Steam with the user's own stored login. Ignored by default, never runs in CI, and is not
//! to be run without the user's explicit permission:
//! `RESPAK_STEAM_LIVE=1 cargo test -p respak-steam --test live -- --ignored --test-threads=1`
//! (Steam allows one session per account: parallel logons kick each other off) after `respak steam login` in the repo root
//! (`RESPAK_STATE_DIR` overrides the `.respak` directory).

#[tokio::test]
#[ignore = "live Steam test: run only with the user's permission"]
async fn stored_token_logs_on() {
    if std::env::var_os("RESPAK_STEAM_LIVE").is_none() {
        return;
    }
    let store = respak_steam::AccountStore::new(std::env::var("RESPAK_STATE_DIR").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../.respak").into()));
    let (_client, account) = respak_steam::logon(&store).await.expect("logon with the stored token");
    assert_ne!(account.steam_id, 0);
}

/// Opens Apex's main depot (current manifest, or `RESPAK_STEAM_MANIFEST`), stats `paks/Win64/common.rpak`
/// and reads its first bytes: only the first chunk may be downloaded.
#[tokio::test]
#[ignore = "live Steam test: run only with the user's permission"]
async fn depot_stat_and_ranged_read() {
    if std::env::var_os("RESPAK_STEAM_LIVE").is_none() {
        return;
    }
    let state = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.respak"));
    let manifest = std::env::var("RESPAK_STEAM_MANIFEST").ok().and_then(|m| m.parse().ok());
    let depots = vec![respak_steam::DepotSpec { depot: 1172471, manifest }];
    let input = respak_steam::open(respak_steam::SteamOptions::new(1172470, depots, state)).await.expect("open the depot");
    let stat = input.operator.stat("Paks/Win64/common.rpak").await.expect("stat");
    assert!(stat.content_length() > 1 << 20);
    let head = input.operator.read_with("paks/win64/COMMON.RPAK").range(0..4).await.expect("read").to_vec();
    assert_eq!(&head, b"RPak");
    let stats = input.stats();
    assert!(stats.chunks_downloaded + stats.cache_hits <= 1, "{stats:?}");
}
