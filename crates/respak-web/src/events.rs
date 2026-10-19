//! What the server tells the page as it happens: server-sent events.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as Sse, KeepAlive};
use axum::response::Sse as SseResponse;
use futures::Stream;
use respak_core::broadcast::error::RecvError;
use respak_core::{CancellationToken, Event};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::jobs::JobView;
use crate::App;

/// Capacity of the channel to the pages.
pub const UI_CAPACITY: usize = 256;

/// One message to the page.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    /// A job started, moved on or ended.
    Job(JobView),
    /// A line for the activity log.
    Log {
        /// `info` or `warn`.
        level: &'static str,
        /// The text.
        message: String,
    },
    /// Progress of a task of the engine.
    Progress {
        /// What is progressing.
        task: String,
        /// Units done.
        done: u64,
        /// Total units.
        total: u64,
    },
    /// The Steam input moved data.
    Steam {
        /// What happened (`request`, `chunk`).
        name: String,
        /// The payload of the engine's event.
        data: serde_json::Value,
    },
    /// A Steam login in progress moved on.
    #[cfg(feature = "steam")]
    SteamLogin {
        /// What happened.
        #[serde(flatten)]
        event: crate::steam_login::LoginEvent,
    },
    /// The session was started or ended.
    Session {
        /// Whether there is a session now.
        connected: bool,
    },
}

/// Sends what the engine reports to the page until `cancel` is cancelled.
pub fn forward(
    mut rx: broadcast::Receiver<Event>,
    ui: broadcast::Sender<UiEvent>,
    cancel: CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                _ = cancel.cancelled() => break,
                event = rx.recv() => event,
            };
            let event = match event {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            if let Some(event) = translate(event) {
                let _ = ui.send(event);
            }
        }
    });
}

fn translate(event: Event) -> Option<UiEvent> {
    let log = |level, message| Some(UiEvent::Log { level, message });
    match event {
        Event::Warning { message } => log("warn", message),
        Event::InputOpened { path, entries } => {
            log("info", format!("opened {path} ({entries} entries)"))
        }
        Event::OutputWritten { path } => log("info", format!("wrote {path}")),
        Event::Progress { task, done, total } => Some(UiEvent::Progress { task, done, total }),
        Event::Custom { source, name, data } if source == "steam" => match name.as_str() {
            "depot" => log(
                "info",
                format!(
                    "steam: depot {} manifest {} ({} files)",
                    data["depot"], data["manifest"], data["files"]
                ),
            ),
            _ => Some(UiEvent::Steam { name, data }),
        },
        _ => None,
    }
}

/// `GET /api/events`: every [`UiEvent`] as one JSON line of an event stream.
pub async fn stream(
    State(app): State<App>,
) -> SseResponse<impl Stream<Item = Result<Sse, Infallible>>> {
    let rx = app.ui.subscribe();
    let connected = app.session().await.is_some();
    let hello = serde_json::to_string(&UiEvent::Session { connected }).unwrap_or_default();
    let first = futures::stream::once(async move { Ok(Sse::default().data(hello)) });
    let rest = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    return Some((Ok(Sse::default().data(data)), rx));
                }
                // A slow page misses events; the jobs are re-read by it when it reconnects.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    SseResponse::new(futures::StreamExt::chain(first, rest))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
