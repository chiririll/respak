//! Streams fed by a decoder that runs on the blocking pool.

use futures::stream::{BoxStream, StreamExt};

use crate::{Cx, Error, ErrorKind, Result};

/// Items a stream may run ahead of its consumer (a 1080p RGBA frame is 8 MB).
const BUFFER: usize = 4;

/// Runs `decode` on the blocking pool and streams what it hands to its sink. The sink returns
/// `false` when the decoder should stop: the stream was dropped or the request was cancelled (the
/// last item is then `Cancelled`).
pub fn blocking_stream<T: Send + 'static>(
    cx: &Cx,
    decode: impl FnOnce(&mut dyn FnMut(Result<T>) -> bool) + Send + 'static,
) -> BoxStream<'static, Result<T>> {
    let (tx, rx) = tokio::sync::mpsc::channel(BUFFER);
    let cx = cx.clone();
    tokio::spawn(async move {
        let sender = tx.clone();
        let cancel = cx.cancel.clone();
        let ran = cx
            .blocking(move || {
                decode(&mut |item| {
                    if cancel.is_cancelled() {
                        let _ = sender.blocking_send(Err(Error::new(ErrorKind::Cancelled)));
                        return false;
                    }
                    sender.blocking_send(item).is_ok()
                })
            })
            .await;
        if let Err(e) = ran {
            let _ = tx.send(Err(e)).await;
        }
    });
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use opendal::{services::Memory, Operator};

    use super::*;
    use crate::{Mediator, Registry};

    fn cx() -> Cx {
        let mem = || Operator::new(Memory::default()).unwrap();
        Cx::new(Mediator::new(Registry::new()), mem(), mem())
    }

    #[tokio::test]
    async fn a_stream_yields_what_the_decoder_hands_over_in_order_then_ends() {
        let cx = cx();
        // More items than the buffer holds: the decoder has to wait for the consumer.
        let items: Vec<u32> = (0..50).collect();
        let sent = items.clone();
        let stream = blocking_stream(&cx, move |sink| {
            for i in sent {
                if !sink(Ok(i)) {
                    return;
                }
            }
        });
        let got: Vec<u32> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(got, items);
    }

    #[tokio::test]
    async fn a_failure_is_the_last_item() {
        let cx = cx();
        let mut stream = blocking_stream(&cx, |sink| {
            sink(Ok(1u32));
            sink(Err(Error::corrupt("cut here")));
        });
        assert_eq!(stream.next().await.unwrap().unwrap(), 1);
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn dropping_the_stream_stops_the_decoder() {
        let cx = cx();
        let produced = Arc::new(AtomicUsize::new(0));
        let counter = produced.clone();
        let mut stream = blocking_stream(&cx, move |sink| {
            while sink(Ok(0u32)) {
                counter.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        stream.next().await.unwrap().unwrap();
        drop(stream);
        // The decoder notices at its next send, so it stops soon, nowhere near "forever".
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let stopped_at = produced.load(Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            produced.load(Ordering::SeqCst),
            stopped_at,
            "still decoding after the drop"
        );
        assert!(stopped_at < 50, "{stopped_at}");
    }

    #[tokio::test]
    async fn a_triggered_token_ends_the_stream_with_cancelled() {
        let cx = cx();
        let cancel = cx.cancel.clone();
        let mut stream = blocking_stream(&cx, |sink| {
            let mut i = 0u32;
            while sink(Ok(i)) {
                i += 1;
            }
        });
        stream.next().await.unwrap().unwrap();
        cancel.cancel();
        let mut last = None;
        while let Some(item) = stream.next().await {
            last = Some(item);
        }
        let err = last.expect("items after the cancel").unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    }

    #[tokio::test]
    async fn a_token_triggered_before_the_start_gives_cancelled_and_no_decoding() {
        let cx = cx();
        cx.cancel.cancel();
        let ran = Arc::new(AtomicUsize::new(0));
        let counter = ran.clone();
        let mut stream = blocking_stream(&cx, move |_sink: &mut dyn FnMut(Result<u32>) -> bool| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}
