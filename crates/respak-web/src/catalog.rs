//! The version catalog (docs/design/webui.md section 5a): named game versions with the Steam
//! manifests they were, so a version can be picked instead of typing depot and manifest ids.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The catalog that ships with the program: every `*.toml` of `catalog/`, one file per game.
#[derive(rust_embed::RustEmbed)]
#[folder = "catalog"]
struct Shipped;

/// The shipped files as (name, text), in file name order.
fn shipped() -> Vec<(String, String)> {
    let mut names: Vec<_> = Shipped::iter().filter(|n| n.ends_with(".toml")).collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let file = Shipped::get(&n)?;
            let text = String::from_utf8_lossy(&file.data).into_owned();
            Some((format!("shipped catalog {n}"), text))
        })
        .collect()
}

/// What kind of version this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionType {
    /// A season launch.
    Season,
    /// An in-game event.
    Event,
    /// A hotfix between the others.
    Hotfix,
}

/// A depot of a game.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepotDef {
    /// The depot id.
    pub id: u32,
    /// What it holds.
    #[serde(default)]
    pub name: Option<String>,
    /// Selected unless the user says otherwise (languages stay opt-in).
    #[serde(default)]
    pub default: bool,
}

/// A manifest of one depot. A string: the ids are 64-bit and TOML integers are not.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestRef {
    /// The depot.
    pub depot: u32,
    /// The manifest id.
    pub manifest: String,
}

/// One version of a game: the manifests of the depots that changed in it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    /// A slug; a user file overrides a shipped version by this id.
    pub id: String,
    /// The name shown.
    pub name: String,
    /// When it came out; orders the versions.
    pub date: toml::value::Datetime,
    /// Season, event or hotfix.
    #[serde(rename = "type", default)]
    pub kind: Option<VersionType>,
    /// Versions of one group are shown together (`season-20`).
    #[serde(default)]
    pub group: Option<String>,
    /// The depots that changed, with their manifests.
    #[serde(default)]
    pub manifests: Vec<ManifestRef>,
}

/// A game: its app, depots and versions.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    /// The name shown.
    pub name: String,
    /// The Steam app id.
    pub app_id: u32,
    /// Its depots.
    #[serde(default)]
    pub depots: Vec<DepotDef>,
    /// Its versions, in file order.
    #[serde(default)]
    pub versions: Vec<Version>,
}

/// Every game of every file read, by key.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// Games, by the key of their table.
    pub games: BTreeMap<String, Game>,
    /// Files that could not be used, with why. A broken user file never hides the others.
    pub problems: Vec<Problem>,
}

/// A file the catalog could not read.
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    /// The file.
    pub file: String,
    /// What is wrong, naming the key.
    pub message: String,
}

/// Parses one catalog file. Errors name the file and say which key is wrong.
pub fn parse(name: &str, text: &str) -> Result<BTreeMap<String, Game>, String> {
    let games: BTreeMap<String, Game> = toml::from_str(text)
        .map_err(|e| format!("{name}: {}", e.message().to_owned() + &at(&e, text)))?;
    for (key, game) in &games {
        let mut seen = std::collections::HashSet::new();
        for version in &game.versions {
            if !seen.insert(&version.id) {
                return Err(format!(
                    "{name}: [{key}] has two versions with the id `{}`",
                    version.id
                ));
            }
            if version.date.date.is_none() {
                return Err(format!(
                    "{name}: [{key}] version `{}`: `date` must be a date like 2024-02-13",
                    version.id
                ));
            }
            for m in &version.manifests {
                if !m.manifest.chars().all(|c| c.is_ascii_digit()) || m.manifest.is_empty() {
                    return Err(format!(
                        "{name}: [{key}] version `{}`: manifest `{}` of depot {} is not a number",
                        version.id, m.manifest, m.depot
                    ));
                }
            }
        }
    }
    Ok(games)
}

/// ` (line 7)` for a TOML error that knows where it is.
fn at(error: &toml::de::Error, text: &str) -> String {
    match error.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            format!(" (line {line})")
        }
        None => String::new(),
    }
}

