//! What the commands that write files (`convert`, `copy`, `run`) share.

use std::io::Write;
use std::process::ExitCode;

use respak_core::{ConvertReport, ErrorKind, OutputResult, TargetSpec};

/// `{"type":"file"}`: the source unchanged.
pub(super) fn file_target() -> TargetSpec {
    TargetSpec {
        format: respak_core::FormatId::from_static("file"),
        options: Default::default(),
        via: Default::default(),
    }
}

/// Drops the entries that only say a source is not a file (an asset inside a pak has no bytes of
/// its own to copy), so copying a glob that reaches into paks copies the real files and goes on.
pub(super) fn skip_assets(report: &mut ConvertReport) {
    report.outputs.retain(|o| {
        !matches!(&o.result, OutputResult::Err { error } if matches!(error.kind(), ErrorKind::NoPath { to, .. } if to == "file"))
    });
}

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
