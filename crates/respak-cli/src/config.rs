//! `respak.toml` and `respak-<profile>.toml`: defaults for the global flags.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::CommandFactory;
use serde::Deserialize;

use crate::Cli;

/// The file searched for when no profile is named.
const DEFAULT_FILE: &str = "respak.toml";

/// The contents of a config file. Every key is optional and has the flag of the same name; a
/// flag given on the command line wins. Relative paths are relative to the file's directory.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `--root`.
    pub root: Option<PathBuf>,
    /// `--out`.
    pub out: Option<PathBuf>,
    /// `--cache`.
    pub cache: Option<PathBuf>,
    /// `--report`.
    pub report: Option<PathBuf>,
    /// `--search`.
    pub search: Option<Vec<String>>,
    /// `--no-patches`.
    pub no_patches: Option<bool>,
    /// Options of the Respawn plugin.
    #[serde(default)]
    pub respawn: RespawnConfig,
    /// `--steam` and its companions; the alternative to `root`.
    pub steam: Option<SteamConfig>,
}

/// The `[respawn]` table.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespawnConfig {
    /// `--rig`.
    pub rig: Option<String>,
    /// `--studio`.
    pub studio: Option<String>,
}

/// The `[steam]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamConfig {
    /// `--steam`.
    pub app: u32,
    /// `--depot`: a depot id, or `"ID=MANIFEST"`.
    #[serde(default)]
    pub depots: Vec<DepotEntry>,
    /// `--branch`.
    pub branch: Option<String>,
    /// `--steam-cache-size`, e.g. `"8GiB"`.
    pub cache_size: Option<String>,
}

/// One entry of `depots`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DepotEntry {
    /// A bare depot id: the branch's current manifest.
    Id(u32),
    /// `"ID"` or `"ID=MANIFEST"`, as on the command line.
    Spec(String),
}

impl std::fmt::Display for DepotEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(id) => write!(f, "{id}"),
            Self::Spec(text) => f.write_str(text),
        }
    }
}

impl Config {
    /// Makes the paths absolute against `base`, the directory of the file they were read from.
    fn anchor(&mut self, base: &Path) {
        for path in [
            &mut self.root,
            &mut self.out,
            &mut self.cache,
            &mut self.report,
        ]
        .into_iter()
        .flatten()
        {
            if path.is_relative() {
                *path = base.join(&*path);
            }
        }
    }
}

/// `dir/name` for the nearest `dir` among `start` and its parents that has such a file.
fn find_up(start: &Path, name: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|file| file.is_file())
}

/// The config the command line selects, searching upward from `cwd`: `--config FILE`, else
/// `respak-<profile>.toml` for a profile (which must exist), else `respak.toml` if there is one.
/// `--no-config` selects none.
pub fn load(cli: &Cli, cwd: &Path) -> anyhow::Result<Config> {
    if cli.no_config {
        return Ok(Config::default());
    }
    let file = match (&cli.config, &cli.profile) {
        (Some(file), _) => file.clone(),
        (None, Some(profile)) => {
            let name = format!("respak-{profile}.toml");
            find_up(cwd, &name).ok_or_else(|| {
                anyhow::anyhow!(
                    "profile `{profile}`: {name} not found in {} or any parent directory \
                     (the commands are: {})",
                    cwd.display(),
                    command_names().join(", ")
                )
            })?
        }
        (None, None) => match find_up(cwd, DEFAULT_FILE) {
            Some(file) => file,
            None => return Ok(Config::default()),
        },
    };
    tracing::info!("config: {}", file.display());
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("cannot read {}", file.display()))?;
    let mut config: Config =
        toml::from_str(&text).with_context(|| format!("invalid {}", file.display()))?;
    config.anchor(file.parent().unwrap_or(Path::new("")));
    Ok(config)
}

/// The names of the commands, for messages.
pub(crate) fn command_names() -> Vec<String> {
    Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_are_errors() {
        assert!(toml::from_str::<Config>("rooot = \"x\"").is_err());
        assert!(toml::from_str::<Config>("[respawn]\nrigg = \"x\"").is_err());
        assert!(toml::from_str::<Config>("[steam]\napp = 1\nbranches = \"x\"").is_err());
    }

    #[test]
    fn full_file() {
        let config: Config = toml::from_str(
            r#"
            root = "game"
            out = "out"
            search = ["a/*.rpak"]
            no_patches = true
            [respawn]
            rig = "auto"
            [steam]
            app = 7
            depots = [8, "9=10"]
            "#,
        )
        .unwrap();
        assert_eq!(config.search.as_deref(), Some(&["a/*.rpak".to_owned()][..]));
        assert_eq!(config.no_patches, Some(true));
        let steam = config.steam.unwrap();
        let depots: Vec<_> = steam.depots.iter().map(ToString::to_string).collect();
        assert_eq!(depots, ["8", "9=10"]);
    }

    #[test]
    fn paths_are_anchored_at_the_file() {
        let mut config: Config =
            toml::from_str("root = \"game\"\nout = \"/abs/out\"\ncache = \"c\"").unwrap();
        let base = Path::new("base");
        config.anchor(base);
        assert_eq!(config.root, Some(base.join("game")));
        assert_eq!(config.cache, Some(base.join("c")));
        assert_eq!(config.report, None);
        #[cfg(unix)]
        assert_eq!(config.out, Some(PathBuf::from("/abs/out")));
    }

    #[test]
    fn nearest_file_wins() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_up(&deep, "respak.toml"), None);
        std::fs::write(dir.path().join("respak.toml"), "").unwrap();
        assert_eq!(
            find_up(&deep, "respak.toml"),
            Some(dir.path().join("respak.toml"))
        );
        std::fs::write(dir.path().join("a/respak.toml"), "").unwrap();
        assert_eq!(
            find_up(&deep, "respak.toml"),
            Some(dir.path().join("a/respak.toml"))
        );
    }
}
