//! The `respak` binary: builds Fs operators from `--root`/`--out`, registers plugins, calls `run`.

use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use opendal::{services::Fs, Operator};
use respak_cli::{run_with_events, Cli};
use respak_core::{CancellationToken, Engine};
use respak_formats::FormatsPlugin;
use respak_nihav::NihavPlugin;
use respak_respawn::RespawnPlugin;

fn fs(root: &std::path::Path) -> anyhow::Result<Operator> {
    std::fs::create_dir_all(root)?;
    Ok(Operator::new(Fs::default().root(&root.to_string_lossy()))?)
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    // Logs go to stderr; `RUST_LOG=info` shows what Steam is doing, the default is warnings only.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    #[cfg(feature = "steam")]
    if let respak_cli::Command::Steam { command } = cli.command {
        return respak_cli::run_steam(command, &mut std::io::stdout()).await;
    }
    let Some(out) = &cli.out else {
        anyhow::bail!("--out is required");
    };
    // The first Ctrl-C asks the call to stop and report what finished; a second one aborts.
    let cancel = CancellationToken::new();
    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("cancelling... (Ctrl-C again to abort)");
                cancel.cancel();
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(130);
                }
            }
        }
    });
    #[cfg(feature = "steam")]
    let mut steam_input = None;
    #[cfg(feature = "steam")]
    let input = if let Some(app) = cli.steam {
        let mut options = respak_steam::SteamOptions::new(app, cli.depot.clone(), ".respak");
        options.branch = cli.branch.clone();
        if let Some(cap) = cli.steam_cache_size {
            options.cache_cap = cap;
        }
        let events = respak_core::Events::new(respak_core::event::DEFAULT_EVENT_CAPACITY);
        let mut rx = events.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if let Some(line) = respak_cli::event_line(&event) {
                    eprintln!("{line}");
                }
            }
        });
        let opened = respak_steam::open(options, &cancel, &events).await?;
        let operator = opened.operator.clone();
        steam_input = Some(opened);
        operator
    } else {
        let root = cli.root.as_ref().ok_or_else(|| anyhow::anyhow!("--root (or --steam) is required"))?;
        fs(root)?
    };
    #[cfg(not(feature = "steam"))]
    let input = fs(cli.root.as_ref().ok_or_else(|| anyhow::anyhow!("--root is required"))?)?;
    let mut builder = Engine::builder()
        .input(input)
        .output(fs(out)?)
        .plugin(FormatsPlugin)
        .plugin(NihavPlugin)
        .plugin(RespawnPlugin::new().search(cli.search.clone()));
    if let Some(dir) = &cli.cache {
        builder = builder.cache(fs(dir)?);
    }
    let engine = builder.build()?;
    let mut report: Box<dyn std::io::Write> = match &cli.report {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            Box::new(std::io::BufWriter::new(std::fs::File::create(path)?))
        }
        None => Box::new(std::io::stdout()),
    };
    let code = run_with_events(cli, &engine, &cancel, &mut report, &mut std::io::stderr(), &mut std::io::stderr()).await?;
    report.flush()?;
    drop(report);
    #[cfg(feature = "steam")]
    if let Some(steam) = steam_input {
        let s = steam.stats();
        eprintln!(
            "steam: {} chunks downloaded ({} bytes), {} served from the cache",
            s.chunks_downloaded, s.bytes_downloaded, s.cache_hits
        );
    }
    Ok(code)
}
