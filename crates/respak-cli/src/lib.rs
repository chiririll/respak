//! respak-cli library: command implementations.
// Sketch stage (task 05): bodies are todo!().
#![allow(unused_variables, dead_code)]

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use respak_core::Engine;

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

/// Runs a command; the JSON report goes to `out`. Non-zero exit if any output failed
/// (the error is also written to stderr).
pub async fn run(cli: Cli, engine: &Engine, out: &mut impl Write) -> anyhow::Result<ExitCode> {
    todo!()
}
