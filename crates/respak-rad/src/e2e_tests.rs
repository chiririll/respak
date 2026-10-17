//! A synthetic bank and stream through the real engine: discovery, opening a source and
//! converting it to WAV. No game files.

use opendal::{services::Memory, Operator};
use respak_core::{
    CancellationToken, Cx, Engine, ErrorKind, FormatId, Registry, Target, Value,
};
use respak_formats::FormatsPlugin;

use crate::mbnk::fixtures::{bank, Fx};
use crate::RadPlugin;

const BANK: &str = "audio/ship/general.mbnk";
const STREAM: &str = "audio/ship/general_stream.mstr";
const SOURCE: &str = "audio/ship/general.mbnk/vo/line.asrc";
const TAG: u32 = 0xBEEF;
/// Where the codec header block and the data sit in the synthetic `.mstr`.
const HEADER_AT: usize = 0x100;
const DATA_AT: usize = 0x200;

fn cx_on(op: &Operator) -> Cx {
    let mut r = Registry::new();
    r.add(&FormatsPlugin);
    r.add(&RadPlugin);
    respak_core::testing::cx(r, op.clone(), op.clone())
}

fn mem() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

/// Writes bits least significant first, the order Bink Audio reads them.
struct Bits(Vec<u8>, usize);

impl Bits {
    fn put(&mut self, value: u32, n: usize) {
        for i in 0..n {
            if (value >> i) & 1 != 0 {
                self.0[self.1 / 8] |= 1 << (self.1 % 8);
            }
            self.1 += 1;
        }
    }
}

/// One frame payload: channel 0 has a DC coefficient (256.0), channel 1 is silent.
fn payload() -> Vec<u8> {
    let mut b = Bits(vec![0; 1024], 0);
    b.put(0, 2);
    b.put(9, 5);
    b.put(1 << 22, 23);
    b.0
}

fn bcf1_header(channels: u8, file_size: u32) -> Vec<u8> {
    let mut h = b"1FCB".to_vec();
    h.extend([2, channels]);
    h.extend(48000u16.to_le_bytes());
    h.extend(2000u32.to_le_bytes()); // frames per channel: one full block and 80 of the next
    h.extend(1024u32.to_le_bytes()); // largest payload
    h.extend(file_size.to_le_bytes());
    h.extend(0u32.to_le_bytes()); // no seek table
    h
}

/// Two frames: a plain one with the DC payload, and a silent one in the `0xFFFF` form that
/// keeps 80 samples.
fn frames() -> Vec<u8> {
    let mut d = vec![0x99, 0x99];
    d.extend(1024u16.to_le_bytes());
    d.extend(payload());
    d.extend([0x99, 0x99, 0xFF, 0xFF]);
    d.extend(1024u16.to_le_bytes());
    d.extend(80u16.to_le_bytes());
    d.extend(vec![0u8; 1024]);
    d
}

fn mstr(build_tag: u32, codec_header: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = b"RTSC".to_vec();
    m.extend(2u16.to_le_bytes());
    m.extend(0xFFFFu16.to_le_bytes());
    m.extend((DATA_AT as u32).to_le_bytes());
    m.extend(0i32.to_le_bytes());
    m.extend(build_tag.to_le_bytes());
    m.resize(HEADER_AT, 0);
    m.extend(codec_header);
    m.resize(DATA_AT, 0);
    m.extend(data);
    m
}

fn line(header_size: u32) -> Fx {
    let mut f = Fx::new("vo/line");
    f.header_off = HEADER_AT as u64;
    f.header_size = header_size;
    f
}

async fn put_stereo_bank(op: &Operator) {
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    let data = frames();
    let header = bcf1_header(2, 24 + data.len() as u32);
    op.write(STREAM, mstr(TAG, &header, &data)).await.unwrap();
}

async fn open(op: &Operator, path: &str) -> respak_core::Result<Value> {
    let cx = cx_on(op);
    cx.mediator.open(path, &cx).await
}

async fn wav(op: &Operator, path: &str, track: u32) -> respak_core::Result<Vec<u8>> {
    let cx = cx_on(op);
    let source = cx.mediator.open(path, &cx).await?;
    let to = Target {
        format: FormatId::new("wav"),
        options: serde_json::json!({ "track": track }),
        via: Vec::new(),
    };
    let out = cx.mediator.convert(source, to, &cx).await?;
    let blob = out.as_blob().expect("a wav blob");
    Ok(blob
        .read(0..blob.len().unwrap_or(u64::MAX), &cx)
        .await?
        .to_vec())
}

