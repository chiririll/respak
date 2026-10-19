//! Jobs: a batch of conversions that runs on its own, reports as it goes, and can be cancelled.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use respak_core::{ConvertRequest, FromSpec, OutputRequest, OutputResult, TargetSpec};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::events::UiEvent;
use crate::session::{detached_convert, Session};

/// What the basket holds, one row: `from` is a path or a glob.
#[derive(Debug, Clone, Deserialize)]
pub struct BasketItem {
    /// Path or glob on the input.
    pub from: String,
    /// Target format and its options.
    pub to: TargetSpec,
    /// Where under the output directory, if not the default.
    #[serde(default)]
    pub path: Option<String>,
    /// Leave out what has no path to the target (for globs).
    #[serde(default)]
    pub skip_unsupported: bool,
}

/// A job's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Working.
    Running,
    /// Every basket row was processed (some outputs may have failed).
    Done,
    /// Stopped on request.
    Cancelled,
}

/// The result of one output.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputView {
    /// The source it read.
    pub from: String,
    /// Whether it was written.
    pub ok: bool,
    /// Files written, relative to the output directory.
    pub files: Vec<String>,
    /// `{kind, message, ..}` of the failure.
    pub error: Option<serde_json::Value>,
}

/// One basket row of a job.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowView {
    /// The row's `from`.
    pub from: String,
    /// The target's format.
    pub target: String,
    /// Waiting, running or finished.
    pub status: &'static str,
    /// Its outputs.
    pub outputs: Vec<OutputView>,
    /// The error of a row that was refused as a whole.
    pub error: Option<serde_json::Value>,
}

/// A job as the page sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    /// Identifier.
    pub id: u64,
    /// Seconds since the epoch.
    pub started: u64,
    /// State.
    pub state: JobState,
    /// Rows processed.
    pub done: usize,
    /// Rows in all.
    pub total: usize,
    /// The rows.
    pub rows: Vec<RowView>,
}

struct Job {
    view: JobView,
    cancel: CancellationToken,
}

/// All jobs of the server.
#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Inner>,
}

struct Inner {
    next: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Job>>,
    ui: broadcast::Sender<UiEvent>,
}

impl Jobs {
    /// Jobs that report to `ui`.
    pub fn new(ui: broadcast::Sender<UiEvent>) -> Self {
        Self {
            inner: Arc::new(Inner {
                next: AtomicU64::new(1),
                jobs: Mutex::default(),
                ui,
            }),
        }
    }

    /// Starts converting the rows on `session`. Returns the job at its start.
    pub fn start(&self, session: Arc<Session>, items: Vec<BasketItem>) -> JobView {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let cancel = session.cancel.child_token();
        let view = JobView {
            id,
            started: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            state: JobState::Running,
            done: 0,
            total: items.len(),
            rows: items
                .iter()
                .map(|item| RowView {
                    from: item.from.clone(),
                    target: item.to.format.name().to_owned(),
                    status: "waiting",
                    outputs: Vec::new(),
                    error: None,
                })
                .collect(),
        };
        self.inner.jobs.lock().unwrap().insert(
            id,
            Job {
                view: view.clone(),
                cancel: cancel.clone(),
            },
        );
        let jobs = self.clone();
        tokio::spawn(async move { jobs.run(id, session, items, cancel).await });
        view
    }

    async fn run(
        &self,
        id: u64,
        session: Arc<Session>,
        items: Vec<BasketItem>,
        cancel: CancellationToken,
    ) {
        for (index, item) in items.into_iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            self.update(id, |view| view.rows[index].status = "running");
            let request = ConvertRequest::new(vec![OutputRequest {
                from: FromSpec::from(item.from.as_str()),
                to: item.to,
                path: item.path,
                skip_unsupported: item.skip_unsupported,
            }]);
            let result = detached_convert(session.engine.clone(), request, cancel.clone()).await;
            self.update(id, |view| {
                let row = &mut view.rows[index];
                row.status = "finished";
                match result {
                    Ok(report) => {
                        row.outputs = report.outputs.into_iter().map(output_view).collect()
                    }
                    Err(error) => row.error = serde_json::to_value(&error).ok(),
                }
                view.done += 1;
            });
        }
        self.update(id, |view| {
            view.state = if cancel.is_cancelled() {
                JobState::Cancelled
            } else {
                JobState::Done
            };
            for row in &mut view.rows {
                if row.status != "finished" {
                    row.status = "skipped";
                }
            }
        });
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut JobView)) {
        let view = {
            let mut jobs = self.inner.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(&id) else { return };
            change(&mut job.view);
            job.view.clone()
        };
        let _ = self.inner.ui.send(UiEvent::Job(view));
    }

    /// Every job, oldest first.
    pub fn list(&self) -> Vec<JobView> {
        self.inner
            .jobs
            .lock()
            .unwrap()
            .values()
            .map(|job| job.view.clone())
            .collect()
    }

    /// Cancels a running job, or forgets a finished one. `false` if there is no such job.
    pub fn cancel_or_dismiss(&self, id: u64) -> bool {
        let mut jobs = self.inner.jobs.lock().unwrap();
        let Some(job) = jobs.get(&id) else {
            return false;
        };
        if job.view.state == JobState::Running {
            job.cancel.cancel();
        } else {
            jobs.remove(&id);
        }
        true
    }
}

fn output_view(output: respak_core::OutputReport) -> OutputView {
    match output.result {
        OutputResult::Ok { files, .. } => OutputView {
            from: output.from,
            ok: true,
            files,
            error: None,
        },
        OutputResult::Err { error } => OutputView {
            from: output.from,
            ok: false,
            files: Vec::new(),
            error: serde_json::to_value(&error).ok(),
        },
    }
}
