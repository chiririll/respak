//! Cancelling and progress while encoding PNG mips.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use opendal::{services::Memory, Operator};
use respak_core::capability::{Bitmap, BitmapBuf, BitmapInfo, PixelFormat};
use respak_core::{
    CancellationToken, Cx, Data, ErrorKind, Event, FormatId, Plugin, Registry, Target,
    Value, ValueKey,
};
use respak_formats::FormatsPlugin;

/// A 4x4 RGBA8 bitmap with three mips whose first read triggers the token.
struct Img {
    buf: BitmapBuf,
    reads: AtomicUsize,
    cancel_on_first_read: Option<CancellationToken>,
}

#[async_trait]
impl Bitmap for Img {
    fn info(&self) -> BitmapInfo {
        self.buf.info()
    }
    async fn read_mip(&self, level: u32, layer: u32) -> respak_core::Result<Bytes> {
        if self.reads.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(cancel) = &self.cancel_on_first_read {
                cancel.cancel();
            }
        }
        self.buf.read_mip(level, layer).await
    }
}

struct Holder(Arc<Img>, FormatId, ValueKey);

impl std::fmt::Debug for Holder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Holder")
    }
}

impl Data for Holder {
    fn format(&self) -> &FormatId {
        &self.1
    }
    fn key(&self) -> &ValueKey {
        &self.2
    }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> {
        Some(&*self.0)
    }
}

struct ImgPlugin;

impl Plugin for ImgPlugin {
    fn id(&self) -> &str {
        "img"
    }
    fn register(&self, r: &mut Registry) {
        r.format("img").provides::<dyn Bitmap>();
    }
}

fn image(cancel: Option<CancellationToken>) -> (Arc<Img>, Value) {
    let info = BitmapInfo {
        width: 4,
        height: 4,
        depth: 1,
        array_len: 1,
        faces: 1,
        mip_count: 3,
        format: PixelFormat::Rgba8,
    };
    let mip = |side: usize| Bytes::from(vec![7u8; side * side * 4]);
    let buf = BitmapBuf::new(info, vec![vec![mip(4), mip(2), mip(1)]]).unwrap();
    let img = Arc::new(Img {
        buf,
        reads: AtomicUsize::new(0),
        cancel_on_first_read: cancel,
    });
    let value: Value = Arc::new(Holder(
        img.clone(),
        FormatId::new("img"),
        ValueKey::file("pic"),
    ));
    (img, value)
}

fn cx() -> Cx {
    let mut r = Registry::new();
    r.add(&FormatsPlugin);
    r.add(&ImgPlugin);
    r.check().unwrap();
    let mem = || Operator::new(Memory::default()).unwrap();
    respak_core::testing::cx(r, mem(), mem())
}

fn all_mips() -> Target {
    Target {
        format: FormatId::new("png"),
        options: serde_json::json!({ "mips": "all" }),
        via: Vec::new(),
    }
}

#[tokio::test]
async fn all_mips_report_progress() {
    let cx = cx();
    let mut rx = cx.mediator.subscribe();
    let (_, value) = image(None);
    cx.mediator.convert(value, all_mips(), &cx).await.unwrap();
    let mut progress = Vec::new();
    while let Ok(e) = rx.try_recv() {
        if let Event::Progress { task, done, total } = e {
            assert_eq!(task, "png mips");
            progress.push((done, total));
        }
    }
    assert_eq!(progress, [(0, 3), (1, 3), (2, 3)]);
}

#[tokio::test]
async fn a_triggered_token_stops_between_mips_and_returns_no_bundle() {
    let cx = cx();
    let (img, value) = image(Some(cx.cancel.clone()));
    let err = cx
        .mediator
        .convert(value, all_mips(), &cx)
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Cancelled), "{err}");
    assert_eq!(
        img.reads.load(Ordering::SeqCst),
        1,
        "no mip is read after the cancel"
    );
}
