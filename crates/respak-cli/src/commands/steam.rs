//! `respak steam`.

use std::io::Write;
use std::process::ExitCode;

use clap::Subcommand;

/// `respak steam <command>`.
#[derive(Debug, Subcommand)]
pub enum SteamCommand {
    /// Log in to Steam and store a refresh token in `.respak/account.json`.
    Login {
        /// Scan a QR code with the Steam mobile app.
        #[arg(long, conflicts_with = "user", required_unless_present = "user")]
        qr: bool,
        /// Log in with this account name (password prompt, then Steam Guard).
        #[arg(long)]
        user: Option<String>,
    },
    /// Delete the stored login.
    Logout,
}

/// Runs a `steam` command; the state directory is `.respak` in the working directory.
pub async fn run_steam(command: SteamCommand, out: &mut impl Write) -> anyhow::Result<ExitCode> {
    use respak_steam::AccountStore;
    let store = AccountStore::new(".respak");
    match command {
        SteamCommand::Login { qr, user } => {
            let account = match user {
                Some(name) if !qr => respak_steam::login_user(&store, &name).await?,
                _ => respak_steam::login_qr(&store, out).await?,
            };
            writeln!(
                out,
                "Logged in as {} (steam id {}). Token stored in {}.",
                account.account_name,
                account.steam_id,
                store.path().display()
            )?;
        }
        SteamCommand::Logout => {
            if store.delete()? {
                writeln!(out, "Logged out: {} deleted.", store.path().display())?;
            } else {
                writeln!(out, "Not logged in.")?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
