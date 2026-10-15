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
    /// Where assets that a pak refers to but does not contain are looked for, after the pak itself
    /// and the `common.rpak` beside it: a glob relative to `--root`, e.g. `paks/Win64/*.rpak`.
    /// Repeat for several. Nothing else is ever scanned.
    #[arg(long, value_name = "GLOB")]
    pub search: Vec<String>,
    /// Do not apply rpak patches: `patch_master.rpak` is not consulted and a patch file such as
    /// `common(01).rpak` is refused instead of being completed from the files below it.
    #[arg(long)]
    pub no_patches: bool,
    /// Directory for the plugin cache (e.g. the guid -> pak index); created if missing.
    #[arg(long, value_name = "DIR")]
    pub cache: Option<PathBuf>,
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
    /// Print the entries of containers (rpak, mbnk, ...), one path per line. Nothing is
    /// described or opened, so it is fast; use `discover --depth 1` for formats and targets.
    List {
        /// Container paths relative to `--root`.
        #[arg(required = true)]
        paths: Vec<String>,
        /// Read every entry and print its name after the path (tab-separated), where it has one.
        /// Slow: an rpak asset's name is stored in its header, and only some kinds can be read.
        #[arg(long, conflicts_with = "names")]
        map: bool,
        /// Read every entry and print only the names, only of entries that have one (slow, see `--map`).
        #[arg(long)]
        names: bool,
    },
    /// Copy a file, or every file a glob matches, to `--out` unchanged (with `--steam`: download
    /// it). To combine it with conversions, add `{"type":"file"}` as one more output of a `run` request.
    Copy {
        /// Source path or glob, relative to `--root` (or the Steam depots).
        path: String,
        /// Output path, relative to `--out`; default: the source path.
        #[arg(long = "path")]
        out_path: Option<String>,
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
        /// Also copy each source file to `--out` unchanged, like `copy`.
        #[arg(long)]
        copy: bool,
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
        Command::List { paths, map, names } => {
            let mut failed = false;
            for path in &paths {
                match engine.list(path, map || names, cancel).await {
                    Ok(entries) => {
                        for entry in entries {
                            match (entry.name, names) {
                                (Some(name), true) => writeln!(out, "{name}")?,
                                (None, true) => {}
                                (Some(name), false) => writeln!(out, "{}\t{name}", entry.path)?,
                                (None, false) => writeln!(out, "{}", entry.path)?,
                            }
                        }
                    }
                    Err(error) => {
                        writeln!(err, "{path}: {error}")?;
                        failed = true;
                    }
                }
            }
            Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Convert { path, to, out_path, copy } => {
            let to: TargetSpec = serde_json::from_str(&to).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest { message: format!("--to is not a valid target: {e}") })
            })?;
            let mut outputs = vec![OutputRequest { from: path.clone().into(), to, path: out_path }];
            if copy {
                outputs.push(OutputRequest { from: path.into(), to: file_target(), path: None });
            }
            let mut done = engine.convert(&ConvertRequest { version: 1, outputs }, cancel).await?;
            if copy {
                skip_assets(&mut done);
            }
            report(done, out, err)
        }
        Command::Copy { path, out_path } => {
            let outputs = vec![OutputRequest { from: path.into(), to: file_target(), path: out_path }];
            let mut done = engine.convert(&ConvertRequest { version: 1, outputs }, cancel).await?;
            skip_assets(&mut done);
            report(done, out, err)
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

/// `{"type":"file"}`: the source unchanged.
fn file_target() -> TargetSpec {
    TargetSpec { format: respak_core::FormatId::from_static("file"), options: Default::default(), via: Default::default() }
}

/// Drops the entries that only say a source is not a file (an asset inside a pak has no bytes of
/// its own to copy), so copying a glob that reaches into paks copies the real files and goes on.
fn skip_assets(report: &mut ConvertReport) {
    report.outputs.retain(|o| {
        !matches!(&o.result, OutputResult::Err { error } if matches!(error.kind(), ErrorKind::NoPath { to, .. } if to == "file"))
    });
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
