//! `respak formats`.

use std::io::Write;
use std::process::ExitCode;

use respak_core::Engine;

/// Prints the format graph as JSON.
pub(crate) fn run(engine: &Engine, out: &mut impl Write) -> anyhow::Result<ExitCode> {
    serde_json::to_writer_pretty(&mut *out, &engine.formats())?;
    writeln!(out)?;
    Ok(ExitCode::SUCCESS)
}
