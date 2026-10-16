//! `LazyBlob`: computed on demand, computed ranges kept, streamed to the output.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use common::cx_for;
use futures::stream;
use respak_core::capability::{read_all, Blob};
use respak_core::value::{LazyBlob, LazySource};
use respak_core::{Error, ErrorKind, FormatId, Registry, ValueKey};

fn key(name: &str) -> ValueKey {
    ValueKey::file(name)
}

/// A blob of `parts` chunks of `size` bytes (chunk `i` is all `i as u8`), counting the chunks pulled.
fn counting(parts: usize, size: usize) -> (LazyBlob, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let started = Arc::new(AtomicUsize::new(0));
    let pulled = Arc::new(AtomicUsize::new(0));
    let (s, p) = (started.clone(), pulled.clone());
    let blob = LazyBlob::new(
        FormatId::new("avi"),
        key("v"),
        Box::new(move |_cx| {
            Box::pin(async move {
                s.fetch_add(1, Ordering::SeqCst);
                let chunks = (0..parts).map(move |i| {
                    p.fetch_add(1, Ordering::SeqCst);
                    Ok(Bytes::from(vec![i as u8; size]))
                });
                Ok(LazySource::Stream(Box::pin(stream::iter(chunks))))
            })
        }),
    );
    (blob, started, pulled)
}

#[tokio::test]
async fn nothing_runs_until_the_first_read() {
    let (_, cx) = cx_for(Registry::new());
    let (blob, started, pulled) = counting(4, 10);
    assert_eq!(started.load(Ordering::SeqCst), 0);
    assert_eq!(blob.len(), None);
    let got = blob.read(0..5, &cx).await.unwrap();
    assert_eq!(&got[..], &[0; 5]);
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(
        pulled.load(Ordering::SeqCst),
        1,
        "one chunk covers the first 5 bytes"
    );
}

#[tokio::test]
async fn a_read_computes_only_as_far_as_it_needs_and_earlier_ranges_are_kept() {
    let (_, cx) = cx_for(Registry::new());
    let (blob, started, pulled) = counting(4, 10);
    let mid = blob.read(15..25, &cx).await.unwrap();
    assert_eq!(&mid[..], &[1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
    assert_eq!(pulled.load(Ordering::SeqCst), 3);
    // Reading what was produced again, or before it, does no work.
    assert_eq!(&blob.read(0..3, &cx).await.unwrap()[..], &[0, 0, 0]);
    assert_eq!(&blob.read(15..25, &cx).await.unwrap()[..], &mid[..]);
    assert_eq!(pulled.load(Ordering::SeqCst), 3);
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(blob.len(), None, "the end has not been seen");
}

#[tokio::test]
async fn reading_past_the_end_is_cut_and_fixes_the_length() {
    let (_, cx) = cx_for(Registry::new());
    let (blob, _, _) = counting(3, 10);
    let tail = blob.read(25..100, &cx).await.unwrap();
    assert_eq!(&tail[..], &[2; 5]);
    assert_eq!(blob.len(), Some(30));
    assert!(blob.read(30..40, &cx).await.unwrap().is_empty());
    assert_eq!(read_all(&blob, &cx).await.unwrap().len(), 30);
}

#[tokio::test]
async fn a_finished_file_is_served_as_it_is() {
    let (_, cx) = cx_for(Registry::new());
    let blob = LazyBlob::new(
        FormatId::new("avi"),
        key("f"),
        Box::new(|_cx| {
            Box::pin(async {
                use std::io::{Seek, SeekFrom, Write};
                let mut file = tempfile::tempfile().unwrap();
                file.write_all(b"0123456789").unwrap();
                // A producer that patches its header afterwards.
                file.seek(SeekFrom::Start(0)).unwrap();
                file.write_all(b"AB").unwrap();
                Ok(LazySource::File(file))
            })
        }),
    );
    assert_eq!(&blob.read(0..4, &cx).await.unwrap()[..], b"AB23");
    assert_eq!(blob.len(), Some(10));
    assert_eq!(&read_all(&blob, &cx).await.unwrap()[..], b"AB23456789");
}

#[tokio::test]
async fn a_failure_is_final() {
    let (_, cx) = cx_for(Registry::new());
    let blob = LazyBlob::new(
        FormatId::new("avi"),
        key("e"),
        Box::new(|_cx| {
            Box::pin(async {
                let items: Vec<respak_core::Result<Bytes>> = vec![
                    Ok(Bytes::from_static(b"abc")),
                    Err(Error::new(ErrorKind::Corrupt {
                        context: "cut here".into(),
                    })),
                ];
                Ok(LazySource::Stream(Box::pin(stream::iter(items))))
            })
        }),
    );
    assert_eq!(&blob.read(0..3, &cx).await.unwrap()[..], b"abc");
    let err = blob.read(0..10, &cx).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
    let again = blob.read(0..3, &cx).await.unwrap_err();
    assert!(again.to_string().contains("cut here"), "{again}");
}

#[tokio::test]
async fn a_large_result_spills_to_a_file_and_reads_back() {
    let (_, cx) = cx_for(Registry::new());
    // 3 x 30 MiB: more than the memory limit.
    let (blob, _, pulled) = counting(3, 30 << 20);
    let size = 30u64 << 20;
    let head = blob.read(0..4, &cx).await.unwrap();
    assert_eq!(&head[..], &[0; 4]);
    let across = blob.read(2 * size - 2..2 * size + 2, &cx).await.unwrap();
    assert_eq!(&across[..], &[1, 1, 2, 2]);
    let again = blob.read(size - 2..size + 2, &cx).await.unwrap();
    assert_eq!(&again[..], &[0, 0, 1, 1]);
    assert_eq!(pulled.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn write_blob_streams_to_the_output_and_skips_a_repeat_without_computing() {
    let (_, cx) = cx_for(Registry::new());
    let (blob, started, _) = counting(3, 10);
    assert!(cx.write_blob("out/v.avi", &key("v"), &blob).await.unwrap());
    let written = cx.output.read("out/v.avi").await.unwrap().to_bytes();
    assert_eq!(written.len(), 30);
    assert_eq!(&written[10..12], &[1, 1]);
    assert_eq!(started.load(Ordering::SeqCst), 1);

    let (again, started_again, _) = counting(3, 10);
    assert!(
        !cx.write_blob("out/v.avi", &key("v"), &again).await.unwrap(),
        "same key, same path"
    );
    assert_eq!(
        started_again.load(Ordering::SeqCst),
        0,
        "a skipped write never computes"
    );
}

#[tokio::test]
async fn a_failed_write_leaves_no_file_and_frees_the_path() {
    let (_, cx) = cx_for(Registry::new());
    let broken = LazyBlob::new(
        FormatId::new("avi"),
        key("v"),
        Box::new(|_cx| {
            Box::pin(async {
                let items: Vec<respak_core::Result<Bytes>> = vec![
                    Ok(Bytes::from_static(b"abc")),
                    Err(Error::new(ErrorKind::Corrupt {
                        context: "cut here".into(),
                    })),
                ];
                Ok(LazySource::Stream(Box::pin(stream::iter(items))))
            })
        }),
    );
    assert!(cx
        .write_blob("out/v.avi", &key("v"), &broken)
        .await
        .is_err());
    assert!(
        cx.output.stat("out/v.avi").await.is_err(),
        "the half-written file is gone"
    );
    let (good, _, _) = counting(1, 4);
    assert!(
        cx.write_blob("out/v.avi", &key("v"), &good).await.unwrap(),
        "the path is free again"
    );
}
