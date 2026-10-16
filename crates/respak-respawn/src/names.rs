//! Guessing asset names: strings found in arbitrary files are hashed and kept when the guid is one
//! we are looking for. A guid cannot be reversed, so this is the only way to name an asset that
//! stores no name; a match is verified by the hash, so a wrong string can only miss.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::guid::string_to_guid;

/// Shortest string that is hashed.
const MIN_LEN: usize = 4;

/// Longest string that is hashed.
const MAX_LEN: usize = 260;

/// A printable run longer than this is not held back for the next chunk.
const MAX_PENDING: usize = 64 << 10;

/// The 16-digit hex guids in `text` (`0x...`, optionally followed by an extension such as
/// `.txtr`); anything else in the text is ignored.
pub fn parse_guids(text: &str) -> HashSet<u64> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|word| {
            let digits = word
                .strip_prefix("0x")
                .or_else(|| word.strip_prefix("0X"))
                .unwrap_or(word);
            (digits.len() == 16 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| u64::from_str_radix(digits, 16).ok())
                .flatten()
        })
        .collect()
}

/// Hashes candidate strings and keeps those whose guid is wanted.
///
/// The wanted guids are either given up front, or are the assets reported with [`asset`](Self::asset)
/// that have no name of their own. In the second case the guids are not known until the end, so
/// every string is kept by hash and matched in [`finish`](Self::finish).
pub struct Finder {
    /// `None`: the unnamed assets seen.
    wanted: Option<HashSet<u64>>,
    named: HashSet<u64>,
    unnamed: HashSet<u64>,
    /// Every string by hash, only while `wanted` is open.
    hashed: HashMap<u64, String>,
    prefixes: Vec<String>,
    suffixes: Vec<String>,
    found: BTreeMap<u64, String>,
    buf: String,
    /// The printable run at the end of the last chunk, which may go on in the next.
    pending: Vec<u8>,
}

impl Finder {
    /// Looks for `wanted`, or, if `None`, for the assets reported with [`asset`](Self::asset) that
    /// have no name. Each string is also tried with every prefix and every suffix added (and with
    /// both); the string as it is is always tried.
    pub fn new(wanted: Option<HashSet<u64>>, prefixes: Vec<String>, suffixes: Vec<String>) -> Self {
        Self {
            wanted,
            named: HashSet::new(),
            unnamed: HashSet::new(),
            hashed: HashMap::new(),
            prefixes,
            suffixes,
            found: BTreeMap::new(),
            buf: String::new(),
            pending: Vec::new(),
        }
    }

    /// An asset met on the way: `named` is whether it stores its own name.
    pub fn asset(&mut self, guid: u64, named: bool) {
        if named {
            self.named.insert(guid);
        } else {
            self.unnamed.insert(guid);
        }
    }

    /// How many guids are looked for (assets seen so far that have no name, if none were given).
    pub fn wanted(&self) -> usize {
        match &self.wanted {
            Some(w) => w.len(),
            None => self.unnamed.difference(&self.named).count(),
        }
    }

    /// Whether the search can stop: the wanted guids were given and all have a name.
    pub fn is_complete(&self) -> bool {
        self.wanted
            .as_ref()
            .is_some_and(|w| w.iter().all(|g| self.found.contains_key(g)))
    }

    /// Call when all strings are in: matches the kept strings against the unnamed assets seen.
    pub fn finish(&mut self) {
        if self.wanted.is_none() {
            let wanted: HashSet<u64> = self.unnamed.difference(&self.named).copied().collect();
            for (guid, name) in self.hashed.drain() {
                if wanted.contains(&guid) {
                    self.found.insert(guid, name);
                }
            }
            self.wanted = Some(wanted);
        }
    }

    /// What was found, by guid (complete after [`finish`](Self::finish)).
    pub fn found(&self) -> &BTreeMap<u64, String> {
        &self.found
    }

