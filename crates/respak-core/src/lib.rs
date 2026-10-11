//! respak-core: mediator, format graph, capabilities, volumes and the engine.


pub mod capability;
pub mod detect;
pub mod engine;
pub mod error;
pub mod mediator;
pub mod plugin;
pub mod value;

pub use capability::Cap;
pub use detect::{magic, ConvertHandler, Detect, Detected, Probe, ReadHandler};
pub use engine::{
    ConvertReport, ConvertRequest, Engine, EngineBuilder, OutputReport, OutputRequest, OutputResult, TargetSpec,
};
pub use error::{Error, ErrorKind, Result};
pub use mediator::{Cx, Mediator, Target};
pub use plugin::{CapId, FormatBuilder, Plugin, Registry};
pub use value::{Data, FormatId, Value, ValueKey, Version};
