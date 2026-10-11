//! respak-cli library: command implementations.
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use respak_core::{ConvertRequest, Engine, Error, ErrorKind, OutputRequest, OutputResult, TargetSpec};

/// `respak --root <dir> --out <dir> <command>`
#[derive(Debug, Parser)]
pub struct Cli {
    /// Input root directory.
    #[arg(long)]
    pub root: PathBuf,
    /// Output directory.
    #[arg(long)]
    pub out: PathBuf,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Commands. (`formats`, `discover`, `run` come in later slices.)
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Convert one file.
    Convert {
        /// Source path, relative to `--root`.
        path: String,
        /// Target as JSON, e.g. `{"type":"dds"}`.
        #[arg(long)]
        to: String,
        /// Output path, relative to `--out`.
        #[arg(long = "path")]
        out_path: Option<String>,
    },
}

/// Runs a command; the JSON report goes to `out`, error messages to `err`. The exit code is
/// non-zero if any output failed. An unusable command line (malformed `--to`) is an `Err`.
pub async fn run(
    cli: Cli,
    engine: &Engine,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Convert { path, to, out_path } => {
            let to: TargetSpec = serde_json::from_str(&to).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("--to is not a valid target: {e}") })
            })?;
            let request =
                ConvertRequest { version: 1, outputs: vec![OutputRequest { from: path, to, path: out_path }] };
            let report = engine.convert(&request).await?;
            serde_json::to_writer_pretty(&mut *out, &report)?;
            writeln!(out)?;

            let mut failed = false;
            for output in &report.outputs {
                if let OutputResult::Err { error } = &output.result {
                    writeln!(err, "{}: {error}", output.from)?;
                    failed = true;
                }
            }
            Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
    }
}
