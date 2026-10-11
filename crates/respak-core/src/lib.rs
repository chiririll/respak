//! respak-core: mediator, format graph, capabilities, volumes and the engine.

pub mod capability;
pub mod error;
pub mod value;

pub use error::{Error, ErrorKind, Result};
pub use value::{Data, FormatId, Value, ValueKey, Version};
