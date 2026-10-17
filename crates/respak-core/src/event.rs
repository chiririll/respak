//! Events (docs/design/project.md §5.7): a broadcast stream of what the engine is doing.
//!
//! Lagging subscribers may miss events; anything that matters is in the returned report.

use tokio::sync::broadcast;

/// Default capacity of the event channel.
pub const DEFAULT_EVENT_CAPACITY: usize = 1024;

/// Something that happened during an `Engine` call.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// A conversion request started (`what` = source key -> target format).
    RequestStarted {
        /// Description of the request.
        what: String,
    },
    /// A conversion request finished.
    RequestFinished {
        /// Description of the request.
        what: String,
    },
    /// A conversion request failed.
    RequestFailed {
        /// Description of the request.
        what: String,
        /// The error message.
        error: String,
    },
    /// A container was mounted.
    InputOpened {
        /// Path of the container.
        path: String,
        /// Number of entries in it.
        entries: usize,
    },
    /// A file was written to the output.
    OutputWritten {
        /// Output path.
        path: String,
    },
    /// Progress of a task.
    Progress {
        /// What is progressing (e.g. `convert`, `texture foo mips`).
        task: String,
        /// Units done.
        done: u64,
        /// Total units.
        total: u64,
    },
    /// Something odd that did not stop the work.
    Warning {
        /// Human-readable message.
        message: String,
    },
    /// An event of a plugin or host.
    Custom {
        /// Who sent it.
        source: String,
        /// Event name.
        name: String,
        /// Payload.
        data: serde_json::Value,
    },
}

/// Sending half of the event channel. Publishing with no subscriber is not an error.
#[derive(Clone)]
pub struct Events {
    tx: broadcast::Sender<Event>,
}

impl Events {
    /// A channel holding up to `capacity` unread events per subscriber.
    pub fn new(capacity: usize) -> Self {
        Self {
            tx: broadcast::channel(capacity.max(1)).0,
        }
    }

    /// Sends `event` to every subscriber.
    pub fn publish(&self, event: Event) {
        // No subscribers is fine.
        let _ = self.tx.send(event);
    }

    /// A new receiver; it sees only events published after this call.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

/// Forwards events to `tracing` (warnings as `warn!`, the rest as `debug!`/`info!`).
pub struct LogSubscriber;

impl LogSubscriber {
    /// Spawns a task that logs every event until the channel closes.
    /// Needs a running tokio runtime.
    pub fn spawn(rx: broadcast::Receiver<Event>) -> tokio::task::JoinHandle<()> {
        let mut rx = rx;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(Event::Warning { message }) => tracing::warn!("{message}"),
                    Ok(Event::RequestFailed { what, error }) => {
                        tracing::debug!("failed {what}: {error}")
                    }
                    Ok(Event::InputOpened { path, entries }) => {
                        tracing::info!("opened {path} ({entries} entries)")
                    }
                    Ok(Event::OutputWritten { path }) => tracing::info!("wrote {path}"),
                    Ok(event) => tracing::debug!("{event:?}"),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("missed {n} events")
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}
