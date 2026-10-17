//! The command line and the config file, merged into what the engine is built from.

use std::path::PathBuf;

use anyhow::bail;

use crate::config::Config;
use crate::Cli;

/// Where the input comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A directory (`--root`).
    Root(PathBuf),
    /// Steam depots (`--steam`).
    #[cfg(feature = "steam")]
    Steam(SteamSource),
}

/// What `--steam` and its companions say.
#[cfg(feature = "steam")]
#[derive(Debug, Clone, PartialEq)]
pub struct SteamSource {
    /// The app id.
    pub app: u32,
    /// The depots, later ones winning collisions.
    pub depots: Vec<respak_steam::DepotSpec>,
    /// The branch whose current manifests unpinned depots take.
    pub branch: String,
    /// Cap of the chunk cache in bytes, if not the default.
    pub cache_size: Option<u64>,
}

/// Every option the engine is built from, with each default applied: the command line wins, then
/// the config file, then the built-in default.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The input.
    pub source: Source,
    /// The output directory.
    pub out: PathBuf,
    /// `--search` globs.
    pub search: Vec<String>,
    /// `--no-patches`.
    pub no_patches: bool,
    /// `--rig`.
    pub rig: String,
    /// `--studio`.
    pub studio: String,
    /// `--cache`.
    pub cache: Option<PathBuf>,
    /// `--report`.
    pub report: Option<PathBuf>,
}

impl Settings {
    /// Merges `cli` over `config`. Fails if the input or the output is not set anywhere, or the
    /// Steam options do not fit together.
    pub fn resolve(cli: &Cli, config: Config) -> anyhow::Result<Self> {
        let source = source(cli, &config)?;
        let Some(out) = cli.out.clone().or(config.out.clone()) else {
            bail!("--out is required (or `out` in the config file)");
        };
        Ok(Self {
            source,
            out,
            search: if cli.search.is_empty() {
                config.search.unwrap_or_default()
            } else {
                cli.search.clone()
            },
            no_patches: cli.no_patches || config.no_patches.unwrap_or(false),
            rig: cli
                .rig
                .clone()
                .or(config.respawn.rig)
                .unwrap_or_else(|| "auto".into()),
            studio: cli
                .studio
                .clone()
                .or(config.respawn.studio)
                .unwrap_or_else(|| "auto".into()),
            cache: cli.cache.clone().or(config.cache),
            report: cli.report.clone().or(config.report),
        })
    }
}

/// The input. `--root` and `--steam` are one choice: if the command line makes it, the config's
/// `root` and `[steam]` are not looked at; if not, the config makes it. The Steam companions
/// (`--depot`, `--branch`, `--steam-cache-size`) given alone refine a config that has `[steam]`.
#[cfg(feature = "steam")]
fn source(cli: &Cli, config: &Config) -> anyhow::Result<Source> {
    let from_cli = cli.root.is_some() || cli.steam.is_some();
    let (root, steam) = if from_cli {
        (cli.root.as_ref(), None)
    } else {
        (config.root.as_ref(), config.steam.as_ref())
    };
    let app = if from_cli {
        cli.steam
    } else {
        steam.map(|steam| steam.app)
    };
    if root.is_some() && app.is_some() {
        bail!("the config file sets both `root` and `[steam]`; use one");
    }
    let companions =
        !cli.depot.is_empty() || cli.branch.is_some() || cli.steam_cache_size.is_some();
    if let Some(root) = root {
        if companions {
            bail!("--depot, --branch and --steam-cache-size need --steam");
        }
        return Ok(Source::Root(root.clone()));
    }
    let Some(app) = app else {
        if companions {
            bail!("--depot, --branch and --steam-cache-size need --steam");
        }
        bail!("--root (or --steam) is required (or `root` / `[steam]` in the config file)");
    };
    let depots = if !cli.depot.is_empty() {
        cli.depot.clone()
    } else {
        steam
            .map(|steam| {
                steam
                    .depots
                    .iter()
                    .map(|depot| {
                        depot.to_string().parse().map_err(|error| {
                            anyhow::anyhow!("`depots` in the config file: {error}")
                        })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default()
    };
    if depots.is_empty() {
        bail!("--steam needs at least one --depot (or `depots` in the config file)");
    }
    let config_cache = steam
        .and_then(|steam| steam.cache_size.as_deref())
        .map(|size| {
            crate::parse_size(size)
                .map_err(|error| anyhow::anyhow!("`cache_size` in the config file: {error}"))
        })
        .transpose()?;
    Ok(Source::Steam(SteamSource {
        app,
        depots,
        branch: cli
            .branch
            .clone()
            .or_else(|| steam.and_then(|steam| steam.branch.clone()))
            .unwrap_or_else(|| "public".into()),
        cache_size: cli.steam_cache_size.or(config_cache),
    }))
}

/// The input of a build without Steam: `--root`, else the config's `root`.
#[cfg(not(feature = "steam"))]
fn source(cli: &Cli, config: &Config) -> anyhow::Result<Source> {
    match cli.root.clone().or(config.root.clone()) {
        Some(root) => Ok(Source::Root(root)),
        None if config.steam.is_some() => bail!("this build has no Steam support"),
        None => bail!("--root is required (or `root` in the config file)"),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(["respak"].iter().chain(args).chain(&["formats"])).unwrap()
    }

    fn config(text: &str) -> Config {
        toml::from_str(text).unwrap()
    }

    fn resolve(args: &[&str], text: &str) -> anyhow::Result<Settings> {
        Settings::resolve(&cli(args), config(text))
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
            assert!(
                message(&["--root", "r", "--branch", "b"], "out = \"o\"").contains("need --steam")
            );
            assert!(message(&[], "out = \"o\"\nroot = \"r\"\n[steam]\napp = 1").contains("use one"));
            assert!(
                message(&[], "out = \"o\"\n[steam]\napp = 1\ndepots = [\"x\"]").contains("depots")
            );
            assert!(message(
                &[],
                "out = \"o\"\n[steam]\napp = 1\ndepots = [2]\ncache_size = \"x\""
            )
            .contains("cache_size"));
        }
    }
}
