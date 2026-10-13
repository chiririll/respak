//! respak-cli library: command implementations.
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use respak_core::broadcast::error::{RecvError, TryRecvError};
use respak_core::{
    CancellationToken, ConvertReport, ConvertRequest, DiscoverOptions, Engine, Error, ErrorKind, Event, OutputRequest,
    OutputResult, TargetSpec,
};

/// `respak --root <dir> --out <dir> <command>`
#[derive(Debug, Parser)]
pub struct Cli {
    /// Input root directory (required by every command except `steam`).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Output directory (required by every command except `steam`).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Write the JSON result to this file instead of stdout (errors still go to stderr).
    #[arg(long, value_name = "FILE")]
    pub report: Option<PathBuf>,
    /// Read the input from Steam depots of this app id instead of `--root` (needs `--depot`).
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "APP", conflicts_with = "root", requires = "depot")]
    pub steam: Option<u32>,
    /// Steam branch whose current manifests `--depot ID` takes.
    #[cfg(feature = "steam")]
    #[arg(long, default_value = "public", requires = "steam")]
    pub branch: String,
    /// A depot to read: `ID` (the branch's current manifest) or `ID=MANIFEST`. Repeat for several;
    /// on a path collision the later one wins.
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "ID[=MANIFEST]", requires = "steam")]
    pub depot: Vec<respak_steam::DepotSpec>,
    /// Cap of the Steam chunk cache in `.respak/steam/chunks`, e.g. `500MiB` or `8GiB` (default 8GiB).
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "SIZE", value_parser = parse_size, requires = "steam")]
    pub steam_cache_size: Option<u64>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Steam account: log in once, the stored token is reused.
    #[cfg(feature = "steam")]
    Steam {
        /// What to do.
        #[command(subcommand)]
        command: SteamCommand,
    },
    /// Print the format graph as JSON.
    Formats,
    /// Print what each path is and which formats it can be converted to, as JSON.
    Discover {
        /// Paths relative to `--root`.
        #[arg(required = true)]
        paths: Vec<String>,
        /// How many container levels to open (1 lists a pak's assets).
        #[arg(long, default_value_t = 0)]
        depth: u32,
    },
    /// Run a request file (`-` reads stdin): any number of outputs, with globs and lists.
    Run {
        /// Path of the request JSON, or `-`.
        request: String,
    },
    /// Convert a file, or every file a glob matches.
    Convert {
        /// Source path or glob, relative to `--root`.
        path: String,
        /// Target as JSON, e.g. `{"type":"dds"}`.
        #[arg(long)]
        to: String,
        /// Output path, relative to `--out`.
        #[arg(long = "path")]
        out_path: Option<String>,
    },
}

/// `500MiB`, `8GiB`, `2GB`, `1024` (bytes): binary units for `KiB`/`MiB`/`GiB`/`TiB`, decimal for `KB`/`MB`/`GB`/`TB`.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let number: u64 = digits.parse().map_err(|_| format!("`{text}` is not a size (try 500MiB or 8GiB)"))?;
    let factor: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        "k" | "kib" => 1 << 10,
        "m" | "mib" => 1 << 20,
        "g" | "gib" => 1 << 30,
        "t" | "tib" => 1 << 40,
        other => return Err(format!("unknown size unit `{other}` in `{text}`")),
    };
    number.checked_mul(factor).ok_or_else(|| format!("`{text}` is too large"))
}

/// `respak steam <command>`.
#[cfg(feature = "steam")]
#[derive(Debug, Subcommand)]
pub enum SteamCommand {
    /// Log in to Steam and store a refresh token in `.respak/account.json`.
    Login {
        /// Scan a QR code with the Steam mobile app.
        #[arg(long, conflicts_with = "user", required_unless_present = "user")]
        qr: bool,
        /// Log in with this account name (password prompt, then Steam Guard).
        #[arg(long)]
        user: Option<String>,
    },
    /// Delete the stored login.
    Logout,
}

