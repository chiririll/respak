//! Logging in to Steam from the page (docs/design/webui.md section 2, docs/design/steam.md): the QR
//! flow and the credentials flow with Steam Guard. The flows themselves are `respak-steam`'s; this
//! only turns what they report into events for the page, and the page's answers into their input.
//!
//! The password is held for the one call that uses it and is never logged, stored or answered
//! with. The token stays in `.respak/account.json`, and nothing here returns it.

use std::sync::{Arc, Mutex};

use respak_steam::{AccountStore, GuardEvent, GuardKind, QrMatrix};
use serde::Serialize;
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::error::{ApiError, ApiResult};
use crate::events::UiEvent;

/// What a login in progress tells the page.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoginEvent {
    /// Scan this with the Steam mobile app; a new one follows when Steam rotates it.
    Qr {
        /// The challenge URL.
        url: String,
        /// Its QR code.
        qr: QrMatrix,
    },
    /// Steam Guard wants a code, a confirmation in the app, or either.
    NeedsCode {
        /// `email`, `device`, or nothing when only the app can confirm.
        kind: Option<&'static str>,
        /// Whether approving in the Steam mobile app also works.
        confirmable: bool,
        /// Whether the code before was rejected.
        retry: bool,
    },
    /// The login finished; the token is stored.
    Done {
        /// The account.
        account_name: String,
    },
    /// The login stopped.
    Failed {
        /// Why.
        message: String,
    },
}

/// The login in progress, if any.
#[derive(Clone, Default)]
pub struct Login {
    active: Arc<Mutex<Option<Active>>>,
}

struct Active {
    cancel: CancellationToken,
    code: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

/// What a flow needs from outside.
struct Hooks {
    ui: broadcast::Sender<UiEvent>,
    cancel: CancellationToken,
    code: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

impl Hooks {
    fn say(&self, event: LoginEvent) {
        let _ = self.ui.send(UiEvent::SteamLogin { event });
    }
}

impl Login {
    /// Starts a login with `flow`, which runs on a thread of its own: Steam's futures are not
    /// `Send`, and driving them in place is what the CLI does too. Only one login at a time.
    fn start<F>(&self, ui: broadcast::Sender<UiEvent>, flow: F) -> ApiResult<()>
    where
        F: FnOnce(
                Hooks,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>>>>
            + Send
            + 'static,
    {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(ApiError::new(
                axum::http::StatusCode::CONFLICT,
                "Conflict",
                "a Steam login is already in progress: finish or cancel it first",
            ));
        }
        let (cancel, code) = (CancellationToken::new(), Arc::default());
        *active = Some(Active {
            cancel: cancel.clone(),
            code: Arc::clone(&code),
        });
        let hooks = Hooks {
            ui: ui.clone(),
            cancel: cancel.clone(),
            code,
        };
        let me = self.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let flow = flow(hooks);
            let outcome = handle.block_on(async {
                tokio::select! {
                    _ = cancel.cancelled() => Err("cancelled".to_owned()),
                    outcome = flow => outcome,
                }
            });
            *me.active.lock().unwrap() = None;
            let event = match outcome {
                Ok(account_name) => LoginEvent::Done { account_name },
                Err(message) => LoginEvent::Failed { message },
            };
            let _ = ui.send(UiEvent::SteamLogin { event });
        });
        Ok(())
    }

    /// Starts the QR flow.
    pub fn start_qr(&self, ui: broadcast::Sender<UiEvent>, store: AccountStore) -> ApiResult<()> {
        self.start(ui, move |hooks| {
            Box::pin(async move {
                let account =
                    respak_steam::login_qr_with(&store, |url| match respak_steam::qr_matrix(url) {
                        Ok(qr) => hooks.say(LoginEvent::Qr {
                            url: url.to_owned(),
                            qr,
                        }),
                        Err(e) => hooks.say(LoginEvent::Failed {
                            message: e.to_string(),
                        }),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(name_of(account))
            })
        })
    }

    /// Starts the credentials flow. `password` is moved into the flow and dropped with it.
    pub fn start_credentials(
        &self,
        ui: broadcast::Sender<UiEvent>,
        store: AccountStore,
        username: String,
        password: String,
    ) -> ApiResult<()> {
        self.start(ui, move |hooks| {
            Box::pin(async move {
                let hooks = Arc::new(hooks);
                let on_guard = {
                    let hooks = Arc::clone(&hooks);
                    move |event: GuardEvent| match event {
                        GuardEvent::Needed { kind, confirmable } => hooks.say(LoginEvent::NeedsCode {
                            kind: kind.map(|k| match k {
                                GuardKind::Email => "email",
                                GuardKind::Device => "device",
                            }),
                            confirmable,
                            retry: false,
                        }),
                        GuardEvent::Rejected(_) => hooks.say(LoginEvent::NeedsCode {
                            kind: None,
                            confirmable: false,
                            retry: true,
                        }),
                    }
                };
                let ask = {
                    let hooks = Arc::clone(&hooks);
                    move |_retry: bool| -> respak_steam::CodeFuture {
                        let (tx, rx) = oneshot::channel();
                        *hooks.code.lock().unwrap() = Some(tx);
                        let cancel = hooks.cancel.clone();
                        Box::pin(async move {
                            tokio::select! {
                                _ = cancel.cancelled() => Err(respak_steam::Error::Protocol("cancelled".into())),
                                code = rx => code.map_err(|_| respak_steam::Error::Protocol("no code was given".into())),
                            }
                        })
                    }
                };
                let account =
                    respak_steam::login_user_with(&store, &username, &password, on_guard, ask)
                        .await
                        .map_err(|e| e.to_string())?;
                Ok(name_of(account))
            })
        })
    }

    /// Gives the code the page's user typed to the flow waiting for it.
    pub fn code(&self, code: String) -> ApiResult<()> {
        let active = self.active.lock().unwrap();
        let sender = active
            .as_ref()
            .and_then(|a| a.code.lock().unwrap().take())
            .ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::CONFLICT,
                    "Conflict",
                    "no Steam code is being asked for",
                )
            })?;
        let _ = sender.send(code);
        Ok(())
    }

    /// Gives up on the login in progress.
    pub fn cancel(&self) {
        if let Some(active) = self.active.lock().unwrap().as_ref() {
            active.cancel.cancel();
        }
    }

    /// Whether a login is in progress.
    pub fn running(&self) -> bool {
        self.active.lock().unwrap().is_some()
    }
}

fn name_of(account: respak_steam::Account) -> String {
    if account.account_name.is_empty() {
        format!("steam id {}", account.steam_id)
    } else {
        account.account_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_have_no_room_for_a_secret() {
        // The page learns the state of the flow and the QR code; no event carries a token or a
        // password, so the serialized forms cannot leak one.
        let json = serde_json::to_string(&LoginEvent::NeedsCode {
            kind: Some("email"),
            confirmable: true,
            retry: false,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"state":"needsCode","kind":"email","confirmable":true,"retry":false}"#
        );
        let done = serde_json::to_string(&LoginEvent::Done {
            account_name: "gordon".into(),
        })
        .unwrap();
        assert_eq!(done, r#"{"state":"done","accountName":"gordon"}"#);
    }

    #[tokio::test]
    async fn a_code_with_nobody_asking_is_refused() {
        let login = Login::default();
        assert!(login.code("12345".into()).is_err());
        assert!(!login.running());
    }
}
