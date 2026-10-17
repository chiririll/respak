//! `mbnk`: a Miles sound bank, opened as a volume of sources.
//!
//! # The volume
//! ```text
//! general.mbnk/
//!   <source name>.asrc            non-localised source
//!   <source name>.<lang>.asrc     localised source (language from the per-version table)
//! ```
//! Every source is listed; a missing `.mstr` shows up only when the source is opened. A source
//! name with `/` in it becomes folders.
//!
//! # Cost
//! The whole `.mbnk` is read (banks are small next to the streams). Nothing of any `.mstr` is
//! touched while listing.

mod bank;
mod entry;
#[cfg(test)]
pub(crate) mod fixtures;
mod handler;
mod layout;
mod source;
#[cfg(test)]
mod tests;

pub use bank::Bank;
pub use entry::SourceEntry;
pub use handler::{MbnkData, MbnkRead};
pub use layout::BankLayout;

use respak_core::{Error, ErrorKind};

fn not_found(path: &str) -> Error {
    Error::new(ErrorKind::NotFound {
        path: path.to_owned(),
    })
}
