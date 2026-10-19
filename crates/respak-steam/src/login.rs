//! `steam login --qr` and `steam login --user NAME`: the two ways to obtain a refresh token.

use std::io::Write;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine};
use prost::Message;
use steamroom::{
    auth::{AuthClientId, GuardType},
    client::{Ready, SteamClient},
    error::ConnectionError,
    generated::{
        CAuthenticationBeginAuthSessionViaCredentialsRequest,
        CAuthenticationBeginAuthSessionViaQrRequest, CAuthenticationPollAuthSessionStatusRequest,
        CAuthenticationPollAuthSessionStatusResponse,
    },
    types::SteamId,
};

use crate::account::{self, Account, AccountStore};
use crate::error::{Error, Result};
use crate::qr::{self, PollReply, Poller};
use crate::session;

/// `EAuthTokenPlatformType::SteamClient`; Steam rejects the logon with other values left unset.
const PLATFORM_STEAM_CLIENT: i32 = 1;
/// `ESessionPersistence::Persistent`.
const PERSISTENT: i32 = 1;

fn save(store: &AccountStore, account_name: String, refresh_token: String) -> Result<Account> {
    let account = Account {
        account_name,
        steam_id: account::steam_id_of(&refresh_token)?,
        refresh_token,
    };
    store.save(&account)?;
    Ok(account)
}

struct QrPoller<'a> {
    client: &'a SteamClient<Ready>,
    request_id: Vec<u8>,
}

impl Poller for QrPoller<'_> {
    async fn poll(&mut self, client_id: u64) -> Result<PollReply> {
        let request = CAuthenticationPollAuthSessionStatusRequest {
            client_id: Some(client_id),
            request_id: Some(self.request_id.clone()),
            ..Default::default()
        };
        let reply = self
            .client
            .call_service_method_non_authed(
                "Authentication.PollAuthSessionStatus#1",
                &request.encode_to_vec(),
            )
            .await?;
        let r: CAuthenticationPollAuthSessionStatusResponse = reply
            .decode()
            .map_err(|e| Error::Protocol(format!("poll reply: {e}")))?;
        Ok(PollReply {
            new_client_id: r.new_client_id,
            new_challenge_url: r.new_challenge_url,
            refresh_token: r.refresh_token,
            account_name: r.account_name,
        })
    }
}

/// The QR flow up to the token: `show` gets the challenge URL, and again each time Steam rotates it.
async fn qr_tokens(show: impl FnMut(&str)) -> Result<qr::QrTokens> {
    let client = session::connect().await?;
    let request = CAuthenticationBeginAuthSessionViaQrRequest {
        device_friendly_name: Some(session::device_name()),
        platform_type: Some(PLATFORM_STEAM_CLIENT),
        ..Default::default()
    };
    let begun = client.begin_auth_session_via_qr(request).await?;
    let (Some(client_id), Some(request_id), Some(url)) =
        (begun.client_id, begun.request_id, begun.challenge_url)
    else {
        return Err(Error::Protocol(
            "Steam did not return a QR challenge".into(),
        ));
    };
    let interval = begun
        .poll_interval
        .map(|i| i.as_duration())
        .unwrap_or(Duration::from_millis(2500));
    let mut poller = QrPoller {
        client: &client,
        request_id,
    };
    qr::run(&mut poller, client_id.raw(), &url, interval, show).await
}

/// QR login for a host with a screen of its own: `show` is called with the challenge URL (to turn
/// into a QR code), and again each time Steam rotates it. Saves the account once it is approved.
pub async fn login_qr_with(store: &AccountStore, show: impl FnMut(&str)) -> Result<Account> {
    let tokens = qr_tokens(show).await?;
    save(store, tokens.account_name, tokens.refresh_token)
}

/// QR login: prints the code, redraws it when Steam rotates it, saves the account once approved.
pub async fn login_qr(store: &AccountStore, out: &mut impl Write) -> Result<Account> {
    writeln!(
        out,
        "Scan this code with the Steam mobile app (Steam Guard > Sign in with QR):"
    )?;
    let mut render_error = None;
    let tokens = qr_tokens(|url| match qr::render(url) {
        Ok(code) => {
            let _ = writeln!(out, "\n{code}\n{url}");
        }
        Err(e) => render_error = Some(e),
    })
    .await?;
    if let Some(e) = render_error {
        return Err(e);
    }
    save(store, tokens.account_name, tokens.refresh_token)
}

fn prompt(text: &str) -> std::io::Result<String> {
    eprint!("{text}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// What Steam Guard asked for, as it happens in a credential login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardEvent {
    /// Steam wants a code (`kind`), a confirmation in the mobile app (`confirmable`), or both.
    Needed {
        /// The kind of code; `None` when only a confirmation in the app will do.
        kind: Option<GuardKind>,
        /// Whether approving the login in the Steam mobile app also works.
        confirmable: bool,
    },
    /// Steam rejected the code; another is asked for.
    Rejected(String),
}

/// The kind of code Steam Guard wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    /// A code Steam emailed.
    Email,
    /// A code of the authenticator in the mobile app.
    Device,
}

