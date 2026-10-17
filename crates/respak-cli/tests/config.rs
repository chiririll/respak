//! `respak.toml`, profiles and help, through the binary.

use std::path::Path;
use std::process::{Command, Output};

/// Runs `respak args` in `cwd`.
fn respak(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_respak"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A directory with `respak.toml` (writing `default.json`) and `respak-apex.toml` (`apex.json`),
/// and a `sub/dir` below it to run in.
fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("sub/dir");
    std::fs::create_dir_all(&work).unwrap();
    for (file, report) in [
        ("respak.toml", "default.json"),
        ("respak-apex.toml", "apex.json"),
    ] {
        std::fs::write(
            dir.path().join(file),
            format!("root = \"in\"\nout = \"out\"\nreport = \"{report}\"\n"),
        )
        .unwrap();
    }
    (dir, work)
}

#[test]
fn the_config_of_a_parent_directory_supplies_the_flags() {
    let (dir, work) = project();
    let output = respak(&work, &["formats"]);
    assert!(output.status.success(), "{}", stderr(&output));
    // Paths are relative to the file, not to where respak ran.
    assert!(dir.path().join("default.json").is_file());
    assert!(dir.path().join("in").is_dir() && dir.path().join("out").is_dir());
    assert!(!dir.path().join("apex.json").exists());
}

#[test]
fn a_profile_reads_its_own_file_only() {
    let (dir, work) = project();
    let output = respak(&work, &["apex", "formats"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(dir.path().join("apex.json").is_file());
    assert!(!dir.path().join("default.json").exists());
}

#[test]
fn a_missing_profile_is_an_error() {
    let (dir, work) = project();
    let output = respak(&work, &["covnert", "formats"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("respak-covnert.toml not found"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("default.json").exists());
}

#[test]
fn the_command_line_beats_the_file() {
    let (dir, work) = project();
    let cli_report = dir.path().join("cli.json");
    let output = respak(
        &work,
        &["formats", "--report", cli_report.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(cli_report.is_file() && !dir.path().join("default.json").exists());
}

#[test]
fn config_replaces_the_profile_lookup_with_a_warning() {
    let (dir, work) = project();
    let other = dir.path().join("other.toml");
    std::fs::write(
        &other,
        "root = \"in\"\nout = \"out\"\nreport = \"other.json\"\n",
    )
    .unwrap();
    let output = respak(
        &work,
        &["--config", other.to_str().unwrap(), "apex", "formats"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("profile is ignored"));
    assert!(dir.path().join("other.json").is_file());
    assert!(!dir.path().join("apex.json").exists());
}

#[test]
fn no_config_reads_no_file() {
    let (_dir, work) = project();
    let output = respak(&work, &["--no-config", "formats"]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(
        text.contains("--root") && text.contains("is required"),
        "{text}"
    );
}

#[test]
fn a_config_with_a_typo_is_an_error() {
    let (dir, work) = project();
    std::fs::write(dir.path().join("respak.toml"), "rooot = \"in\"\n").unwrap();
    let output = respak(&work, &["formats"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("respak.toml"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_command_without_arguments_prints_its_help() {
    let (_dir, work) = project();
    for command in ["convert", "list", "dump"] {
        let output = respak(&work, &[command]);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            stderr(&output)
        );
        assert!(!output.status.success());
        assert!(
            text.contains("Usage:") && text.contains(&format!("{command} [OPTIONS]")),
            "{text}"
        );
    }
    let output = respak(&work, &[]);
    assert!(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    )
    .contains("Usage: respak"));
}
