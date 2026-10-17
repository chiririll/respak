//! Running one async function over many items on a pool of workers.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::StreamExt;

use super::{INSPECT_PROGRESS, WORKER_CONCURRENCY};
use crate::error::Result;
use crate::event::Event;
use crate::mediator::Cx;

/// Applies `f` to every item and returns the results in the order of `items`.
///
/// Items are read several at a time, on as many threads as there are cores (on one, in a
/// single-threaded runtime). Each worker takes the next unread item until there are none, so a
/// slow item does not hold up the others. Fails with `Cancelled` once `cx` is cancelled, and
/// publishes `Progress` for `task` every [`INSPECT_PROGRESS`] items.
pub(super) async fn parallel_map_ordered<T, R, F, Fut>(
    items: &[T],
    cx: &Cx,
    task: &str,
    f: F,
) -> Result<Vec<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Fut + Sync,
    Fut: Future<Output = R>,
{
    let total = items.len();
    let handle = tokio::runtime::Handle::current();
    let workers = match handle.runtime_flavor() {
        tokio::runtime::RuntimeFlavor::MultiThread => std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(total.max(1)),
        _ => 1,
    };
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let (next, done, f) = (&next, &done, &f);
    let worker = || async move {
        let mut read = Vec::new();
        let mut results = futures::stream::iter(std::iter::from_fn(|| {
            let i = next.fetch_add(1, Ordering::Relaxed);
            (i < total).then_some(i)
        }))
        .map(|i| async move { (i, f(&items[i]).await) })
        .buffer_unordered(WORKER_CONCURRENCY);
        while let Some(result) = results.next().await {
            cx.check_cancelled()?;
            read.push(result);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % INSPECT_PROGRESS == 0 {
                cx.mediator.publish(Event::Progress {
                    task: task.to_owned(),
                    done: n as u64,
                    total: total as u64,
                });
            }
        }
        Ok::<_, crate::error::Error>(read)
    };
    let mut read: Vec<(usize, R)> = if workers == 1 {
        worker().await?
    } else {
        let results = tokio::task::block_in_place(|| {
            std::thread::scope(|scope| {
                let threads: Vec<_> = (0..workers)
                    .map(|_| scope.spawn(|| handle.block_on(worker())))
                    .collect();
                threads
                    .into_iter()
                    .map(|t| t.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
                    .collect::<Vec<_>>()
            })
        });
        let mut all = Vec::with_capacity(total);
        for result in results {
            all.extend(result?);
        }
        all
    };
    read.sort_unstable_by_key(|(i, _)| *i);
    Ok(read.into_iter().map(|(_, r)| r).collect())
}
