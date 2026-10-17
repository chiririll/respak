use bytes::Bytes;
use opendal::Operator;
use respak_core::{CancellationToken, ErrorKind};
use tokio::sync::Mutex;

use super::body::Body;
use respak_respawn_testing::oodle_stored;

const BLOCK: usize = 0x40000;

async fn body(stream: Vec<u8>, len: usize) -> (Body, Operator, CancellationToken) {
    let op = Operator::new(opendal::services::Memory::default()).unwrap();
    let stream_len = stream.len() as u64;
    let at = 100; // some bytes before the stream: the header
    let cancel = CancellationToken::new();
    let mut file = vec![0u8; at];
    file.extend(stream);
    op.write("p", file).await.unwrap();
    let body = Body::Oodle {
        op: op.clone(),
        path: "p".into(),
        start: at as u64,
        len: len as u64,
        stream_len,
        state: Mutex::default(),
        cancel: cancel.clone(),
    };
    (body, op, cancel)
}

fn data(n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| (i % 251) as u8 ^ (i / BLOCK) as u8)
        .collect()
}

#[tokio::test]
async fn reads_across_segments_and_decodes_only_what_it_needs() {
    let data = data(7 * BLOCK + 123);
    // segments of 2 blocks: [0,2) [2,4) [4,6) [6,7.x)
    let (body, _, _) = body(oodle_stored(&data, 2), data.len()).await;
    // inside the last segment first: earlier ones stay undecoded
    let tail = (6 * BLOCK + 10) as u64..data.len() as u64;
    assert_eq!(
        body.read(tail.clone()).await.unwrap(),
        Bytes::copy_from_slice(&data[tail.start as usize..])
    );
    let Body::Oodle { state, .. } = &body else {
        unreachable!()
    };
    assert_eq!(state.lock().await.segments.len(), 1);
    // across three segments
    let r = (BLOCK as u64 + 5)..(5 * BLOCK as u64 + 5);
    assert_eq!(
        body.read(r.clone()).await.unwrap(),
        Bytes::copy_from_slice(&data[r.start as usize..r.end as usize])
    );
    assert_eq!(state.lock().await.segments.len(), 4);
    // empty and out of range
    assert!(body.read(5..5).await.unwrap().is_empty());
    assert!(body.read(0..data.len() as u64 + 1).await.is_err());
}

#[tokio::test]
async fn a_triggered_token_stops_decoding_between_segments() {
    let data = data(7 * BLOCK + 123);
    let (body, _, cancel) = body(oodle_stored(&data, 2), data.len()).await;
    let tail = (6 * BLOCK) as u64..data.len() as u64;
    body.read(tail).await.unwrap();
    cancel.cancel();
    let err = body.read(0..(5 * BLOCK) as u64).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    let Body::Oodle { state, .. } = &body else {
        unreachable!()
    };
    assert_eq!(
        state.lock().await.segments.len(),
        1,
        "no segment was decoded after the cancel"
    );
}

#[tokio::test]
async fn a_stream_that_ends_early_is_corrupt() {
    let data = data(3 * BLOCK);
    let mut stream = oodle_stored(&data, 1);
    stream.truncate(stream.len() - 1000);
    let (body, _, _) = body(stream, data.len()).await;
    assert!(body.read(0..data.len() as u64).await.is_err());
    // the part before the cut still reads
    assert_eq!(
        body.read(0..10).await.unwrap(),
        Bytes::copy_from_slice(&data[..10])
    );
}
