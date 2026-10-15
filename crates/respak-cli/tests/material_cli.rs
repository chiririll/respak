//! `--search` and `--cache` reach the plugin: the real binary on a temp directory (slice 13).

use std::path::Path;
use std::process::Command;

use respak_respawn::testing::{build_tex_pak, PakBuilder, TexSpec};

fn write(root: &Path, path: &str, bytes: Vec<u8>) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, bytes).unwrap();
}

fn texture() -> TexSpec {
    TexSpec::new(0xABCD, 10, 4, 4, 0, 1, vec![7; 16])
}

fn respak(root: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_respak"))
        .args(["--root".as_ref(), root.as_os_str(), "--out".as_ref(), out.as_os_str()])
        .args(extra)
        .args(["convert", "paks/lvl.rpak/0x00000000000000A1.matl", "--to", r#"{"type":"matl.json"}"#, "--path", "mat"])
        .output()
        .unwrap()
}

#[test]
fn search_and_cache_find_a_texture_in_another_pak() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out, cache) = (dir.path().join("in"), dir.path().join("out"), dir.path().join("cache"));
    write(&root, "elsewhere/tex.rpak", build_tex_pak(8, &[texture()]));
    let mut lvl = PakBuilder::new();
    lvl.matl(0xA1, "material/wall", 0, &[0xABCD]);
    write(&root, "paks/lvl.rpak", lvl.build(8));

    // Without --search the texture is missing: the export still succeeds but has no texture file.
    let plain = respak(&root, &out, &[]);
    assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
    assert!(!out.join("mat/textures/slot_0.png").exists());

    let found = respak(&root, &out, &["--search", "elsewhere/*.rpak", "--cache", cache.to_str().unwrap()]);
    assert!(found.status.success(), "{}", String::from_utf8_lossy(&found.stderr));
    assert!(out.join("mat/textures/slot_0.png").exists());
    assert!(cache.join("respawn/v1/guid/000000000000abcd.txtr").exists());

    // The index alone is enough on the next run.
    std::fs::remove_dir_all(&out).unwrap();
    let again = respak(&root, &out, &["--cache", cache.to_str().unwrap()]);
    assert!(again.status.success(), "{}", String::from_utf8_lossy(&again.stderr));
    assert!(out.join("mat/textures/slot_0.png").exists());
}

#[test]
fn copy_skips_assets_inside_paks_and_copies_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out) = (dir.path().join("in"), dir.path().join("out"));
    let mut lvl = PakBuilder::new();
    lvl.matl(0xA1, "material/wall", 0, &[]);
    let pak = lvl.build(8);
    write(&root, "paks/lvl.rpak", pak.clone());

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_respak"))
            .args(["--root".as_ref(), root.as_os_str(), "--out".as_ref(), out.as_os_str()])
            .args(args)
            .output()
            .unwrap()
    };
    // The pak file is copied; its assets (not files) are skipped without an error.
    let copied = run(&["copy", "paks/**"]);
    assert!(copied.status.success(), "{}", String::from_utf8_lossy(&copied.stderr));
    assert_eq!(std::fs::read(out.join("paks/lvl.rpak")).unwrap(), pak);

    let assets = run(&["copy", "paks/lvl.rpak/*"]);
    assert!(assets.status.success(), "{}", String::from_utf8_lossy(&assets.stderr));

    let both = run(&["convert", "paks/lvl.rpak/*.matl", "--to", r#"{"type":"matl.json"}"#, "--copy"]);
    assert!(both.status.success(), "{}", String::from_utf8_lossy(&both.stderr));
}

#[test]
fn list_prints_paths_a_map_or_only_the_names() {
    let dir = tempfile::tempdir().unwrap();
    let (root, out) = (dir.path().join("in"), dir.path().join("out"));
    let mut named = texture();
    named.name = Some("ui/icon");
    write(&root, "c.rpak", build_tex_pak(8, &[named]));
    let list = |extra: &[&str]| {
        let run = Command::new(env!("CARGO_BIN_EXE_respak"))
            .args(["--root".as_ref(), root.as_os_str(), "--out".as_ref(), out.as_os_str(), "list".as_ref(), "c.rpak".as_ref()])
            .args(extra)
            .output()
            .unwrap();
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        String::from_utf8(run.stdout).unwrap()
    };

    assert_eq!(list(&[]), "0x000000000000ABCD.txtr\n");
    assert_eq!(list(&["--map"]), "0x000000000000ABCD.txtr\ttexture/ui/icon.txtr\n");
    assert_eq!(list(&["--names"]), "texture/ui/icon.txtr\n");
}
