//! The merged file tree of the `--depot` list: `/` separators, original case, case-insensitive
//! lookups, and on a collision the depot later in the list wins (STEAM.md §3).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::chunks::ChunkRef;

/// A file in one depot's manifest (the manifest's `\` already turned into `/`).
#[derive(Debug, Clone)]
pub struct DepotFile {
    /// Path with the original case.
    pub path: String,
    /// Size from the manifest.
    pub size: u64,
    /// Chunks sorted by offset.
    pub chunks: Arc<[ChunkRef]>,
}

/// A file as the tree serves it: which depot and manifest it comes from.
#[derive(Debug, Clone)]
pub struct TreeFile {
    /// The depot id.
    pub depot: u32,
    /// The manifest id (for errors).
    pub manifest: u64,
    /// The file.
    pub file: DepotFile,
}

/// One child of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Name with the original case.
    pub name: String,
    /// Directory or file.
    pub is_dir: bool,
    /// Size (0 for directories).
    pub size: u64,
}

/// Lowercased lookup key of a path: no leading, trailing or doubled slashes.
fn key(path: &str) -> String {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/").to_lowercase()
}

/// The merged tree.
#[derive(Debug, Default)]
pub struct DepotTree {
    files: HashMap<String, TreeFile>,
    /// Directory key -> (child key -> display name); the root is `""`. A child that is also a
    /// directory is found by `dirs`.
    children: HashMap<String, BTreeMap<String, String>>,
    dirs: HashMap<String, ()>,
}

impl DepotTree {
    /// Adds one depot's files; call in `--depot` order, later calls win collisions.
    pub fn add(&mut self, depot: u32, manifest: u64, files: Vec<DepotFile>) {
        for file in files {
            let path = file.path.replace('\\', "/");
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }
            let mut dir = String::new();
            for (i, part) in parts.iter().enumerate() {
                let child = if dir.is_empty() { part.to_lowercase() } else { format!("{dir}/{}", part.to_lowercase()) };
                self.children.entry(dir.clone()).or_default().entry(child.clone()).or_insert_with(|| part.to_string());
                if i + 1 < parts.len() {
                    self.dirs.insert(child.clone(), ());
                }
                dir = child;
            }
            self.files.insert(dir, TreeFile { depot, manifest, file: DepotFile { path, ..file } });
        }
    }

    /// The file at `path` (case-insensitive), if it is one.
    pub fn file(&self, path: &str) -> Option<&TreeFile> {
        self.files.get(&key(path))
    }

    /// Whether `path` is a directory (the empty path is the root).
    pub fn is_dir(&self, path: &str) -> bool {
        let k = key(path);
        k.is_empty() || (self.dirs.contains_key(&k) && !self.files.contains_key(&k))
    }

    /// Children of the directory `dir`, sorted by name; `None` if it is not a directory.
    pub fn list(&self, dir: &str) -> Option<Vec<TreeEntry>> {
        if !self.is_dir(dir) {
            return None;
        }
        let kids = self.children.get(&key(dir))?;
        let mut out: Vec<TreeEntry> = kids
            .iter()
            .map(|(k, name)| match self.files.get(k) {
                Some(f) => TreeEntry { name: name.clone(), is_dir: false, size: f.file.size },
                None => TreeEntry { name: name.clone(), is_dir: true, size: 0 },
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Some(out)
    }

    /// Whether the tree holds no files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, size: u64) -> DepotFile {
        DepotFile { path: path.into(), size, chunks: Arc::from(vec![]) }
    }

    #[test]
    fn later_depot_wins_a_collision() {
        let mut t = DepotTree::default();
        t.add(1, 10, vec![f("a/x.bin", 1), f("only1.bin", 5)]);
        t.add(2, 20, vec![f("a/x.bin", 2)]);
        let x = t.file("a/x.bin").unwrap();
        assert_eq!((x.depot, x.manifest, x.file.size), (2, 20, 2));
        assert_eq!(t.file("only1.bin").unwrap().depot, 1);
    }

    #[test]
    fn lookup_is_case_insensitive_and_keeps_case() {
        let mut t = DepotTree::default();
        t.add(1, 1, vec![f("Paks\\Win64\\Common.rpak", 9)]);
        assert!(t.file("paks/win64/common.rpak").is_some());
        assert!(t.file("PAKS/WIN64/COMMON.RPAK").is_some());
        assert_eq!(t.file("paks/win64/common.rpak").unwrap().file.path, "Paks/Win64/Common.rpak");
        assert!(t.file("paks/win64").is_none());
        assert!(t.file("nope").is_none());
    }

    #[test]
    fn case_variants_from_two_depots_collide() {
        let mut t = DepotTree::default();
        t.add(1, 1, vec![f("Paks/A.rpak", 1)]);
        t.add(2, 2, vec![f("paks/a.rpak", 2)]);
        assert_eq!(t.file("PAKS/A.RPAK").unwrap().depot, 2);
        assert_eq!(t.list("paks").unwrap().len(), 1);
    }

    #[test]
    fn list_gives_sorted_children_with_directories() {
        let mut t = DepotTree::default();
        t.add(1, 1, vec![f("b.txt", 3), f("Paks/Win64/c.rpak", 7), f("Paks/a.rpak", 5)]);
        assert_eq!(
            t.list("").unwrap(),
            [
                TreeEntry { name: "Paks".into(), is_dir: true, size: 0 },
                TreeEntry { name: "b.txt".into(), is_dir: false, size: 3 },
            ]
        );
        assert_eq!(
            t.list("paks").unwrap(),
            [
                TreeEntry { name: "Win64".into(), is_dir: true, size: 0 },
                TreeEntry { name: "a.rpak".into(), is_dir: false, size: 5 },
            ]
        );
        assert!(t.is_dir("") && t.is_dir("PAKS/win64") && !t.is_dir("b.txt") && !t.is_dir("zzz"));
        assert!(t.list("b.txt").is_none());
    }
}
