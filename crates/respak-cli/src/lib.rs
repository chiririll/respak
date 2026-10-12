//! respak-cli library: command implementations.
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use respak_core::{ConvertReport, ConvertRequest, DiscoverOptions, Engine, Error, ErrorKind, OutputRequest, OutputResult, TargetSpec};

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

/// Commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the format graph as JSON.
    Formats,
    /// Print what each path is and which formats it can be converted to, as JSON.
    Discover {
        /// Paths relative to `--root`.
        #[arg(required = true)]
        paths: Vec<String>,
        /// How many container levels to open (1 lists a pak's assets).
        #[arg(long, default_value_t = 0)]
        depth: u32,
    },
    /// Run a request file (`-` reads stdin): any number of outputs, with globs and lists.
    Run {
        /// Path of the request JSON, or `-`.
        request: String,
    },
    /// Convert a file, or every file a glob matches.
    Convert {
        /// Source path or glob, relative to `--root`.
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
        Command::Formats => {
            serde_json::to_writer_pretty(&mut *out, &engine.formats())?;
            writeln!(out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Discover { paths, depth } => {
            let options = DiscoverOptions { depth };
            let mut found = Vec::new();
            let mut failed = false;
            for path in &paths {
                match engine.discover(path, &options).await {
                    Ok(discovery) => found.push(discovery),
                    Err(error) => {
                        writeln!(err, "{path}: {error}")?;
                        failed = true;
                    }
                }
            }
            serde_json::to_writer_pretty(&mut *out, &found)?;
            writeln!(out)?;
            Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Convert { path, to, out_path } => {
            let to: TargetSpec = serde_json::from_str(&to).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("--to is not a valid target: {e}") })
            })?;
            let request =
                ConvertRequest { version: 1, outputs: vec![OutputRequest { from: path.into(), to, path: out_path }] };
            report(engine.convert(&request).await?, out, err)
        }
        Command::Run { request } => {
            let text = if request == "-" {
                std::io::read_to_string(std::io::stdin())?
            } else {
                std::fs::read_to_string(&request).map_err(|e| {
                    Error::new(ErrorKind::InvalidRequest { message: format!("cannot read {request}: {e}") })
                })?
            };
            let request: ConvertRequest = serde_json::from_str(&text).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("the request is not valid: {e}") })
            })?;
            report(engine.convert(&request).await?, out, err)
        }
    }
}

/// Prints the report as JSON, each failure on `err`; non-zero exit if any output failed.
fn report(report: ConvertReport, out: &mut impl Write, err: &mut impl Write) -> anyhow::Result<ExitCode> {
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
