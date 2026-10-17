//! `respak dump`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use opendal::Operator;
use respak_core::{CancellationToken, Engine};
use respak_respawn::RpakAsset;

/// Bytes read from the pak per request.
const CHUNK: u64 = 1 << 20;

/// Arguments of `dump`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct DumpArgs {
    /// Asset path or glob inside a pak, relative to `--root`, e.g. `paks/Win64/common.rpak/0x1234.txtr`.
    path: String,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    out_dir: Option<PathBuf>,
    args: DumpArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let out_dir = out_dir.ok_or_else(|| anyhow::anyhow!("--out is required"))?;
    let mut failed = false;
    for path in engine.expand(&args.path, cancel).await? {
        let value = match engine.open(&path, cancel).await {
            Ok(mut values) if values.len() == 1 => values.remove(0),
            Ok(_) => continue,
            Err(error) => {
                writeln!(err, "{path}: {error}")?;
                failed = true;
                continue;
            }
        };
        let Some(asset) = value.downcast_ref::<RpakAsset>() else {
            writeln!(err, "{path}: not a pak asset (use `copy` for files)")?;
            failed = true;
            continue;
        };
        // The header, then the streamed data under the volume's own names (`.stream`, `.opt`).
        let parts = [
            (path.clone(), Some(&asset.header_path)),
            (format!("{path}.stream"), asset.stream.as_ref()),
            (format!("{path}.opt"), asset.opt.as_ref()),
        ];
        for (target, source) in parts {
            let Some(source) = source else { continue };
            match dump_part(&asset.volume, source, &out_dir.join(&target), cancel).await {
                Ok(()) => writeln!(out, "{target}")?,
                Err(error) => {
                    writeln!(err, "{target}: {error}")?;
                    failed = true;
                }
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Copies `source` of `volume` to the file `dest`, in chunks.
async fn dump_part(
    volume: &Operator,
    source: &str,
    dest: &Path,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let len = volume.stat(source).await?.content_length();
    let mut file = std::io::BufWriter::new(std::fs::File::create(dest)?);
    let mut at = 0;
    while at < len {
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        let end = (at + CHUNK).min(len);
        file.write_all(&volume.read_with(source).range(at..end).await?.to_bytes())?;
        at = end;
    }
    file.flush()?;
    Ok(())
}
