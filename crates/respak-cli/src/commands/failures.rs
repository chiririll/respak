//! Failures of the items a command works through: each is printed, and any of them makes the
//! exit code non-zero.

use std::fmt::Display;
use std::io::Write;
use std::process::ExitCode;

/// Whether any item failed.
#[derive(Debug, Default)]
pub(super) struct Failures(bool);

impl Failures {
    /// Prints `<what>: <error>` on `err` and remembers the failure.
    pub(super) fn report(
        &mut self,
        err: &mut impl Write,
        what: impl Display,
        error: impl Display,
    ) -> std::io::Result<()> {
        self.0 = true;
        writeln!(err, "{what}: {error}")
    }

    /// Success, unless something was reported.
    pub(super) fn exit_code(self) -> ExitCode {
        if self.0 {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }
}
