//! `respak convert`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{
    CancellationToken, ConvertRequest, Engine, Error, ErrorKind, OutputRequest, TargetSpec,
};

use super::output::report;

/// Arguments of `convert`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ConvertArgs {
    /// Source path or glob, relative to `--root`.
    path: String,
    /// Target as JSON, e.g. `{"type":"dds"}`.
    #[arg(long)]
    to: String,
    /// Output path, relative to `--out`.
    #[arg(long = "path")]
    out_path: Option<String>,
    /// Also copy each source file to `--out` unchanged, like `copy`.
    #[arg(long)]
    copy: bool,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: ConvertArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let to: TargetSpec = serde_json::from_str(&args.to).map_err(|e| {
        Error::new(ErrorKind::InvalidRequest {
            message: format!("--to is not a valid target: {e}"),
        })
    })?;
    let mut outputs = vec![OutputRequest {
        from: args.path.clone().into(),
        to,
        path: args.out_path,
        skip_unsupported: false,
    }];
    if args.copy {
        // An asset inside a pak has no bytes of its own to copy: skip it, not fail.
        outputs.push(OutputRequest {
            from: args.path.into(),
            to: TargetSpec::file(),
            path: None,
            skip_unsupported: true,
        });
    }
    let done = engine
        .convert(&ConvertRequest::new(outputs), cancel)
        .await?;
    report(done, out, err)
}
