//! `names`: strings come from decoded assets (a `dtbl` cell) and from files of no known format,
//! never from packed data.

use std::path::Path;
use std::process::Command;

use respak_respawn::string_to_guid;
use respak_respawn::testing::{DtblCell, PakBuilder};

fn write(root: &Path, path: &str, bytes: Vec<u8>) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, bytes).unwrap();
}

#[test]
fn names_come_from_table_cells_and_unknown_files() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out) = (dir.path().join("in"), dir.path().join("out"));
    let in_table = "ui_image/menu/logo";
    let in_binary = "datatable/weapons.rpak";
    let mut pak = PakBuilder::new();
    pak.dtbl(
        0xD1,
        0,
        false,
        &[("image", 5)],
        &[vec![DtblCell::Str(in_table)]],
    );
    write(&root, "paks/a.rpak", pak.build(8));
    let mut exe = vec![0u8, 1, 2];
    exe.extend_from_slice(format!("\0\0load \"{in_binary}\" now\0\u{1}").as_bytes());
    write(&root, "bin/game.exe", exe);
    // Not a name anywhere: only in the pak's raw bytes, which are not scanned.
    let hashes = format!(
        "0x{:016X}.txtr\n0x{:016X}.dtbl\n0x{:016X}.dtbl\n",
        string_to_guid(in_table),
        string_to_guid(in_binary),
        string_to_guid("settings/nowhere.rpak"),
    );
    write(dir.path(), "hashes.txt", hashes.into_bytes());

    let run = Command::new(env!("CARGO_BIN_EXE_respak"))
        .args([
            "--root".as_ref(),
            root.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
            "names".as_ref(),
        ])
        .args([
            "--hashes".as_ref(),
            dir.path().join("hashes.txt").as_os_str(),
        ])
        .arg("--guids")
        .args(["paks/a.rpak", "bin/*.exe"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8(run.stdout).unwrap();
    let mut lines: Vec<_> = stdout.lines().collect();
    lines.sort();
    let mut want = vec![
        format!("0x{:016X}\t{in_table}", string_to_guid(in_table)),
        format!("0x{:016X}\t{in_binary}", string_to_guid(in_binary)),
    ];
    want.sort();
    assert_eq!(lines, want);
    let log = String::from_utf8_lossy(&run.stderr);
    assert!(log.contains("[1/2] reading paks/a.rpak"), "{log}");
    assert!(
        log.contains("[2/2] reading bin/game.exe") && log.contains("reading as text"),
        "{log}"
    );
    assert!(log.contains("2 of 3 guids named"), "{log}");
}

#[test]
fn without_a_hash_file_the_unnamed_assets_of_the_sources_are_looked_for() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out) = (dir.path().join("in"), dir.path().join("out"));
    let name = "datatable/weapons.rpak";
    let mut pak = PakBuilder::new();
    pak.dtbl(string_to_guid(name), 0, false, &[("x", 1)], &[]);
    write(&root, "paks/a.rpak", pak.build(8));
    write(
        &root,
        "bin/game.exe",
        format!("\0\0load \"{name}\" now\0").into_bytes(),
    );

    let run = Command::new(env!("CARGO_BIN_EXE_respak"))
        .args([
            "--root".as_ref(),
            root.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
            "names".as_ref(),
        ])
        .args(["paks/a.rpak", "bin/*.exe"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8(run.stdout).unwrap(),
        format!("{name}\n")
    );
    assert!(String::from_utf8_lossy(&run.stderr).contains("1 of 1 guids named"));
}
