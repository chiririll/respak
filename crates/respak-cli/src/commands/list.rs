//! `respak list`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, Engine};

use super::failures::Failures;

/// Arguments of `list`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct ListArgs {
    /// Container paths relative to `--root`.
    #[arg(required = true)]
    paths: Vec<String>,
    /// Read every entry and print its name after the path (tab-separated), where it has one.
    /// Slow: an rpak asset's name is stored in its header, and only some kinds can be read.
    #[arg(long, conflicts_with = "names")]
    map: bool,
    /// Read every entry and print only the names, only of entries that have one (slow, see `--map`).
    #[arg(long)]
    names: bool,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: ListArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let names = args.names;
    let mut failures = Failures::default();
    for path in &args.paths {
        match engine.list(path, args.map || names, cancel).await {
            Ok(entries) => {
                for entry in entries {
                    match (entry.name, names) {
                        (Some(name), true) => writeln!(out, "{name}")?,
                        (None, true) => {}
                        (Some(name), false) => writeln!(out, "{}\t{name}", entry.path)?,
                        (None, false) => writeln!(out, "{}", entry.path)?,
                    }
                }
            }
            Err(error) => {
                failures.report(err, path, error)?;
            }
        }
    }
    Ok(failures.exit_code())
}
