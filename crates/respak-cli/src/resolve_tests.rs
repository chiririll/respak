//! `Settings::resolve` through the command line.

use std::path::PathBuf;

use clap::Parser;
use respak_host::{Config, Settings, Source};

use crate::Cli;

fn cli(args: &[&str]) -> Cli {
    Cli::try_parse_from(["respak"].iter().chain(args).chain(&["formats"])).unwrap()
}

fn config(text: &str) -> Config {
    toml::from_str(text).unwrap()
}

fn resolve(args: &[&str], text: &str) -> anyhow::Result<Settings> {
    Settings::resolve(&cli(args).overrides(), config(text))
}

fn message(args: &[&str], text: &str) -> String {
    resolve(args, text).unwrap_err().to_string()
}

#[test]
fn command_line_alone() {
    let settings = resolve(
        &[
            "--root", "r", "--out", "o", "--search", "a", "--search", "b",
        ],
        "",
    )
    .unwrap();
    assert_eq!(settings.source, Source::Root("r".into()));
    assert_eq!(settings.out, PathBuf::from("o"));
    assert_eq!(settings.search, ["a", "b"]);
    assert_eq!(
        (settings.rig.as_str(), settings.studio.as_str()),
        ("auto", "auto")
    );
    assert!(!settings.no_patches && settings.cache.is_none() && settings.report.is_none());
}

#[test]
fn config_alone() {
    let settings = resolve(
        &[],
        r#"root = "r"
           out = "o"
           cache = "c"
           search = ["a"]
           no_patches = true
           [respawn]
           rig = "0x1"
           studio = "v17""#,
    )
    .unwrap();
    assert_eq!(settings.source, Source::Root("r".into()));
    assert_eq!(settings.cache, Some("c".into()));
    assert_eq!(settings.search, ["a"]);
    assert!(settings.no_patches);
    assert_eq!(
        (settings.rig.as_str(), settings.studio.as_str()),
        ("0x1", "v17")
    );
}

#[test]
fn command_line_wins() {
    let settings = resolve(
        &[
            "--root", "cli", "--search", "x", "--rig", "auto", "--out", "cli-out",
        ],
        r#"root = "cfg"
           out = "cfg-out"
           search = ["a", "b"]
           [respawn]
           rig = "0x1""#,
    )
    .unwrap();
    assert_eq!(settings.source, Source::Root("cli".into()));
    assert_eq!(settings.out, PathBuf::from("cli-out"));
    assert_eq!(settings.search, ["x"]);
    assert_eq!(settings.rig, "auto");
}

#[test]
fn missing_input_and_output() {
    let missing = message(&["--out", "o"], "");
    assert!(
        missing.contains("--root") && missing.contains("is required"),
        "{missing}"
    );
    assert!(message(&["--root", "r"], "").contains("--out is required"));
}

#[cfg(feature = "steam")]
mod steam {
    use respak_host::SteamSource;
    use respak_steam::DepotSpec;

    use super::*;

    fn depot(depot: u32, manifest: Option<u64>) -> DepotSpec {
        DepotSpec { depot, manifest }
    }

    fn steam(settings: Settings) -> SteamSource {
        match settings.source {
            Source::Steam(steam) => steam,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn flags() {
        let got = steam(
            resolve(
                &[
                    "--steam",
                    "1",
                    "--depot",
                    "2=3",
                    "--depot",
                    "4",
                    "--branch",
                    "beta",
                    "--steam-cache-size",
                    "500MiB",
                    "--out",
                    "o",
                ],
                "",
            )
            .unwrap(),
        );
        assert_eq!(got.app, 1);
        assert_eq!(got.depots, [depot(2, Some(3)), depot(4, None)]);
        assert_eq!(got.branch, "beta");
        assert_eq!(got.cache_size, Some(500 << 20));
    }

    const CONFIG: &str = r#"out = "o"
        [steam]
        app = 1
        depots = [2, "3=4"]
        branch = "beta"
        cache_size = "1GiB""#;

    #[test]
    fn from_config() {
        let got = steam(resolve(&[], CONFIG).unwrap());
        assert_eq!(got.app, 1);
        assert_eq!(got.depots, [depot(2, None), depot(3, Some(4))]);
        assert_eq!(got.branch, "beta");
        assert_eq!(got.cache_size, Some(1 << 30));
    }

    #[test]
    fn companions_refine_the_config() {
        let got = steam(
            resolve(
                &[
                    "--depot",
                    "9",
                    "--branch",
                    "public",
                    "--steam-cache-size",
                    "1",
                ],
                CONFIG,
            )
            .unwrap(),
        );
        assert_eq!(
            (got.app, got.branch.as_str(), got.cache_size),
            (1, "public", Some(1))
        );
        assert_eq!(got.depots, [depot(9, None)]);
    }

    #[test]
    fn a_command_line_input_replaces_the_config_input() {
        let got = steam(resolve(&["--steam", "5", "--depot", "6"], CONFIG).unwrap());
        assert_eq!(
            (got.app, got.branch.as_str(), got.cache_size),
            (5, "public", None)
        );
        assert_eq!(got.depots, [depot(6, None)]);
        let root = resolve(&["--root", "r"], CONFIG).unwrap();
        assert_eq!(root.source, Source::Root("r".into()));
    }

    #[test]
    fn what_does_not_fit_is_refused() {
        assert!(message(&["--steam", "1", "--out", "o"], "").contains("at least one --depot"));
        assert!(message(&["--depot", "2", "--out", "o"], "").contains("need --steam"));
        assert!(message(&["--root", "r", "--branch", "b"], "out = \"o\"").contains("need --steam"));
        assert!(message(&[], "out = \"o\"\nroot = \"r\"\n[steam]\napp = 1").contains("use one"));
        assert!(message(&[], "out = \"o\"\n[steam]\napp = 1\ndepots = [\"x\"]").contains("depots"));
        assert!(message(
            &[],
            "out = \"o\"\n[steam]\napp = 1\ndepots = [2]\ncache_size = \"x\""
        )
        .contains("cache_size"));
    }
}

#[test]
fn the_memory_cache_is_off_unless_asked_for() {
    let base = ["--root", "r", "--out", "o"];
    assert_eq!(resolve(&base, "").unwrap().memory_cache, None);
    let on = [&base[..], &["--memory-cache", "256MiB"]].concat();
    assert_eq!(resolve(&on, "").unwrap().memory_cache, Some(256 << 20));
    let zero = [&base[..], &["--memory-cache", "0"]].concat();
    assert_eq!(resolve(&zero, "").unwrap().memory_cache, None);
}

#[test]
fn the_memory_cache_can_come_from_the_config_file() {
    let base = ["--root", "r", "--out", "o"];
    let file = "memory_cache = \"1MiB\"";
    assert_eq!(resolve(&base, file).unwrap().memory_cache, Some(1 << 20));
    let cli = [&base[..], &["--memory-cache", "0"]].concat();
    assert_eq!(resolve(&cli, file).unwrap().memory_cache, None);
    assert!(message(&base, "memory_cache = \"lots\"").contains("memory_cache"));
}