impl Catalog {
    /// The shipped catalog plus every `*.toml` of `user_dir`, in file name order: a user version
    /// replaces a shipped one with the same id, a new id is added; depots the same by id.
    pub fn load(user_dir: Option<&Path>) -> Self {
        let mut catalog = Catalog::default();
        for (name, text) in shipped() {
            match parse(&name, &text) {
                Ok(games) => catalog.merge(games),
                Err(message) => catalog.problems.push(Problem { file: name, message }),
            }
        }
        let Some(dir) = user_dir else { return catalog };
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for file in files {
            let shown = file.display().to_string();
            match std::fs::read_to_string(&file) {
                Ok(text) => match parse(&shown, &text) {
                    Ok(games) => catalog.merge(games),
                    Err(message) => catalog.problems.push(Problem {
                        file: shown,
                        message,
                    }),
                },
                Err(e) => catalog.problems.push(Problem {
                    file: shown.clone(),
                    message: format!("{shown}: {e}"),
                }),
            }
        }
        catalog
    }

    /// Lays `games` over what is there.
    pub fn merge(&mut self, games: BTreeMap<String, Game>) {
        for (key, game) in games {
            let Some(have) = self.games.get_mut(&key) else {
                self.games.insert(key, game);
                continue;
            };
            have.name = game.name;
            have.app_id = game.app_id;
            for depot in game.depots {
                match have.depots.iter_mut().find(|d| d.id == depot.id) {
                    Some(slot) => *slot = depot,
                    None => have.depots.push(depot),
                }
            }
            for version in game.versions {
                match have.versions.iter_mut().find(|v| v.id == version.id) {
                    Some(slot) => *slot = version,
                    None => have.versions.push(version),
                }
            }
        }
    }
}

/// `2024-02-13`, for ordering and showing.
fn day(version: &Version) -> (u16, u8, u8) {
    version
        .date
        .date
        .map_or((0, 0, 0), |d| (d.year, d.month, d.day))
}

/// What picking a version gives.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    /// The Steam app.
    pub app: u32,
    /// `ID=MANIFEST`, one per depot that has a manifest at this version.
    pub depots: Vec<String>,
    /// Depots asked for that no version up to this one has a manifest for.
    pub left_out: Vec<u32>,
}

/// Why a version cannot be resolved.
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// No such game.
    Game(String),
    /// No such version.
    Version(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Game(g) => write!(f, "no game `{g}` in the catalog"),
            Self::Version(v) => write!(f, "no version `{v}`"),
        }
    }
}

impl Game {
    /// The manifests of `version` for `depots` (the default depots when `None`). A version lists
    /// only the depots that changed; for another depot the closest earlier version (by date, ties
    /// by file order) that lists it gives the manifest.
    pub fn resolve(&self, version: &str, depots: Option<&[u32]>) -> Result<Resolved, ResolveError> {
        let mut order: Vec<&Version> = self.versions.iter().collect();
        order.sort_by_key(|v| day(v)); // stable: ties keep the file order
        let at = order
            .iter()
            .position(|v| v.id == version)
            .ok_or_else(|| ResolveError::Version(version.to_owned()))?;
        let wanted: Vec<u32> = match depots {
            Some(depots) => depots.to_vec(),
            None => {
                let defaults: Vec<u32> = self
                    .depots
                    .iter()
                    .filter(|d| d.default)
                    .map(|d| d.id)
                    .collect();
                if defaults.is_empty() {
                    self.depots.iter().map(|d| d.id).collect()
                } else {
                    defaults
                }
            }
        };
        let (mut pinned, mut left_out) = (Vec::new(), Vec::new());
        for depot in wanted {
            let found = order[..=at]
                .iter()
                .rev()
                .find_map(|v| v.manifests.iter().find(|m| m.depot == depot));
            match found {
                Some(m) => pinned.push(format!("{depot}={}", m.manifest)),
                None => left_out.push(depot),
            }
        }
        Ok(Resolved {
            app: self.app_id,
            depots: pinned,
            left_out,
        })
    }
}

/// The catalog as the page reads it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    /// The games.
    pub games: Vec<GameView>,
    /// Files that could not be read.
    pub problems: Vec<Problem>,
    /// Where user catalogs are read from.
    pub user_dir: Option<String>,
}

/// A game for the page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    /// The key.
    pub key: String,
    /// The name.
    pub name: String,
    /// The app.
    pub app_id: u32,
    /// Depots.
    pub depots: Vec<DepotDef>,
    /// Versions, newest first.
    pub versions: Vec<VersionView>,
}

/// A version for the page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionView {
    /// The slug.
    pub id: String,
    /// The name.
    pub name: String,
    /// `2024-02-13`.
    pub date: String,
    /// Season, event or hotfix.
    #[serde(rename = "type")]
    pub kind: Option<VersionType>,
    /// The group.
    pub group: Option<String>,
    /// The depots it lists.
    pub changed: Vec<u32>,
}