fn samples(wav: &[u8]) -> Vec<i16> {
    wav[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[tokio::test]
async fn a_bank_is_detected_and_listed_without_its_stream() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    let engine = Engine::builder()
        .input(op.clone())
        .output(mem())
        .plugin(FormatsPlugin)
        .plugin(RadPlugin)
        .build()
        .unwrap();
    let d = engine
        .discover(BANK, &Default::default(), &CancellationToken::new())
        .await
        .unwrap();
    let e = d.entries.into_iter().next().unwrap();
    assert_eq!(
        (e.format.as_deref(), e.version.as_deref()),
        (Some("mbnk"), Some("49"))
    );
    // 12 for the magic, 2 for `fileSize` matching the length, 1 for the extension is not counted
    assert!(e.score >= 14, "{}", e.score);

    let cx = cx_on(&op);
    let bank = cx.mediator.open(BANK, &cx).await.unwrap();
    let bank = cx.mediator.mount(bank, &cx).await.unwrap();
    let volume = bank.as_container().unwrap().volume();
    let names: Vec<_> = volume
        .list("vo/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name().to_owned())
        .collect();
    assert_eq!(names, ["line.asrc"], "listing needs no .mstr");
}

#[tokio::test]
async fn a_stereo_source_is_two_mono_tracks_converted_to_wav() {
    let op = mem();
    put_stereo_bank(&op).await;
    let source = open(&op, SOURCE).await.unwrap();
    assert_eq!(source.format().name(), "asrc");
    let tracks = source.as_audio().unwrap().tracks();
    assert_eq!(
        tracks
            .iter()
            .map(|t| (t.channels, t.sample_rate))
            .collect::<Vec<_>>(),
        [(1, 48000), (1, 48000)]
    );
    assert_eq!(source.as_metadata().unwrap().name(), Some("vo/line"));

    let left = samples(&wav(&op, SOURCE, 0).await.unwrap());
    assert_eq!(
        left.len(),
        2000,
        "the last frame is trimmed to the header's frame count"
    );
    assert!(left[0] > 0, "the DC coefficient comes through");
    assert!(
        left[..1920].iter().all(|&s| s == left[0]),
        "a DC input is constant"
    );
    assert!(
        left[1999] < left[0],
        "the second, silent block fades the first out over the overlap"
    );
    let right = samples(&wav(&op, SOURCE, 1).await.unwrap());
    assert!(
        right.len() == 2000 && right.iter().all(|&s| s == 0),
        "the silent channel stays silent"
    );

    let w = wav(&op, SOURCE, 0).await.unwrap();
    assert_eq!(&w[..4], b"RIFF");
    assert_eq!(
        u16::from_le_bytes([w[22], w[23]]),
        1,
        "one channel per file"
    );
    assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 48000);
}

#[tokio::test]
async fn a_track_past_the_channels_is_an_error() {
    let op = mem();
    put_stereo_bank(&op).await;
    let err = wav(&op, SOURCE, 2).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::InvalidOptions { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn a_missing_stream_is_described_but_cannot_be_read() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    // describing (what `discover` does) works and warns about nothing
    let source = open(&op, SOURCE).await.unwrap();
    assert_eq!(source.format().name(), "asrc");
    assert_eq!(source.as_metadata().unwrap().name(), Some("vo/line"));
    assert_eq!(
        source.as_audio().unwrap().tracks().len(),
        1,
        "one placeholder track"
    );
    // reading it names the missing file
    let err = wav(&op, SOURCE, 0).await.unwrap_err();
    match err.kind() {
        ErrorKind::MissingDependency { file, .. } => assert_eq!(file, STREAM),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_stream_of_another_build_or_patch_is_corrupt() {
    let op = mem();
    put_stereo_bank(&op).await;
    let data = frames();
    let header = bcf1_header(2, 24 + data.len() as u32);
    op.write(STREAM, mstr(TAG + 1, &header, &data))
        .await
        .unwrap();
    let err = open(&op, SOURCE).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn an_unknown_codec_and_a_short_codec_stream() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    op.write(STREAM, mstr(TAG, b"OGGSOGGSOGGSOGGSOGGSOGGS", &[]))
        .await
        .unwrap();
    let err = open(&op, SOURCE).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { .. }),
        "{err}"
    );
    // the stream claims to be shorter than its own header
    op.write(STREAM, mstr(TAG, &bcf1_header(2, 10), &[]))
        .await
        .unwrap();
    let err = open(&op, SOURCE).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

#[tokio::test]
async fn a_damaged_frame_yields_what_came_before_then_an_error() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    let mut data = frames();
    let second = 4 + 1024;
    data[second] = 0; // the sync word of frame 2
    let header = bcf1_header(2, 24 + data.len() as u32);
    op.write(STREAM, mstr(TAG, &header, &data)).await.unwrap();
    let err = wav(&op, SOURCE, 0).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Corrupt { .. }), "{err}");
}

/// A RAD Audio header (48 kHz, stereo, 1000 frames) and nothing else.
fn rada_header() -> Vec<u8> {
    let mut h = vec![0u8; 40];
    h[..4].copy_from_slice(b"ADAR");
    h[8] = 1;
    h[9] = 2;
    h[0x0F] = 3;
    h[0x10..0x18].copy_from_slice(&1000u64.to_le_bytes());
    h[0x18..0x20].copy_from_slice(&40u64.to_le_bytes());
    h
}

#[tokio::test]
async fn rada_reports_its_shape_but_cannot_be_decoded() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(40)], 1, 0))
        .await
        .unwrap();
    op.write(STREAM, mstr(TAG, &rada_header(), &[]))
        .await
        .unwrap();
    let source = open(&op, SOURCE).await.unwrap();
    assert_eq!(source.as_audio().unwrap().tracks().len(), 2);
    let err = wav(&op, SOURCE, 0).await.unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::UnsupportedVersion { format, .. } if format == "rada"),
        "{err}"
    );
}

#[tokio::test]
async fn cancelling_ends_the_stream_with_cancelled() {
    use futures::StreamExt;
    let op = mem();
    put_stereo_bank(&op).await;
    let cx = cx_on(&op);
    let source = cx.mediator.open(SOURCE, &cx).await.unwrap();
    cx.cancel.cancel();
    let mut stream = source.as_audio().unwrap().samples(0, &cx).await.unwrap();
    let first = stream.next().await.expect("an item");
    assert!(matches!(first.unwrap_err().kind(), ErrorKind::Cancelled));
}

#[tokio::test]
async fn random_bytes_after_a_valid_header_never_panic() {
    let op = mem();
    op.write(BANK, bank(49, TAG, "general", &[line(24)], 1, 0))
        .await
        .unwrap();
    let noise: Vec<u8> = (0..4000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect();
    let header = bcf1_header(2, 24 + noise.len() as u32);
    op.write(STREAM, mstr(TAG, &header, &noise)).await.unwrap();
    assert!(wav(&op, SOURCE, 0).await.is_err());
}
