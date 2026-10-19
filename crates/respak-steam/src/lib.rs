//! Steam access for respak: login, the stored token, logon (docs/design/steam.md). Host only; the only crate with Steam code.

mod account;
mod cache;
mod cdn;
mod chunks;
mod depot;
mod error;
mod link;
mod login;
mod qr;
mod session;
mod store;
mod tree;

pub use account::{
    claims, expiry, now_secs, steam_id_of, Account, AccountStore, Claims, Expiry, RENEW_WINDOW_SECS,
};
pub use error::{Error, Result};
pub use login::{
    login_qr, login_qr_with, login_user, login_user_with, CodeFuture, GuardEvent, GuardKind,
};
pub use qr::{
    matrix as qr_matrix, render as render_qr, run as poll_qr, PollReply, Poller, QrMatrix, QrTokens,
};
pub use session::{connect, device_name, logon};

pub use cache::{ChunkCache, DEFAULT_CAP};
pub use cdn::CdnSource;
pub use chunks::{assemble, covering, ChunkRef};
pub use depot::DepotSource;
pub use link::{open, DepotSpec, Resolver, SteamInput, SteamOptions};
pub use store::{ChunkSource, ChunkStore, Stats};
pub use tree::{DepotFile, DepotTree, TreeEntry, TreeFile};
