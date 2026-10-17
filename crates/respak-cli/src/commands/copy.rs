//! `respak copy`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, ConvertRequest, Engine, OutputRequest, TargetSpec};

use super::output::report;

/// Arguments of `copy`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
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
        to: TargetSpec::file(),
        path: args.out_path,
        // An asset inside a pak has no bytes of its own to copy: skip it, not fail, so a glob
        // that reaches into paks copies the real files and goes on.
        skip_unsupported: true,
    }];
    let done = engine
        .convert(&ConvertRequest::new(outputs), cancel)
        .await?;
    report(done, out, err)
}
