//! `dump`: the raw bytes of a pak asset, whatever its kind.

use std::process::Command;

use respak_respawn_testing::{build_pak, TestAsset};

#[test]
fn dumps_the_header_of_an_asset_of_an_unknown_kind() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out) = (dir.path().join("in"), dir.path().join("out"));
    let header: Vec<u8> = (0..32).collect();
    std::fs::create_dir_all(root.join("paks")).unwrap();
    std::fs::write(
        root.join("paks/a.rpak"),
        build_pak(8, &[TestAsset::new(0xABCD, b"zzzz", 1, &header)]),
    )
    .unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_respak"))
        .args(["--root".as_ref(), root.as_os_str()])
        .args(["--out".as_ref(), out.as_os_str()])
        .args(["dump", "paks/a.rpak/*"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let dumped = std::fs::read(out.join("paks/a.rpak/0x000000000000ABCD.zzzz")).unwrap();
    assert_eq!(dumped, header);
}
