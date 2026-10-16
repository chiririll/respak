//! Connecting to a Steam CM server and logging on with a refresh token.

use prost::Message;
use steamroom::{
    client::{msg::ClientMsg, LoggedIn, Ready, SteamClient, PROTOCOL_VERSION},
    connection::{CmServer, Protocol},
    error::ConnectionError,
    generated::{
        CAuthenticationAccessTokenGenerateForAppRequest,
        CAuthenticationAccessTokenGenerateForAppResponse, CMsgClientLogon,
    },
    messages::EMsg,
    transport::{tcp::TcpTransport, websocket::WebSocketTransport},
    types::SteamId,
};

use crate::account::{self, Account, AccountStore, Expiry};
use crate::error::{Error, Result};

/// Name shown in the account's authorized devices: `respak on <hostname>`.
pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into());
    format!("respak on {host}")
}

/// Connects to a CM server (TCP first, then WebSocket) and finishes the handshake.
pub async fn connect() -> Result<SteamClient<Ready>> {
    let servers = CmServer::fetch()
        .await
        .unwrap_or_else(|_| CmServer::defaults());
    let mut last = None;
    for protocol in [Protocol::Tcp, Protocol::WebSocket] {
        let Some(server) = servers.iter().find(|s| s.protocol == protocol) else {
            continue;
        };
        let attempt = async {
            Ok::<_, steamroom::Error>(match protocol {
                Protocol::Tcp => {
                    let (client, _rx) =
                        SteamClient::connect(TcpTransport::connect(server).await?).await?;
                    client.encrypt().await?
                }
                Protocol::WebSocket => {
                    SteamClient::connect_ws(WebSocketTransport::connect(server).await?)
                        .await?
                        .0
                }
            })
        };
        match attempt.await {
            Ok(client) => return Ok(client.prepare().await?),
            Err(e) => {
                tracing::warn!("CM server {server:?} failed: {e}");
                last = Some(e);
            }
        }
    }
    Err(last
        .map(Error::Steam)
        .unwrap_or_else(|| Error::Protocol("no Steam CM servers".into())))
}

/// Sends `CMsgClientLogon` with the refresh token in `access_token`.
async fn logon_with(
    client: SteamClient<Ready>,
    account: &Account,
) -> std::result::Result<SteamClient<LoggedIn>, steamroom::Error> {
    let logon = CMsgClientLogon {
        protocol_version: Some(PROTOCOL_VERSION),
        cell_id: Some(0),
        client_os_type: Some(20),
        account_name: Some(account.account_name.clone()),
        access_token: Some(account.refresh_token.clone()),
        ..Default::default()
    };
    let body = logon.encode_to_vec();
    let mut msg = ClientMsg::with_body(EMsg::CLIENT_LOGON, &body);
    msg.header.steamid = Some(SteamId::from_parts(1, 1, 1, 0).raw());
    msg.header.client_sessionid = Some(0);
    client.login(msg).await.map(|(client, _)| client)
}

/// Asks Steam for a renewed refresh token. `None` if Steam did not hand out a new one.
async fn renew(client: &SteamClient<Ready>, account: &Account) -> Result<Option<String>> {
    let request = CAuthenticationAccessTokenGenerateForAppRequest {
        refresh_token: Some(account.refresh_token.clone()),
        steamid: Some(account.steam_id),
        renewal_type: Some(1),
    };
    let reply = client
        .call_service_method_non_authed(
            "Authentication.GenerateAccessTokenForApp#1",
            &request.encode_to_vec(),
        )
        .await?;
    let reply: CAuthenticationAccessTokenGenerateForAppResponse = reply
        .decode()
        .map_err(|e| Error::Protocol(format!("renewal reply: {e}")))?;
    Ok(reply.refresh_token.filter(|t| !t.is_empty()))
}

/// Logs on with the stored token. A token that is expired or near expiry is renewed first; one that
/// Steam rejects is renewed once and retried. Never prompts: failure is `NeedLogin`.
pub async fn logon(store: &AccountStore) -> Result<(SteamClient<LoggedIn>, Account)> {
    let mut account = store.load()?;
    let expiry = account::expiry(&account.refresh_token, account::now_secs())?;
    let mut renewed = false;
    if expiry != Expiry::Fresh {
        renewed = true;
        match renew(&connect().await?, &account).await {
            Ok(Some(token)) => {
                account.refresh_token = token;
                store.save(&account)?;
            }
            Ok(None) | Err(_) if expiry == Expiry::Near => {}
            Ok(None) => {
                return Err(Error::NeedLogin(
                    "the stored token expired and could not be renewed".into(),
                ))
            }
            Err(e) => {
                return Err(Error::NeedLogin(format!(
                    "the stored token expired and renewal failed: {e}"
                )))
            }
        }
    }
    loop {
        match logon_with(connect().await?, &account).await {
            Ok(client) => return Ok((client, account)),
            Err(steamroom::Error::Connection(ConnectionError::LogonFailed(code))) if !renewed => {
                renewed = true;
                tracing::info!("Steam rejected the token ({code:?}); renewing once");
                match renew(&connect().await?, &account).await {
                    Ok(Some(token)) => {
                        account.refresh_token = token;
                        store.save(&account)?;
                    }
                    _ => {
                        return Err(Error::NeedLogin(format!(
                            "Steam rejected the stored token ({code:?})"
                        )))
                    }
                }
            }
            Err(steamroom::Error::Connection(ConnectionError::LogonFailed(code))) => {
                return Err(Error::NeedLogin(format!(
                    "Steam rejected the stored token ({code:?})"
                )))
            }
            Err(e) => return Err(e.into()),
        }
    }
}
