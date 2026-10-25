//! `respak.toml` and `respak-<profile>.toml`: defaults for the global flags.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The file searched for when no profile is named.
pub const DEFAULT_FILE: &str = "respak.toml";

/// The contents of a config file. Every key is optional and has the flag of the same name; a
/// flag given on the command line wins. Relative paths are relative to the file's directory.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `--root`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    /// `--out`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub out: Option<PathBuf>,
    /// `--cache`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<PathBuf>,
    /// `--report`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<PathBuf>,
    /// `--memory-cache`, e.g. `"256MiB"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_cache: Option<String>,
    /// `--search`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<Vec<String>>,
    /// `--no-patches`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_patches: Option<bool>,
    /// Options of the Respawn plugin.
    #[serde(default, skip_serializing_if = "RespawnConfig::is_empty")]
    pub respawn: RespawnConfig,
    /// `--steam` and its companions; the alternative to `root`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam: Option<SteamConfig>,
}

/// The `[respawn]` table.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespawnConfig {
    /// `--rig`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig: Option<String>,
    /// `--studio`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio: Option<String>,
}

impl RespawnConfig {
    /// Whether no option is set.
    pub fn is_empty(&self) -> bool {
        self.rig.is_none() && self.studio.is_none()
    }
}

/// The `[steam]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamConfig {
    /// `--steam`.
    pub app: u32,
    /// `--depot`: a depot id, or `"ID=MANIFEST"`.
    #[serde(default)]
    pub depots: Vec<DepotEntry>,
    /// `--branch`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// `--steam-cache-size`, e.g. `"8GiB"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_size: Option<String>,
    /// `--steam-verbose`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
}

/// One entry of `depots`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub fn anchor(&mut self, base: &Path) {
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

/// Anchors `config`'s paths at the directory of `file`.
pub fn anchor_all(config: &mut Config, file: &Path) {
    config.anchor(file.parent().unwrap_or(Path::new("")));
}

/// `dir/name` for the nearest `dir` among `start` and its parents that has such a file.
pub fn find_up(start: &Path, name: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|file| file.is_file())
}

/// Which config file a host asks for.
#[derive(Debug, Default, Clone)]
pub struct ConfigSelect {
    /// Read no file at all.
    pub none: bool,
    /// This file (`--config`).
    pub file: Option<PathBuf>,
    /// `respak-<profile>.toml`, which must exist.
    pub profile: Option<String>,
}

impl ConfigSelect {
    /// The file this selects, searching upward from `cwd`: the named file, else
    /// `respak-<profile>.toml` for a profile (which must exist), else `respak.toml` if there is
    /// one. `Ok(None)` when nothing is selected or found. `hint` ends the profile error.
    pub fn locate(&self, cwd: &Path, hint: &str) -> anyhow::Result<Option<PathBuf>> {
        if self.none {
            return Ok(None);
        }
        Ok(match (&self.file, &self.profile) {
            (Some(file), _) => Some(file.clone()),
            (None, Some(profile)) => {
                let name = format!("respak-{profile}.toml");
                Some(find_up(cwd, &name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "profile `{profile}`: {name} not found in {} or any parent directory{hint}",
                        cwd.display(),
                    )
                })?)
            }
            (None, None) => find_up(cwd, DEFAULT_FILE),
        })
    }
}

/// Reads `file` as it is written (paths not anchored).
pub fn load_file(file: &Path) -> anyhow::Result<Config> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid {}", file.display()))
}

/// The config `select` names, searching upward from `cwd`, with its paths anchored at the file;
/// the default if there is none.
pub fn load(select: &ConfigSelect, cwd: &Path, hint: &str) -> anyhow::Result<Config> {
    let Some(file) = select.locate(cwd, hint)? else {
        return Ok(Config::default());
    };
    tracing::info!("config: {}", file.display());
    let mut config = load_file(&file)?;
    anchor_all(&mut config, &file);
    Ok(config)
}

/// Writes `config` to `file`. If the file exists its comments, layout and key order are kept:
/// only the keys of the config are changed, added or removed.
pub fn save(file: &Path, config: &Config) -> anyhow::Result<()> {
    let new: toml_edit::DocumentMut = toml::to_string(config)
        .context("cannot serialize the config")?
        .parse()?;
    let mut doc = match std::fs::read_to_string(file) {
        Ok(text) => text
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("invalid {}", file.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml_edit::DocumentMut::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", file.display())),
    };
    merge(doc.as_table_mut(), new.as_table());
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, doc.to_string())
        .with_context(|| format!("cannot write {}", file.display()))
}

/// Makes `old` hold what `new` holds, keeping the decoration of values that stay.
fn merge(old: &mut toml_edit::Table, new: &toml_edit::Table) {
    let gone: Vec<String> = old
        .iter()
        .map(|(k, _)| k.to_owned())
        .filter(|k| !new.contains_key(k))
        .collect();
    for key in gone {
        old.remove(&key);
    }
    for (key, item) in new.iter() {
        match (old.get_mut(key), item) {
            (Some(toml_edit::Item::Table(o)), toml_edit::Item::Table(n)) => merge(o, n),
            (Some(toml_edit::Item::Value(o)), toml_edit::Item::Value(n)) => {
                let decor = o.decor().clone();
                *o = n.clone();
                *o.decor_mut() = decor;
            }
            _ => {
                old.insert(key, item.clone());
            }
        }
    }
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
            memory_cache = "256MiB"
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
        assert_eq!(config.memory_cache.as_deref(), Some("256MiB"));
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

    #[test]
    fn save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("respak.toml");
        let config: Config = toml::from_str(
            "root = \"g\"\nout = \"o\"\nsearch = [\"a\"]\n[respawn]\nrig = \"x\"\n[steam]\napp = 1\ndepots = [2, \"3=4\"]\n",
        )
        .unwrap();
        save(&file, &config).unwrap();
        assert_eq!(load_file(&file).unwrap(), config);
    }

    #[test]
    fn save_keeps_comments_and_drops_removed_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("respak.toml");
        std::fs::write(
            &file,
            "# my game\nroot = \"g\" # here\nout = \"o\"\ncache = \"c\"\n",
        )
        .unwrap();
        let mut config = load_file(&file).unwrap();
        config.root = Some("h".into());
        config.cache = None;
        config.no_patches = Some(true);
        save(&file, &config).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("# my game") && text.contains("# here"),
            "{text}"
        );
        assert!(
            text.contains("root = \"h\"") && !text.contains("cache"),
            "{text}"
        );
        assert_eq!(load_file(&file).unwrap(), config);
    }

    #[test]
    fn a_missing_profile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let select = ConfigSelect {
            profile: Some("nope".into()),
            ..ConfigSelect::default()
        };
        let error = select
            .locate(dir.path(), " (hint)")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("respak-nope.toml") && error.ends_with("(hint)"),
            "{error}"
        );
    }
}
