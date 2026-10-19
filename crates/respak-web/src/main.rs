//! `respak-web`: serves the web UI on this computer and opens it in the browser.

use std::path::PathBuf;

use clap::Parser;
use respak_web::{router, App};

/// The web UI of respak.
#[derive(Debug, Parser)]
#[command(name = "respak-web", version)]
struct Args {
    /// Port to listen on, on 127.0.0.1 (default: a free one).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Do not open the browser.
    #[arg(long)]
    no_open: bool,
    /// Look for `respak.toml` here and in the directories above (default: the working directory).
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let cwd = match args.dir {
        Some(dir) => plain(dir.canonicalize()?),
        None => std::env::current_dir()?,
    };
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", args.port)).await?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}/");
    println!("respak web UI: {url}  (Ctrl-C to stop)");
    if !args.no_open {
        open_browser(&url);
    }
    axum::serve(listener, router(App::new(cwd), port))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// A path without the verbatim prefix (`\\?\`) Windows puts on canonical ones, which people do not want to read.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

/// Asks the system to open `url`; failing to is not an error, the URL is printed.
fn open_browser(url: &str) {
    let result = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(error) = result {
        eprintln!("could not open the browser: {error}");
    }
}
