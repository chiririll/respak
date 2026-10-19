//! respak-web: the web UI of respak (docs/design/webui.md). A host like the CLI: it builds the
//! `Engine` through `respak-host` and serves it, with the embedded frontend, on `127.0.0.1`.

mod api;
mod assets;
mod catalog;
mod error;
mod events;
mod jobs;
mod security;
mod session;
#[cfg(feature = "steam")]
mod steam_login;
mod tree;

use std::path::PathBuf;
use std::sync::Arc;

use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use tokio::sync::{broadcast, RwLock};

use error::{ApiError, ApiResult};
use events::UiEvent;
use session::Session;

pub use security::Guard;

/// The state every handler shares.
#[derive(Clone)]
pub struct App {
    /// Where `respak.toml` is looked for.
    pub cwd: PathBuf,
    /// Messages to the pages.
    pub ui: broadcast::Sender<UiEvent>,
    jobs: jobs::Jobs,
    #[cfg(feature = "steam")]
    login: steam_login::Login,
    session: Arc<RwLock<Option<Arc<Session>>>>,
}

impl App {
    /// An app with no session, reading its settings from `cwd` and the directories above it.
    pub fn new(cwd: PathBuf) -> Self {
        let (ui, _) = broadcast::channel(events::UI_CAPACITY);
        Self {
            cwd,
            jobs: jobs::Jobs::new(ui.clone()),
            #[cfg(feature = "steam")]
            login: steam_login::Login::default(),
            ui,
            session: Arc::default(),
        }
    }

    pub(crate) async fn session(&self) -> Option<Arc<Session>> {
        self.session.read().await.clone()
    }

    pub(crate) async fn need_session(&self) -> ApiResult<Arc<Session>> {
        self.session().await.ok_or_else(ApiError::not_connected)
    }

    pub(crate) async fn set_session(&self, session: Arc<Session>) {
        *self.session.write().await = Some(session);
    }

    /// Cancels the running work and drops the session.
    pub(crate) async fn end_session(&self) {
        if let Some(old) = self.session.write().await.take() {
            old.cancel.cancel();
            let _ = self.ui.send(UiEvent::Session { connected: false });
        }
    }
}

/// The server: the API under `/api`, the frontend everywhere else, guarded for a server on `port`.
pub fn router(app: App, port: u16) -> Router {
    let api = Router::new()
        .route("/info", get(api::info))
        .route("/config", get(api::get_config).put(api::put_config))
        .route(
            "/session",
            get(api::get_session)
                .post(api::connect)
                .delete(api::disconnect),
        )
        .route("/steam/status", get(api::steam_status))
        .route("/steam/logout", post(api::steam_logout))
        .route("/steam/login/qr", post(api::steam_login_qr))
        .route(
            "/steam/login/credentials",
            post(api::steam_login_credentials),
        )
        .route("/steam/login/code", post(api::steam_login_code))
        .route(
            "/steam/login",
            get(api::steam_login_state).delete(api::steam_login_cancel),
        )
        .route("/catalog", get(api::get_catalog))
        .route("/catalog/resolve", post(api::resolve_catalog))
        .route("/fs", get(api::browse))
        .route("/tree", get(api::get_tree))
        .route("/search", get(api::get_search))
        .route("/entry", get(api::get_entry))
        .route("/preview/image", get(api::preview_image))
        .route("/preview/audio", get(api::preview_audio))
        .route("/preview/atlas", get(api::preview_atlas))
        .route("/preview/atlas-image", get(api::preview_atlas_image))
        .route("/preview/text", get(api::preview_text))
        .route("/preview/hex", get(api::preview_hex))
        .route("/convert", post(api::start_job))
        .route("/jobs", get(api::list_jobs))
        .route("/jobs/{id}", delete(api::cancel_job))
        .route("/output/{*path}", get(api::get_output))
        .route("/events", get(events::stream))
        .fallback(api::not_found)
        .with_state(app);
    Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        .layer(middleware::from_fn_with_state(
            Guard::new(port),
            security::check,
        ))
}
