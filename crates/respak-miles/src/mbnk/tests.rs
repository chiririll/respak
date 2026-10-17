use std::sync::Arc;

use respak_core::volume::VolumeSource;
use respak_core::ErrorKind;

use super::fixtures::*;
use super::source::MbnkSource;
use super::{Bank, BankLayout};

/// `sourceCount`/`localisedSourceCount` for one plain source plus one localised across all
/// languages (`1 + 1 * (languages - 1)` entries; the localised source's `english` entry is
/// counted in `unlocalised`).
fn mixed(version: i32) -> Vec<u8> {
    let layout = BankLayout::for_version(version).unwrap();
    let mut sources = vec![Fx::new("music/theme")];
    for (i, _) in layout.languages.iter().enumerate() {
        let mut f = Fx::new("vo/hello");
        f.language = i as u16;
        f.patch = (i % 2) as u16;
        sources.push(f);
    }
    bank(version, 0xBEEF, "general", &sources, 2, 1)
}

#[test]
fn every_layout_gives_the_same_sources() {
    for version in [13, 28, 38, 39, 45, 46, 48, 49] {
        let b = Bank::parse(&mixed(version)).unwrap_or_else(|e| panic!("v{version}: {e}"));
        let langs = b.layout().languages;
        assert_eq!(b.sources().len(), 1 + langs.len(), "v{version}");
        assert_eq!((b.stem(), b.build_tag()), ("general", 0xBEEF), "v{version}");
        let theme = &b.sources()[0];
        assert_eq!(
            (
                theme.name.as_str(),
                theme.language.as_deref(),
                theme.sample_rate,
                theme.sample_count
            ),
            ("music/theme", None, 48000, 1000),
            "v{version}"
        );
        assert_eq!(theme.header_range, 0x100..0x118, "v{version}");
        let last = b.sources().last().unwrap();
        assert_eq!(
            last.language.as_deref(),
            langs.last().copied(),
            "v{version}"
        );
        assert_eq!(
            Bank::volume_name(last),
            format!("vo/hello.{}.asrc", langs.last().unwrap()),
            "v{version}"
        );
        assert_eq!(last.patch_idx, ((langs.len() - 1) % 2) as u16, "v{version}");
    }
}

#[test]
fn the_language_tables_have_the_documented_sizes() {
    let n = |v| BankLayout::for_version(v).unwrap().languages.len();
    assert_eq!((n(13), n(28), n(45), n(49)), (11, 9, 10, 10));
    for v in [11, 29, 31, 35, 37, 41, 47, 50, -1] {
        assert!(BankLayout::for_version(v).is_none(), "v{v}");
    }
}

#[test]
fn the_entry_count_follows_the_header_counts() {
    // 2 plain + 1 localised * (10 - 1) = 11 entries in a v49 bank
    let sources: Vec<Fx> = (0..11).map(|_| Fx::new("a")).collect();
    let b = Bank::parse(&bank(49, 1, "general", &sources, 2, 1)).unwrap();
    assert_eq!(b.sources().len(), 11);
}

#[test]
fn an_unknown_version_and_bad_magic() {
    let mut bytes = bank(49, 1, "general", &[Fx::new("a")], 1, 0);
    bytes[4..8].copy_from_slice(&47i32.to_le_bytes());
    assert!(matches!(
        Bank::parse(&bytes).unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
    bytes[0] = b'X';
    assert!(matches!(
        Bank::parse(&bytes).unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));
}

#[test]
fn damaged_banks_are_corrupt_never_a_panic() {
    let good = bank(49, 1, "general", &[Fx::new("a")], 1, 0);
    // an entry array past the end of the file
    let mut b = good.clone();
    b[0x38..0x3C].copy_from_slice(&1_000_000u32.to_le_bytes());
    assert!(matches!(
        Bank::parse(&b).unwrap_err().kind(),
        ErrorKind::Corrupt { .. }
    ));
    // counts that overflow
    let mut b = good.clone();
    b[0x3C..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Bank::parse(&b).is_err());
    // a string offset out of range
    let mut b = good.clone();
    b[0x70..0x78].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Bank::parse(&b).is_err());
    // truncations at every length
    for len in 0..good.len() {
        let _ = Bank::parse(&good[..len]);
    }
    assert!(Bank::parse(&good[..0x20]).is_err());
}

#[test]
fn duplicate_names_get_distinct_paths() {
    let sources = [Fx::new("same"), Fx::new("same")];
    let b = Bank::parse(&bank(49, 1, "general", &sources, 2, 0)).unwrap();
    assert!(b.find("same.asrc").is_some());
    assert!(b.find("same~1.asrc").is_some());
}

#[test]
fn the_volume_lists_files_and_folders() {
    let b = Bank::parse(&mixed(49)).unwrap();
    let root = b.list("");
    let names: Vec<_> = root.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
    assert_eq!(names, [("music", true), ("vo", true)]);
    let vo = b.list("vo");
    assert_eq!(vo.len(), 10);
    assert!(vo
        .iter()
        .all(|e| !e.is_dir && e.name.starts_with("hello.") && e.len > 0));
    assert!(b.is_dir("music") && !b.is_dir("music/theme.asrc") && !b.is_dir("nope"));
    assert!(b.find("music/theme.asrc").is_some() && b.find("vo/hello.korean.asrc").is_some());
}

#[tokio::test]
async fn volume_stat_read_and_not_found() {
    let src = MbnkSource(Arc::new(Bank::parse(&mixed(49)).unwrap()));
    assert!(src.stat("").await.unwrap().is_dir);
    assert!(src.stat("vo").await.unwrap().is_dir);
    let stat = src.stat("music/theme.asrc").await.unwrap();
    assert!(!stat.is_dir);
    let all = src.read("music/theme.asrc", 0..stat.len).await.unwrap();
    assert_eq!(all.len() as u64, stat.len);
    assert!(std::str::from_utf8(&all)
        .unwrap()
        .starts_with("name=music/theme\n"));
    assert_eq!(src.read("music/theme.asrc", 5..9).await.unwrap().len(), 4);
    assert!(matches!(
        src.stat("music/missing.asrc").await.unwrap_err().kind(),
        ErrorKind::NotFound { .. }
    ));
    assert!(src.read("nope", 0..1).await.is_err());
}
