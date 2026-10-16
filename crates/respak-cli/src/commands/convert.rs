//! `respak convert`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{
    CancellationToken, ConvertRequest, Engine, Error, ErrorKind, OutputRequest, TargetSpec,
};

use super::output::{file_target, report, skip_assets};

/// Arguments of `convert`.
#[derive(Debug, Args)]
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
    }];
    if args.copy {
        outputs.push(OutputRequest {
            from: args.path.into(),
            to: file_target(),
            path: None,
        });
    }
    let mut done = engine
        .convert(
            &ConvertRequest {
                version: 1,
                outputs,
            },
            cancel,
        )
        .await?;
    if args.copy {
        skip_assets(&mut done);
    }
    report(done, out, err)
}