impl Catalog {
    /// What the page shows.
    pub fn view(&self, user_dir: Option<&Path>) -> CatalogView {
        let games = self
            .games
            .iter()
            .map(|(key, game)| {
                let mut order: Vec<&Version> = game.versions.iter().collect();
                order.sort_by_key(|v| std::cmp::Reverse(day(v)));
                GameView {
                    key: key.clone(),
                    name: game.name.clone(),
                    app_id: game.app_id,
                    depots: game.depots.clone(),
                    versions: order
                        .into_iter()
                        .map(|v| {
                            let (y, m, d) = day(v);
                            VersionView {
                                id: v.id.clone(),
                                name: v.name.clone(),
                                date: format!("{y:04}-{m:02}-{d:02}"),
                                kind: v.kind,
                                group: v.group.clone(),
                                changed: v.manifests.iter().map(|m| m.depot).collect(),
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        CatalogView {
            games,
            problems: self.problems.clone(),
            user_dir: user_dir.map(|d| d.display().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
        [apex]
        name = "Apex"
        app_id = 1
        [[apex.depots]]
        id = 10
        default = true
        [[apex.depots]]
        id = 11
        [[apex.versions]]
        id = "s1"
        name = "Season 1"
        date = 2020-01-01
        group = "season-1"
        manifests = [ { depot = 10, manifest = "100" }, { depot = 11, manifest = "110" } ]
        [[apex.versions]]
        id = "s1-hotfix"
        name = "Hotfix"
        date = 2020-01-05
        type = "hotfix"
        manifests = [ { depot = 10, manifest = "101" } ]
        [[apex.versions]]
        id = "s2"
        name = "Season 2"
        date = 2020-03-01
        manifests = [ { depot = 10, manifest = "200" } ]
    "#;

    fn game() -> Game {
        parse("t", FILE).unwrap().remove("apex").unwrap()
    }

    #[test]
    fn the_shipped_catalog_parses() {
        let files = shipped();
        assert!(files.len() >= 2, "{files:?}");
        for (name, text) in files {
            parse(&name, &text).unwrap();
        }
    }

    #[test]
    fn the_shipped_apex_versions_resolve() {
        let apex = parse("apex", &shipped().iter().find(|f| f.0.ends_with("apex.toml")).unwrap().1).unwrap().remove("apex").unwrap();
        // The Season 20 launch patch (seen 2024-02-13); the depots chosen are the first two.
        let got = apex.resolve("s20-launch", Some(&[1172471, 1172472])).unwrap();
        assert_eq!(got.depots, ["1172471=4691086100141750254", "1172472=146673051231967474"]);
        // Depot 3893181 first appeared in 2025.
        let got = apex.resolve("s20-launch", Some(&[3893181])).unwrap();
        assert_eq!(got.left_out, [3893181]);
        // An event that changed nothing takes the season's manifests.
        let event = apex.resolve("s20e01-5th-anniversary", Some(&[1172471])).unwrap();
        assert_eq!(event.depots, ["1172471=4691086100141750254"]);
    }

    #[test]
    fn an_unlisted_depot_takes_the_closest_earlier_manifest() {
        let got = game().resolve("s2", Some(&[10, 11])).unwrap();
        assert_eq!(got.depots, ["10=200", "11=110"]);
        let got = game().resolve("s1-hotfix", Some(&[10, 11])).unwrap();
        assert_eq!(got.depots, ["10=101", "11=110"]);
    }

    #[test]
    fn a_depot_with_no_earlier_manifest_is_left_out() {
        let mut g = game();
        g.versions.remove(0);
        let got = g.resolve("s2", Some(&[10, 11])).unwrap();
        assert_eq!(got.depots, ["10=200"]);
        assert_eq!(got.left_out, [11]);
    }

    #[test]
    fn only_the_chosen_depots_are_resolved_and_defaults_apply() {
        let g = game();
        assert_eq!(g.resolve("s2", Some(&[11])).unwrap().depots, ["11=110"]);
        // Depot 10 is the only default.
        assert_eq!(g.resolve("s2", None).unwrap().depots, ["10=200"]);
    }

    #[test]
    fn versions_are_ordered_by_date_and_ties_by_file_order() {
        let mut g = game();
        // Written before s1 in the file, but dated after it: it comes later.
        let mut late = g.versions[1].clone();
        late.id = "late".into();
        late.manifests = vec![ManifestRef {
            depot: 11,
            manifest: "999".into(),
        }];
        g.versions.insert(0, late);
        assert_eq!(g.resolve("s2", Some(&[11])).unwrap().depots, ["11=999"]);
        // Same date as s1-hotfix: the one written later in the file is the later one.
        let mut tie = g.versions[2].clone();
        tie.id = "tie".into();
        tie.date = g.versions[1].date;
        tie.manifests = vec![ManifestRef {
            depot: 10,
            manifest: "555".into(),
        }];
        g.versions.push(tie);
        assert_eq!(g.resolve("tie", Some(&[10])).unwrap().depots, ["10=555"]);
    }

    #[test]
    fn an_unknown_version_is_an_error() {
        assert_eq!(
            game().resolve("nope", None),
            Err(ResolveError::Version("nope".into()))
        );
    }

    #[test]
    fn mistakes_name_the_file_and_the_key() {
        let int_manifest = FILE.replace(r#"manifest = "100""#, "manifest = 100");
        let e = parse("my.toml", &int_manifest).unwrap_err();
        assert!(e.contains("my.toml") && e.contains("line"), "{e}");

        let typo = FILE.replace("app_id", "appid");
        assert!(parse("my.toml", &typo).unwrap_err().contains("my.toml"));

        let twice =
            format!("{FILE}\n[[apex.versions]]\nid = \"s1\"\nname = \"x\"\ndate = 2021-01-01\n");
        let e = parse("my.toml", &twice).unwrap_err();
        assert!(e.contains("my.toml") && e.contains("`s1`"), "{e}");

        let not_a_number = FILE.replace(r#"manifest = "100""#, r#"manifest = "abc""#);
        assert!(parse("my.toml", &not_a_number)
            .unwrap_err()
            .contains("not a number"));
    }

    const USER: &str = r#"
        [apex]
        name = "Apex Legends"
        app_id = 1
        [[apex.versions]]
        id = "s2"
        name = "Season 2 (fixed)"
        date = 2020-03-01
        manifests = [ { depot = 10, manifest = "222" } ]
        [[apex.versions]]
        id = "mine"
        name = "Mine"
        date = 2020-04-01
        [other]
        name = "Other"
        app_id = 2
    "#;

    #[test]
    fn a_user_version_replaces_the_one_with_its_id_and_new_ones_are_added() {
        let mut catalog = Catalog::default();
        catalog.merge(parse("shipped", FILE).unwrap());
        catalog.merge(parse("user", USER).unwrap());
        let apex = &catalog.games["apex"];
        assert_eq!(apex.name, "Apex Legends");
        assert_eq!(apex.versions.len(), 4);
        let s2 = apex.versions.iter().find(|v| v.id == "s2").unwrap();
        assert_eq!(s2.name, "Season 2 (fixed)");
        assert_eq!(apex.resolve("s2", Some(&[10])).unwrap().depots, ["10=222"]);
        // What the user did not mention stays.
        assert_eq!(apex.depots.len(), 2);
        assert!(catalog.games.contains_key("other"));
    }

    #[test]
    fn a_broken_user_file_is_reported_and_the_others_still_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a-mine.toml"), USER).unwrap();
        std::fs::write(
            dir.path().join("b-broken.toml"),
            "[apex
",
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a catalog").unwrap();
        let catalog = Catalog::load(Some(dir.path()));
        assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
        assert!(catalog.problems[0].file.contains("b-broken.toml"));
        assert!(catalog.games["apex"]
            .versions
            .iter()
            .any(|v| v.id == "mine"));
        // The shipped games are there too.
        assert_eq!(catalog.games["titanfall2"].app_id, 1237970);
    }

    #[test]
    fn a_resolved_version_is_a_valid_steam_section() {
        let got = game().resolve("s2", Some(&[10, 11])).unwrap();
        let config: respak_host::Config = toml::from_str(&format!(
            "out = \"o\"\n[steam]\napp = {}\ndepots = {:?}\n",
            got.app, got.depots
        ))
        .unwrap();
        let settings =
            respak_host::Settings::resolve(&respak_host::Overrides::default(), config).unwrap();
        #[cfg(feature = "steam")]
        match settings.source {
            respak_host::Source::Steam(steam) => {
                assert_eq!(steam.depots.len(), 2);
                assert_eq!(steam.depots[0].manifest, Some(200));
            }
            other => panic!("{other:?}"),
        }
        #[cfg(not(feature = "steam"))]
        let _ = settings;
    }
}
