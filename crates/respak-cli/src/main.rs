//! The `respak` binary: reads the command line and the config file, builds the engine, calls `run`.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use respak_cli::{build_engine, load_config, open_input, run_with_events, Cli, Settings};
use respak_core::CancellationToken;

/// Logs go to stderr; `RUST_LOG=info` shows what Steam is doing, the default is warnings only.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
}

/// The first Ctrl-C asks the call to stop and report what finished; a second one aborts.
fn spawn_ctrl_c(cancel: &CancellationToken) {
    let cancel = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("cancelling... (Ctrl-C again to abort)");
            cancel.cancel();
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        }
    });
}

/// Where the JSON result goes: the file at `path`, else stdout.
fn open_report(path: Option<&Path>) -> anyhow::Result<Box<dyn Write>> {
    Ok(match path {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            Box::new(std::io::BufWriter::new(std::fs::File::create(path)?))
        }
        None => Box::new(std::io::stdout()),
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    init_logging();
    let mut cli = Cli::parse_args();
    #[cfg(feature = "steam")]
    if let respak_cli::Command::Steam { command } = cli.command {
        return respak_cli::run_steam(command, &mut std::io::stdout()).await;
    }
    if cli.config.is_some() && cli.profile.is_some() {
        eprintln!("warning: --config is given, so the profile is ignored");
    }
    let settings = Settings::resolve(&cli, load_config(&cli, &std::env::current_dir()?)?)?;
    // `dump` still reads the output directory from the command line.
    cli.out = Some(settings.out.clone());
    let cancel = CancellationToken::new();
    spawn_ctrl_c(&cancel);
    let input = open_input(&settings, &cancel).await?;
    let engine = build_engine(&settings, input.operator.clone())?;
    let mut report = open_report(settings.report.as_deref())?;
    let code = run_with_events(
        cli,
        &engine,
        &cancel,
        &mut report,
        &mut std::io::stderr(),
        &mut std::io::stderr(),
    )
    .await?;
    report.flush()?;
    drop(report);
    if let Some(summary) = input.summary() {
        eprintln!("{summary}");
    }
    Ok(code)
}
