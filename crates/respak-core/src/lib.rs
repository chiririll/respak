//! respak-core: mediator, format graph, capabilities, volumes and the engine.


pub mod capability;
pub mod detect;
pub mod discover;
pub mod engine;
pub mod error;
pub mod glob;
pub mod graph;
pub mod mediator;
pub mod plugin;
pub mod resolve;
pub mod target;
pub mod value;
pub mod volume;

pub use capability::Cap;
pub use detect::{magic, u16le, u32le, within, ConvertHandler, Detect, Detected, Probe, ReadHandler};
pub use discover::{Alternative, DiscoverOptions, Discovery, DiscoveryEntry};
pub use engine::{
    ConvertReport, ConvertRequest, Engine, EngineBuilder, FromSpec, OutputReport, OutputRequest, OutputResult,
    SourceSpec, TargetSpec,
};
pub use error::{Error, ErrorKind, Result};
pub use graph::{EdgeInfo, FormatGraph, FormatInfo, Path, Step, TargetInfo};
pub use mediator::{Cx, Mediator, Target};
pub use plugin::{CapId, FormatBuilder, Plugin, Registry};
pub use resolve::Mounts;
pub use target::TypedTarget;
pub use value::{Data, FormatId, Value, ValueKey, Version};
