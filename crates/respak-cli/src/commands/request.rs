//! `respak run`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, ConvertRequest, Engine, Error, ErrorKind};

use super::output::report;

/// Arguments of `run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Path of the request JSON, or `-`.
    request: String,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: RunArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let text = if args.request == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(&args.request).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest {
                message: format!("cannot read {}: {e}", args.request),
            })
        })?
    };
    let request: ConvertRequest = serde_json::from_str(&text).map_err(|e| {
        Error::new(ErrorKind::InvalidRequest {
            message: format!("the request is not valid: {e}"),
        })
    })?;
    report(engine.convert(&request, cancel).await?, out, err)
}
