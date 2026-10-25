//! The host's overrides and the config file, merged into what the engine is built from.

use std::path::PathBuf;

use anyhow::bail;

use crate::config::Config;

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
    /// Print a line per chunk request and downloaded chunk (`--steam-verbose`).
    pub verbose: bool,
}

/// What a host asks for on top of the config file: the command line of the CLI, nothing in the web
/// UI. Every field wins over the file's.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    /// `--root`.
    pub root: Option<PathBuf>,
    /// `--out`.
    pub out: Option<PathBuf>,
    /// `--search`.
    pub search: Vec<String>,
    /// `--no-patches`.
    pub no_patches: bool,
    /// `--rig`.
    pub rig: Option<String>,
    /// `--studio`.
    pub studio: Option<String>,
    /// `--cache`.
    pub cache: Option<PathBuf>,
    /// `--memory-cache`.
    pub memory_cache: Option<u64>,
    /// `--report`.
    pub report: Option<PathBuf>,
    /// `--steam`.
    #[cfg(feature = "steam")]
    pub steam: Option<u32>,
    /// `--branch`.
    #[cfg(feature = "steam")]
    pub branch: Option<String>,
    /// `--depot`.
    #[cfg(feature = "steam")]
    pub depot: Vec<respak_steam::DepotSpec>,
    /// `--steam-cache-size`.
    #[cfg(feature = "steam")]
    pub steam_cache_size: Option<u64>,
    /// `--steam-verbose`.
    #[cfg(feature = "steam")]
    pub steam_verbose: bool,
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
    /// `--memory-cache` (or `memory_cache` in the config file) in bytes; `None` is off (an explicit 0 is off too).
    pub memory_cache: Option<u64>,
    /// `--report`.
    pub report: Option<PathBuf>,
}

impl Settings {
    /// Merges the overrides over `config`. Fails if the input or the output is not set anywhere, or the
    /// Steam options do not fit together.
    pub fn resolve(cli: &Overrides, config: Config) -> anyhow::Result<Self> {
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
            memory_cache: match (cli.memory_cache, config.memory_cache.as_deref()) {
                (Some(bytes), _) => Some(bytes),
                (None, Some(text)) => Some(crate::size::parse_size(text).map_err(|error| {
                    anyhow::anyhow!("`memory_cache` in the config file: {error}")
                })?),
                (None, None) => None,
            }
            .filter(|&bytes| bytes > 0),
            report: cli.report.clone().or(config.report),
        })
    }
}

/// The input. `--root` and `--steam` are one choice: if the command line makes it, the config's
/// `root` and `[steam]` are not looked at; if not, the config makes it. The Steam companions
/// (`--depot`, `--branch`, `--steam-cache-size`) given alone refine a config that has `[steam]`.
#[cfg(feature = "steam")]
fn source(cli: &Overrides, config: &Config) -> anyhow::Result<Source> {
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
            crate::size::parse_size(size)
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
        verbose: cli.steam_verbose || steam.and_then(|steam| steam.verbose).unwrap_or(false),
    }))
}

/// The input of a build without Steam: `--root`, else the config's `root`.
#[cfg(not(feature = "steam"))]
fn source(cli: &Overrides, config: &Config) -> anyhow::Result<Source> {
    match cli.root.clone().or(config.root.clone()) {
        Some(root) => Ok(Source::Root(root)),
        None if config.steam.is_some() => bail!("this build has no Steam support"),
        None => bail!("--root is required (or `root` in the config file)"),
    }
}
