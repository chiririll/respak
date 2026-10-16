//! One file per command: its arguments (where it has any) and its `run`.

mod convert;
mod copy;
mod discover;
mod formats;
mod list;
mod output;
mod request;
#[cfg(feature = "steam")]
pub(crate) mod steam;

pub use convert::ConvertArgs;
pub use copy::CopyArgs;
pub use discover::DiscoverArgs;
pub use list::ListArgs;
pub use request::RunArgs;

pub(crate) use convert::run as convert;
pub(crate) use copy::run as copy;
pub(crate) use discover::run as discover;
pub(crate) use formats::run as formats;
pub(crate) use list::run as list;
pub(crate) use request::run as request;
