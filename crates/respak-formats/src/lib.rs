//! respak-formats: common formats (png, dds). One plugin, no containers.
// Sketch stage (task 05): bodies are todo!().
#![allow(unused_variables, dead_code)]

mod dds;
mod png;

use respak_core::{Plugin, Registry};

/// Registers `png` and `dds`.
pub struct FormatsPlugin;

impl Plugin for FormatsPlugin {
    fn id(&self) -> &str {
        "formats"
    }

    fn register(&self, r: &mut Registry) {
        todo!()
    }
}
