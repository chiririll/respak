//! The `respak` binary: builds Fs operators from `--root`/`--out`, registers plugins, calls `run`.

use std::process::ExitCode;

use clap::Parser;
use opendal::{services::Fs, Operator};
use respak_cli::{run, Cli};
use respak_core::Engine;
use respak_formats::FormatsPlugin;
use respak_respawn::RespawnPlugin;

fn fs(root: &std::path::Path) -> anyhow::Result<Operator> {
    std::fs::create_dir_all(root)?;
    Ok(Operator::new(Fs::default().root(&root.to_string_lossy()))?)
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let engine = Engine::builder().input(fs(&cli.root)?).output(fs(&cli.out)?).plugin(FormatsPlugin).plugin(RespawnPlugin::new()).build()?;
    run(cli, &engine, &mut std::io::stdout(), &mut std::io::stderr()).await
}
