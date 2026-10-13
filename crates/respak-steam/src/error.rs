//! Errors of the Steam login.

/// What can go wrong while logging in to Steam.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No usable stored token: the user has to log in again.
    #[error("not logged in to Steam (run `respak steam login`): {0}")]
    NeedLogin(String),
    /// Steam (or the connection to it) failed.
    #[error("steam: {0}")]
    Steam(#[from] steamroom::Error),
    /// The account file could not be read or written.
    #[error("account file {path}: {message}")]
    Account {
        /// The file.
        path: String,
        /// What happened.
        message: String,
    },
    /// Steam answered, but not the way the login needs.
    #[error("unexpected reply from Steam: {0}")]
    Protocol(String),
    /// A chunk, key or manifest could not be fetched, decoded or checked.
    #[error("{0}")]
    Fetch(String),
    /// Reading from the terminal failed.
    #[error("terminal: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of Steam operations.
pub type Result<T> = std::result::Result<T, Error>;