    /// Tries `text` and each token of it (split at quotes, spaces and brackets, so a string cut
    /// out of a binary or a script yields the paths in it).
    pub fn feed(&mut self, text: &str) {
        self.check(text);
        let is_break = |c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\''
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '='
                        | '|'
                        | '*'
                        | '?'
                )
        };
        for token in text.split(is_break) {
            if token.len() != text.len() {
                self.check(token);
            }
        }
    }

    /// Feeds the next `chunk` of a file: every run of printable ASCII in it (also across chunk
    /// boundaries). Call with an empty chunk at the end of the file to flush the last run.
    pub fn feed_bytes(&mut self, chunk: &[u8]) {
        let printable = |b: &u8| (0x20..0x7F).contains(b);
        self.pending.extend_from_slice(chunk);
        let end = if chunk.is_empty() || self.pending.len() > MAX_PENDING {
            self.pending.len()
        } else {
            self.pending
                .iter()
                .rposition(|b| !printable(b))
                .map_or(0, |i| i + 1)
        };
        let done: Vec<u8> = self.pending.drain(..end).collect();
        for run in done.split(|b| !printable(b)).filter(|r| r.len() >= MIN_LEN) {
            self.feed(std::str::from_utf8(run).unwrap_or_default());
        }
    }

    fn check(&mut self, text: &str) {
        let text =
            text.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '/' | '\\' | '`'));
        if text.len() < MIN_LEN || text.len() > MAX_LEN {
            return;
        }
        for prefix in std::iter::once("").chain(self.prefixes.iter().map(String::as_str)) {
            for suffix in std::iter::once("").chain(self.suffixes.iter().map(String::as_str)) {
                self.buf.clear();
                self.buf.push_str(prefix);
                self.buf.push_str(text);
                self.buf.push_str(suffix);
                let guid = string_to_guid(&self.buf);
                match &self.wanted {
                    Some(wanted) if wanted.contains(&guid) => {
                        self.found.entry(guid).or_insert_with(|| self.buf.clone());
                    }
                    Some(_) => {}
                    None => {
                        self.hashed.entry(guid).or_insert_with(|| self.buf.clone());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_guids_with_extensions_and_ignores_the_rest() {
        let guids =
            parse_guids("0x00004E54F8C2A211.efct\n00006FD412AF3101\nnot 0x1234 0xZZ04E54F8C2A211");
        assert_eq!(guids, HashSet::from([0x4E54F8C2A211, 0x6FD412AF3101]));
    }

    #[test]
    fn finds_a_name_across_chunks_of_binary_data() {
        let guid = string_to_guid("scripts/vscripts/init.nut");
        let mut finder = Finder::new(Some(HashSet::from([guid])), Vec::new(), Vec::new());
        finder.feed_bytes(b"\0\x01ab scripts/vsc");
        assert!(finder.found().is_empty());
        finder.feed_bytes(b"ripts/init.nut\0\xff");
        finder.feed_bytes(b"");
        assert_eq!(
            finder.found().get(&guid).map(String::as_str),
            Some("scripts/vscripts/init.nut")
        );
        assert!(finder.is_complete());
    }

    #[test]
    fn finds_names_in_strings_and_with_affixes() {
        let direct = string_to_guid("datatable/weapons.rpak");
        let affixed = string_to_guid("ui_image/menu/logo.rpak");
        let mut finder = Finder::new(
            Some(HashSet::from([direct, affixed, 1])),
            vec!["ui_image/".into()],
            vec![".rpak".into()],
        );
        finder.feed("load(\"datatable/weapons.rpak\", 1)");
        finder.feed(" menu/logo ");
        assert_eq!(
            finder.found().get(&direct).map(String::as_str),
            Some("datatable/weapons.rpak")
        );
        assert_eq!(
            finder.found().get(&affixed).map(String::as_str),
            Some("ui_image/menu/logo.rpak")
        );
        assert_eq!(finder.found().len(), 2);
    }

    #[test]
    fn unnamed_assets_seen_are_the_wanted_ones_when_none_are_given() {
        let (unnamed, named) = (
            string_to_guid("datatable/a.rpak"),
            string_to_guid("datatable/b.rpak"),
        );
        let mut finder = Finder::new(None, Vec::new(), Vec::new());
        // The strings may come before or after the assets that they name.
        finder.feed("datatable/a.rpak datatable/b.rpak");
        finder.asset(unnamed, false);
        finder.asset(named, true);
        assert_eq!(finder.wanted(), 1);
        assert!(!finder.is_complete());
        finder.finish();
        assert_eq!(
            finder.found().keys().copied().collect::<Vec<_>>(),
            vec![unnamed]
        );
    }
}
