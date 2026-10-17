//! What the commands that write files (`convert`, `copy`, `run`) share.

use std::io::Write;
use std::process::ExitCode;

use respak_core::{ConvertReport, OutputResult};

/// Prints the report as JSON, each failure on `err`; non-zero exit if any output failed.
pub(super) fn report(
    report: ConvertReport,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    serde_json::to_writer_pretty(&mut *out, &report)?;
    writeln!(out)?;
    let mut failed = false;
    for output in &report.outputs {
        if let OutputResult::Err { error } = &output.result {
            writeln!(err, "{}: {error}", output.from)?;
            failed = true;
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