/// A code being typed somewhere. Dropping it gives up on that code.
pub type CodeFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>;

/// Credential login for a host that asks the user in its own way: `on_guard` hears what Steam
/// Guard wants, and `ask(retry)` returns a future for the code (`retry` is set after a rejected
/// one). Steam's mobile confirmation is polled for meanwhile, so either one finishes the login.
/// The password is used for this call only.
pub async fn login_user_with(
    store: &AccountStore,
    name: &str,
    password: &str,
    mut on_guard: impl FnMut(GuardEvent),
    mut ask: impl FnMut(bool) -> CodeFuture,
) -> Result<Account> {
    let client = session::connect().await?;
    let key = client.get_password_rsa_public_key(name).await?;
    let (Some(modulus), Some(exponent)) = (key.publickey_mod, key.publickey_exp) else {
        return Err(Error::Protocol(
            "Steam did not return a password key".into(),
        ));
    };
    let encrypted = steamroom::crypto::rsa::encrypt_with_rsa_public_key(
        password.as_bytes(),
        &modulus,
        &exponent,
    )
    .map_err(steamroom::Error::from)?;
    let request = CAuthenticationBeginAuthSessionViaCredentialsRequest {
        account_name: Some(name.to_string()),
        encrypted_password: Some(STANDARD.encode(encrypted)),
        encryption_timestamp: Some(key.timestamp.unwrap_or(0)),
        remember_login: Some(true),
        persistence: Some(PERSISTENT),
        platform_type: Some(PLATFORM_STEAM_CLIENT),
        device_friendly_name: Some(session::device_name()),
        ..Default::default()
    };
    let begun = client.begin_auth_session_via_credentials(request).await?;
    let (Some(client_id), Some(request_id)) = (begun.client_id, begun.request_id.clone()) else {
        return Err(Error::Protocol(
            "Steam did not start an auth session".into(),
        ));
    };
    let interval = begun
        .poll_interval
        .map(|i| i.as_duration())
        .unwrap_or(Duration::from_millis(2500));
    let steam_id = begun.steam_id.unwrap_or(SteamId::new(0));

    let code_type = if begun.allowed_confirmations.contains(&GuardType::DeviceCode) {
        Some(GuardType::DeviceCode)
    } else if begun.allowed_confirmations.contains(&GuardType::EmailCode) {
        Some(GuardType::EmailCode)
    } else {
        None
    };
    let confirmable = begun
        .allowed_confirmations
        .iter()
        .any(|g| g.is_confirmation());
    if code_type.is_some() || confirmable {
        on_guard(GuardEvent::Needed {
            kind: code_type.map(|t| {
                if t == GuardType::EmailCode {
                    GuardKind::Email
                } else {
                    GuardKind::Device
                }
            }),
            confirmable,
        });
    }

    let mut input = code_type.map(|_| ask(false));
    let tokens = loop {
        tokio::select! {
            line = async { input.as_mut().unwrap().await }, if input.is_some() => {
                input = None;
                let code = line?;
                match client.submit_steam_guard_code(AuthClientId::new(client_id.raw()), steam_id, &code, code_type.unwrap()).await {
                    Ok(()) => {}
                    Err(steamroom::Error::Connection(ConnectionError::ServiceMethodFailed(e))) => {
                        on_guard(GuardEvent::Rejected(format!("{e:?}")));
                        input = Some(ask(true));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            _ = tokio::time::sleep(interval) => {
                if let Some(tokens) = client.poll_auth_session(client_id, &request_id).await? {
                    break tokens;
                }
            }
        }
    };
    save(
        store,
        tokens.account_name.unwrap_or_else(|| name.to_string()),
        tokens.refresh_token,
    )
}

/// Credential login: password prompt, then Steam Guard (code and/or confirmation in the mobile app).
pub async fn login_user(store: &AccountStore, name: &str) -> Result<Account> {
    let password = tokio::task::spawn_blocking(|| rpassword::prompt_password("Steam password: "))
        .await
        .map_err(|e| Error::Protocol(e.to_string()))??;
    login_user_with(
        store,
        name,
        &password,
        |event| match event {
            GuardEvent::Needed { kind, confirmable } => {
                let or_app = if confirmable {
                    " (or confirm in the Steam mobile app)"
                } else {
                    ""
                };
                match kind {
                    Some(GuardKind::Email) => {
                        eprintln!("Steam Guard: enter the code emailed to you{or_app}.")
                    }
                    Some(GuardKind::Device) => {
                        eprintln!("Steam Guard: enter the authenticator code{or_app}.")
                    }
                    None => eprintln!("Steam Guard: confirm the login in the Steam mobile app."),
                }
            }
            GuardEvent::Rejected(why) => eprintln!("Steam rejected the code ({why}); try again."),
        },
        |_retry| {
            Box::pin(async {
                tokio::task::spawn_blocking(|| prompt("Code: "))
                    .await
                    .map_err(|e| Error::Protocol(e.to_string()))?
                    .map_err(Error::from)
            })
        },
    )
    .await
}
