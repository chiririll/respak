//! respak-host: what the hosts of the engine (the CLI, the web UI) share. It reads and writes
//! `respak.toml`, merges it with the host's own overrides into [`Settings`], and builds the
//! input and the [`Engine`](respak_core::Engine) from them. No command line, no HTTP.

mod config;
mod settings;
mod setup;
mod size;

pub use config::{
    anchor_all, find_up, load, load_file, save, Config, ConfigSelect, DepotEntry, RespawnConfig,
    SteamConfig, DEFAULT_FILE,
};
#[cfg(feature = "steam")]
pub use settings::SteamSource;
pub use settings::{Overrides, Settings, Source};
pub use setup::{build_engine, build_engine_with, fs, open_input, OpenedInput};
pub use size::{format_size, parse_size};
