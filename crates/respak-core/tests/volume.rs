//! The generic `VolumeSource` -> `Operator` adapter, on a toy tree.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use opendal::ErrorKind as OdKind;
use respak_core::volume::{volume_operator, VolumeEntry, VolumeSource, VolumeStat};
use respak_core::{Error, ErrorKind, Result};

type ReadLog = Arc<Mutex<Vec<(String, Range<u64>)>>>;

/// `/hello.txt`, `/dir/a.bin`, `/dir/sub/b.bin`; records every ranged read.
struct Toy {
    reads: ReadLog,
}

const FILES: &[(&str, &[u8])] = &[
    ("hello.txt", b"hello world"),
    ("dir/a.bin", b"0123456789"),
    ("dir/sub/b.bin", b"xyz"),
];

#[async_trait]
impl VolumeSource for Toy {
    async fn stat(&self, path: &str) -> Result<VolumeStat> {
        if path.is_empty()
            || FILES
                .iter()
                .any(|(f, _)| f.starts_with(&format!("{path}/")))
        {
            return Ok(VolumeStat {
                is_dir: true,
                len: 0,
            });
        }
        match FILES.iter().find(|(f, _)| *f == path) {
            Some((_, data)) => Ok(VolumeStat {
                is_dir: false,
                len: data.len() as u64,
            }),
            None => Err(Error::new(ErrorKind::NotFound {
                path: path.to_owned(),
            })),
        }
    }

    async fn read(&self, path: &str, range: Range<u64>) -> Result<Bytes> {
        self.reads
            .lock()
            .unwrap()
            .push((path.to_owned(), range.clone()));
        let (_, data) = FILES.iter().find(|(f, _)| *f == path).unwrap();
        Ok(Bytes::copy_from_slice(
            &data[range.start as usize..range.end as usize],
        ))
    }

    async fn list(&self, dir: &str) -> Result<Vec<VolumeEntry>> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<VolumeEntry> = Vec::new();
        for (f, data) in FILES.iter().filter(|(f, _)| f.starts_with(&prefix)) {
            let rest = &f[prefix.len()..];
            let (name, is_dir) = match rest.split_once('/') {
                Some((d, _)) => (d, true),
                None => (rest, false),
            };
            if !out.iter().any(|e| e.name == name) {
                out.push(VolumeEntry {
                    name: name.to_owned(),
                    is_dir,
                    len: if is_dir { 0 } else { data.len() as u64 },
                });
            }
        }
        Ok(out)
    }
}

fn toy() -> (opendal::Operator, ReadLog) {
    let reads = Arc::new(Mutex::new(Vec::new()));
    (
        volume_operator(
            Toy {
                reads: reads.clone(),
            },
            "toy",
        ),
        reads,
    )
}

#[tokio::test]
async fn stat_files_dirs_and_missing_paths() {
    let (op, _) = toy();
    let file = op.stat("dir/a.bin").await.unwrap();
    assert!(file.is_file());
    assert_eq!(file.content_length(), 10);
    assert!(op.stat("dir/").await.unwrap().is_dir());
    assert!(op.stat("/").await.unwrap().is_dir());
    assert_eq!(
        op.stat("nope.bin").await.unwrap_err().kind(),
        OdKind::NotFound
    );
    assert!(op.exists("hello.txt").await.unwrap());
    assert!(!op.exists("nope.bin").await.unwrap());
}

#[tokio::test]
async fn reads_are_ranged_and_clamped_to_the_requested_bytes() {
    let (op, reads) = toy();
    assert_eq!(op.read("hello.txt").await.unwrap().to_vec(), b"hello world");
    assert_eq!(
        op.read_with("dir/a.bin")
            .range(2..5)
            .await
            .unwrap()
            .to_vec(),
        b"234"
    );
    assert_eq!(
        op.read_with("dir/a.bin").range(7..).await.unwrap().to_vec(),
        b"789"
    );
    // Like OpenDAL's own services: an explicit range past the end is an error, so callers clamp.
    assert_eq!(
        op.read_with("dir/a.bin")
            .range(8..100)
            .await
            .unwrap_err()
            .kind(),
        OdKind::RangeNotSatisfied
    );

    let log = reads.lock().unwrap();
    assert_eq!(
        log[1],
        ("dir/a.bin".to_owned(), 2..5),
        "only the asked bytes reach the source"
    );
    assert_eq!(log[2], ("dir/a.bin".to_owned(), 7..10));
}

#[tokio::test]
async fn reading_a_missing_file_or_a_directory_fails() {
    let (op, _) = toy();
    assert_eq!(
        op.read("nope.bin").await.unwrap_err().kind(),
        OdKind::NotFound
    );
    assert!(op.read("dir/").await.is_err());
}

#[tokio::test]
async fn lists_direct_children_only() {
    let (op, _) = toy();
    let mut root: Vec<String> = op
        .list("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path().to_owned())
        .collect();
    root.retain(|p| p != "/");
    assert_eq!(root, ["hello.txt", "dir/"]);

    let dir: Vec<String> = op
        .list("dir/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path().to_owned())
        .collect();
    assert_eq!(
        dir.iter()
            .filter(|p| p.as_str() != "dir/")
            .cloned()
            .collect::<Vec<_>>(),
        ["dir/a.bin", "dir/sub/"]
    );
}

#[tokio::test]
async fn a_volume_is_read_only() {
    let (op, _) = toy();
    assert!(op.write("x", "y").await.is_err());
    assert!(op.delete("hello.txt").await.is_err());
}
