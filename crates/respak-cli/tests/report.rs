//! `--report FILE`: the JSON result goes to a file, not to stdout.

use std::process::Command;

#[test]
fn report_goes_to_the_file_and_stdout_stays_empty() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sub/formats.json");
    let output = Command::new(env!("CARGO_BIN_EXE_respak"))
        .args(["--root"])
        .arg(dir.path().join("in"))
        .arg("--out")
        .arg(dir.path().join("out"))
        .arg("--report")
        .arg(&file)
        .arg("formats")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(json.is_object() || json.is_array());
}
