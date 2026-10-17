//! `respak discover`.

use std::io::Write;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, DiscoverOptions, Engine};

use super::failures::Failures;

/// Arguments of `discover`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct DiscoverArgs {
    /// Paths relative to `--root`.
    #[arg(required = true)]
    paths: Vec<String>,
    /// How many container levels to open (1 lists a pak's assets).
    #[arg(long, default_value_t = 0)]
    depth: u32,
    /// Read every entry to fill in its name where it is not known without decoding (slow,
    /// like `list --names`).
    #[arg(long)]
    names: bool,
}

pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: DiscoverArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let options = DiscoverOptions {
        depth: args.depth,
        names: args.names,
    };
    let mut found = Vec::new();
    let mut failures = Failures::default();
    for path in &args.paths {
        match engine.discover(path, &options, cancel).await {
            Ok(discovery) => found.push(discovery),
            Err(error) => {
                failures.report(err, path, error)?;
            }
        }
    }
    serde_json::to_writer_pretty(&mut *out, &found)?;
    writeln!(out)?;
    Ok(failures.exit_code())
}
