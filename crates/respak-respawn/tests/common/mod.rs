//! Test helpers: synthetic paks and an engine over memory operators.
#![allow(dead_code)]

use opendal::{services::Memory, Operator};
use respak_core::Engine;
use respak_respawn::RespawnPlugin;

pub use respak_respawn::testing::*;

pub struct Env {
    pub engine: Engine,
    pub input: Operator,
}

pub fn env() -> Env {
    let input = Operator::new(Memory::default()).unwrap();
    let output = Operator::new(Memory::default()).unwrap();
    let engine = Engine::builder()
        .input(input.clone())
        .output(output)
        .plugin(RespawnPlugin::new())
        .build()
        .unwrap();
    Env { engine, input }
}
