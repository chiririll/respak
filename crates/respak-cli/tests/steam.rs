#![cfg(feature = "steam")]
//! `steam` command line: parsing only (the live login needs a Steam account).

use clap::Parser;
use respak_cli::{Cli, Command, SteamCommand};

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(["respak", "steam"].iter().chain(args))
}

#[test]
fn login_qr_and_user_parse_without_root_or_out() {
    let Command::Steam {
        command: SteamCommand::Login { qr, user },
    } = parse(&["login", "--qr"]).unwrap().command
    else {
        panic!()
    };
    assert!(qr && user.is_none());
    let Command::Steam {
        command: SteamCommand::Login { qr, user },
    } = parse(&["login", "--user", "gordon"]).unwrap().command
    else {
        panic!()
    };
    assert!(!qr && user.as_deref() == Some("gordon"));
    assert!(matches!(
        parse(&["logout"]).unwrap().command,
        Command::Steam {
            command: SteamCommand::Logout
        }
    ));
}

#[test]
fn login_needs_exactly_one_method() {
    assert!(parse(&["login"]).is_err());
    assert!(parse(&["login", "--qr", "--user", "x"]).is_err());
}

#[test]
fn steam_input_flags() {
    let cli = Cli::try_parse_from([
        "respak",
        "--steam",
        "1172470",
        "--depot",
        "1172471=7843921",
        "--depot",
        "1172472",
        "--branch",
        "beta",
        "--steam-cache-size",
        "500MiB",
        "--out",
        "out",
        "formats",
    ])
    .unwrap();
    assert_eq!(cli.steam, Some(1172470));
    assert_eq!(cli.branch, "beta");
    assert_eq!(cli.steam_cache_size, Some(500 << 20));
    assert_eq!(
        cli.depot,
        [
            respak_steam::DepotSpec {
                depot: 1172471,
                manifest: Some(7843921)
            },
            respak_steam::DepotSpec {
                depot: 1172472,
                manifest: None
            }
        ]
    );
}

#[test]
fn steam_and_root_exclude_each_other_and_steam_needs_a_depot() {
    let parse =
        |args: &[&str]| Cli::try_parse_from(["respak", "--out", "o", "formats"].iter().chain(args));
    assert!(parse(&["--steam", "1", "--depot", "2", "--root", "r"]).is_err());
    assert!(parse(&["--steam", "1"]).is_err());
    assert!(parse(&["--depot", "2"]).is_err());
    assert!(parse(&["--steam", "1", "--depot", "x"]).is_err());
}

#[test]
fn sizes() {
    use respak_cli::parse_size;
    assert_eq!(parse_size("1024"), Ok(1024));
    assert_eq!(parse_size("8GiB"), Ok(8 << 30));
    assert_eq!(parse_size("2gb"), Ok(2_000_000_000));
    assert_eq!(parse_size("500 MiB"), Ok(500 << 20));
    assert!(parse_size("GiB").is_err() && parse_size("5 parsecs").is_err());
}
