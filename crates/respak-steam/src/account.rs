//! The stored login: `.respak/account.json` and the expiry of its refresh token.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A refresh token this close to its expiry (seconds) is renewed before use.
pub const RENEW_WINDOW_SECS: u64 = 24 * 3600;

/// What `account.json` holds. The password is never stored.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Account name; empty for a QR login until Steam reports it.
    pub account_name: String,
    /// The 64-bit Steam ID.
    pub steam_id: u64,
    /// The refresh token (a JWT).
    pub refresh_token: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("account_name", &self.account_name).field("steam_id", &self.steam_id).finish_non_exhaustive()
    }
}

/// How a refresh token stands against the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// Valid for longer than the renewal window.
    Fresh,
    /// Valid, but should be renewed.
    Near,
    /// Past `exp`.
    Expired,
}

/// The `.respak/` directory holding the account file.
#[derive(Debug, Clone)]
pub struct AccountStore {
    dir: PathBuf,
}

impl AccountStore {
    /// A store in `dir` (usually `.respak`); the directory is created on save.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { dir: dir.as_ref().to_path_buf() }
    }

    /// The account file.
    pub fn path(&self) -> PathBuf {
        self.dir.join("account.json")
    }

    fn error(&self, message: impl ToString) -> Error {
        Error::Account { path: self.path().display().to_string(), message: message.to_string() }
    }

    /// Reads the stored account. A missing file is `NeedLogin`, a malformed one an `Account` error.
    pub fn load(&self) -> Result<Account> {
        let text = std::fs::read_to_string(self.path()).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NeedLogin("no account.json".into()),
            _ => self.error(e),
        })?;
        serde_json::from_str(&text).map_err(|e| self.error(format!("malformed ({e}); run `respak steam login` again")))
    }

    /// Writes the account (creating the directory).
    pub fn save(&self, account: &Account) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| self.error(e))?;
        let text = serde_json::to_string_pretty(account).map_err(|e| self.error(e))?;
        std::fs::write(self.path(), text).map_err(|e| self.error(e))
    }

    /// Deletes the account file; returns whether there was one.
    pub fn delete(&self) -> Result<bool> {
        match std::fs::remove_file(self.path()) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(self.error(e)),
        }
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The claims of a JWT payload that respak reads.
#[derive(Debug, Deserialize)]
pub struct Claims {
    /// Expiry, seconds since the epoch.
    pub exp: Option<u64>,
    /// Subject: the decimal Steam ID.
    pub sub: Option<String>,
}

/// Decodes the payload of a JWT (no signature check: Steam checks it, we only read `exp` and `sub`).
pub fn claims(jwt: &str) -> Result<Claims> {
    let payload = jwt.split('.').nth(1).ok_or_else(|| Error::Protocol("the token is not a JWT".into()))?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).map_err(|e| Error::Protocol(format!("token payload: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Protocol(format!("token payload: {e}")))
}

/// Classifies the token against `now`. A token without `exp` counts as fresh (Steam is the judge).
pub fn expiry(jwt: &str, now: u64) -> Result<Expiry> {
    Ok(match claims(jwt)?.exp {
        None => Expiry::Fresh,
        Some(exp) if exp <= now => Expiry::Expired,
        Some(exp) if exp - now <= RENEW_WINDOW_SECS => Expiry::Near,
        Some(_) => Expiry::Fresh,
    })
}

/// The Steam ID in the token's `sub`.
pub fn steam_id_of(jwt: &str) -> Result<u64> {
    claims(jwt)?.sub.and_then(|s| s.parse().ok()).ok_or_else(|| Error::Protocol("the token has no steam id".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &str) -> String {
        format!("{}.{}.sig", URL_SAFE_NO_PAD.encode("{}"), URL_SAFE_NO_PAD.encode(payload))
    }

    fn account() -> Account {
        Account { account_name: "gordon".into(), steam_id: 76561198000000000, refresh_token: "a.b.c".into() }
    }

    #[test]
    fn round_trip_and_logout() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path().join(".respak"));
        store.save(&account()).unwrap();
        assert_eq!(store.load().unwrap(), account());
        assert!(store.delete().unwrap());
        assert!(!store.path().exists());
        assert!(!store.delete().unwrap());
    }

    #[test]
    fn missing_file_needs_login() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(AccountStore::new(dir.path()).load(), Err(Error::NeedLogin(_))));
    }

    #[test]
    fn malformed_file_is_an_account_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path());
        std::fs::write(store.path(), "{ nope").unwrap();
        assert!(matches!(store.load(), Err(Error::Account { .. })));
    }

    #[test]
    fn expiry_past_near_far() {
        let now = 1_000_000;
        assert_eq!(expiry(&jwt(r#"{"exp":999999}"#), now).unwrap(), Expiry::Expired);
        assert_eq!(expiry(&jwt(r#"{"exp":1000000}"#), now).unwrap(), Expiry::Expired);
        assert_eq!(expiry(&jwt(&format!(r#"{{"exp":{}}}"#, now + 3600)), now).unwrap(), Expiry::Near);
        assert_eq!(expiry(&jwt(&format!(r#"{{"exp":{}}}"#, now + 30 * 24 * 3600)), now).unwrap(), Expiry::Fresh);
        assert_eq!(expiry(&jwt("{}"), now).unwrap(), Expiry::Fresh);
        assert!(expiry("garbage", now).is_err());
    }

    #[test]
    fn steam_id_from_sub() {
        assert_eq!(steam_id_of(&jwt(r#"{"sub":"76561198000000000"}"#)).unwrap(), 76561198000000000);
        assert!(steam_id_of(&jwt("{}")).is_err());
    }
}
