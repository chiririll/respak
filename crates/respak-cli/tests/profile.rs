//! The command line: a profile in the place of the command, and flags on either side of it.

use respak_cli::{Cli, Command};

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_args(["respak"].iter().chain(args))
}

#[test]
fn a_word_before_the_command_is_a_profile() {
    let cli = parse(&["apex", "convert", "a.png", "--to", "{}"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("apex"));
    assert!(matches!(cli.command, Command::Convert(_)));
    let cli = parse(&["--out", "o", "apex", "formats", "--root", "r"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("apex"));
    assert_eq!((cli.out, cli.root), (Some("o".into()), Some("r".into())));
}

#[test]
fn no_profile_without_a_word() {
    let cli = parse(&["formats"]).unwrap();
    assert!(cli.profile.is_none() && matches!(cli.command, Command::Formats));
}

#[test]
fn the_flag_names_a_profile_that_is_called_like_a_command() {
    let cli = parse(&["-p", "convert", "formats"]).unwrap();
    assert_eq!(cli.profile.as_deref(), Some("convert"));
    assert!(matches!(cli.command, Command::Formats));
}

#[test]
fn flags_may_follow_the_command() {
    let cli = parse(&["list", "a", "--root", "r", "--out", "o", "--search", "s"]).unwrap();
    assert_eq!(cli.root, Some("r".into()));
    assert_eq!(cli.out, Some("o".into()));
    assert_eq!(cli.search, ["s"]);
}

#[test]
fn a_bad_profile_is_refused() {
    // A typo after a profile, a profile twice, a name that is a path, no command at all.
    assert!(parse(&["apex", "covnert"]).is_err());
    assert!(parse(&["apex", "-p", "b", "formats"]).is_err());
    assert!(parse(&["../evil", "formats"]).is_err());
    assert!(parse(&["-p", "a/b", "formats"]).is_err());
    assert!(parse(&["--config", "f", "--no-config", "formats"]).is_err());
}

#[cfg(feature = "steam")]
#[test]
fn steam_is_a_command_not_a_profile() {
    let cli = parse(&["steam", "login", "--qr"]).unwrap();
    assert!(cli.profile.is_none() && matches!(cli.command, Command::Steam { .. }));
}
