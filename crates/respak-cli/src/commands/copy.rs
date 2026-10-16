//! `respak copy`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, ConvertRequest, Engine, OutputRequest};

use super::output::{file_target, report, skip_assets};

/// Arguments of `copy`.
#[derive(Debug, Args)]
pub struct CopyArgs {
    /// Source path or glob, relative to `--root` (or the Steam depots).
    path: String,
    /// Output path, relative to `--out`; default: the source path.
    #[arg(long = "path")]
    out_path: Option<String>,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: CopyArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let outputs = vec![OutputRequest {
        from: args.path.into(),
        to: file_target(),
        path: args.out_path,
    }];
    let mut done = engine
        .convert(
            &ConvertRequest {
                version: 1,
                outputs,
            },
            cancel,
        )
        .await?;
    skip_assets(&mut done);
    report(done, out, err)
}