/// Runs a `steam` command; the state directory is `.respak` in the working directory.
#[cfg(feature = "steam")]
pub async fn run_steam(command: SteamCommand, out: &mut impl Write) -> anyhow::Result<ExitCode> {
    use respak_steam::AccountStore;
    let store = AccountStore::new(".respak");
    match command {
        SteamCommand::Login { qr, user } => {
            let account = match user {
                Some(name) if !qr => respak_steam::login_user(&store, &name).await?,
                _ => respak_steam::login_qr(&store, out).await?,
            };
            writeln!(out, "Logged in as {} (steam id {}). Token stored in {}.", account.account_name, account.steam_id, store.path().display())?;
        }
        SteamCommand::Logout => {
            if store.delete()? {
                writeln!(out, "Logged out: {} deleted.", store.path().display())?;
            } else {
                writeln!(out, "Not logged in.")?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// One line of progress for `event`, or `None` for events a person does not need to see
/// (failures are in the report).
pub fn event_line(event: &Event) -> Option<String> {
    match event {
        Event::Warning { message } => Some(format!("warning: {message}")),
        Event::Progress { task, done, total } => Some(format!("[{done}/{total}] {task}")),
        Event::InputOpened { path, entries } => Some(format!("opened {path} ({entries} entries)")),
        Event::OutputWritten { path } => Some(format!("wrote {path}")),
        Event::Custom { source, name, data } if source == "steam" => match name.as_str() {
            "depot" => Some(format!("steam: depot {} manifest {} ({} files)", data["depot"], data["manifest"], data["files"])),
            "chunk" => Some(format!("steam: {} chunks, {} bytes downloaded", data["chunks"], data["bytes"])),
            _ => None,
        },
        _ => None,
    }
}

/// [`run`], printing the engine's events to `events` as they happen.
pub async fn run_with_events(
    cli: Cli,
    engine: &Engine,
    cancel: &CancellationToken,
    out: &mut impl Write,
    err: &mut impl Write,
    events: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let mut rx = engine.events();
    let command = run(cli, engine, cancel, out, err);
    tokio::pin!(command);
    let result = loop {
        tokio::select! {
            result = &mut command => break result,
            event = rx.recv() => match event {
                Ok(event) => {
                    if let Some(line) = event_line(&event) {
                        writeln!(events, "{line}")?;
                    }
                }
                Err(RecvError::Lagged(missed)) => writeln!(events, "({missed} events skipped)")?,
                Err(RecvError::Closed) => {}
            },
        }
    };
    // Whatever was published after the last poll.
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if let Some(line) = event_line(&event) {
                    writeln!(events, "{line}")?;
                }
            }
            Err(TryRecvError::Lagged(missed)) => writeln!(events, "({missed} events skipped)")?,
            Err(_) => break,
        }
    }
    result
}

/// Runs a command; the JSON report goes to `out`, error messages to `err`. The exit code is
/// non-zero if any output failed. An unusable command line (malformed `--to`) is an `Err`.
pub async fn run(
    cli: Cli,
    engine: &Engine,
    cancel: &CancellationToken,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    match cli.command {
        #[cfg(feature = "steam")]
        Command::Steam { command } => run_steam(command, out).await,
        Command::Formats => {
            serde_json::to_writer_pretty(&mut *out, &engine.formats())?;
            writeln!(out)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Discover { paths, depth } => {
            let options = DiscoverOptions { depth };
            let mut found = Vec::new();
            let mut failed = false;
            for path in &paths {
                match engine.discover(path, &options, cancel).await {
                    Ok(discovery) => found.push(discovery),
                    Err(error) => {
                        writeln!(err, "{path}: {error}")?;
                        failed = true;
                    }
                }
            }
            serde_json::to_writer_pretty(&mut *out, &found)?;
            writeln!(out)?;
            Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Convert { path, to, out_path } => {
            let to: TargetSpec = serde_json::from_str(&to).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("--to is not a valid target: {e}") })
            })?;
            let request =
                ConvertRequest { version: 1, outputs: vec![OutputRequest { from: path.into(), to, path: out_path }] };
            report(engine.convert(&request, cancel).await?, out, err)
        }
        Command::Run { request } => {
            let text = if request == "-" {
                std::io::read_to_string(std::io::stdin())?
            } else {
                std::fs::read_to_string(&request).map_err(|e| {
                    Error::new(ErrorKind::InvalidRequest { message: format!("cannot read {request}: {e}") })
                })?
            };
            let request: ConvertRequest = serde_json::from_str(&text).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("the request is not valid: {e}") })
            })?;
            report(engine.convert(&request, cancel).await?, out, err)
        }
    }
}

/// Prints the report as JSON, each failure on `err`; non-zero exit if any output failed.
fn report(report: ConvertReport, out: &mut impl Write, err: &mut impl Write) -> anyhow::Result<ExitCode> {
    serde_json::to_writer_pretty(&mut *out, &report)?;
    writeln!(out)?;
    let mut failed = false;
    for output in &report.outputs {
        if let OutputResult::Err { error } = &output.result {
            writeln!(err, "{}: {error}", output.from)?;
            failed = true;
        }
    }
    Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}
