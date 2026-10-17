//! `respak dump`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::value::FileData;
use respak_core::{CancellationToken, Engine};
use respak_respawn::RpakAsset;

use super::failures::Failures;

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
    args: DumpArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let mut failures = Failures::default();
    for path in engine.expand(&args.path, cancel).await? {
        let value = match engine.open(&path, cancel).await {
            Ok(mut values) if values.len() == 1 => values.remove(0),
            Ok(values) => {
                failures.report(
                    err,
                    &path,
                    format!("expected one value, got {}", values.len()),
                )?;
                continue;
            }
            Err(error) => {
                failures.report(err, &path, error)?;
                continue;
            }
        };
        let Some(asset) = value.downcast_ref::<RpakAsset>() else {
            failures.report(err, &path, "not a pak asset (use `copy` for files)")?;
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
            let data = FileData::new(asset.volume.clone(), source.as_str(), target.as_str(), None);
            match engine.write(&target, &data, cancel).await {
                Ok(_) => writeln!(out, "{target}")?,
                Err(error) => failures.report(err, &target, error)?,
            }
        }
    }
    Ok(failures.exit_code())
}
