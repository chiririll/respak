//! respak-cli library: the command line and its dispatch; each command lives in `commands/`.
mod commands;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use respak_core::broadcast::error::{RecvError, TryRecvError};
use respak_core::{CancellationToken, Engine, Event};

#[cfg(feature = "steam")]
pub use commands::steam::{run_steam, SteamCommand};
pub use commands::{ConvertArgs, CopyArgs, DiscoverArgs, ListArgs, NamesArgs, RunArgs};

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
    /// The skeleton animation sequences (`aseq`) are bound to: `auto` (the `arig` or `mdl_` that
    /// lists the sequence, looked for in its pak and the `common.rpak` beside it), or the guid
    /// (`0x...`) or path of an `arig` / `mdl_`.
    #[arg(long, default_value = "auto", value_name = "AUTO|GUID|PATH")]
    pub rig: String,
    /// How the studio layout of an `arig` is found: `auto` (a `mdl_` in the same pak, else the
    /// block itself) or a layout name: `v16`, `v17`, `v19_2`.
    #[arg(long, default_value = "auto", value_name = "AUTO|LAYOUT")]
    pub studio: String,
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
    Discover(DiscoverArgs),
    /// Print the entries of containers (rpak, mbnk, ...), one path per line. Nothing is
    /// described or opened, so it is fast; use `discover --depth 1` for formats and targets.
    List(ListArgs),
    /// Look for the names of asset guids in the strings the input can give: names and possible
    /// names stored in the assets of paks, and printable text of files of no known format
    /// (executables, scripts). Each string is hashed; those that match a wanted guid are printed
    /// as `0x<GUID><TAB><name>`. Works with `--steam`.
    Names(NamesArgs),
    /// Copy a file, or every file a glob matches, to `--out` unchanged (with `--steam`: download
    /// it). To combine it with conversions, add `{"type":"file"}` as one more output of a `run` request.
    Copy(CopyArgs),
    /// Run a request file (`-` reads stdin): any number of outputs, with globs and lists.
    Run(RunArgs),
    /// Convert a file, or every file a glob matches.
    Convert(ConvertArgs),
}

/// `500MiB`, `8GiB`, `2GB`, `1024` (bytes): binary units for `KiB`/`MiB`/`GiB`/`TiB`, decimal for `KB`/`MB`/`GB`/`TB`.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let number: u64 = digits
        .parse()
        .map_err(|_| format!("`{text}` is not a size (try 500MiB or 8GiB)"))?;
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
    number
        .checked_mul(factor)
        .ok_or_else(|| format!("`{text}` is too large"))
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
            "depot" => Some(format!(
                "steam: depot {} manifest {} ({} files)",
                data["depot"], data["manifest"], data["files"]
            )),
            "chunk" => Some(format!(
                "steam: {} chunks, {} bytes downloaded",
                data["chunks"], data["bytes"]
            )),
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
        Command::Formats => commands::formats(engine, out),
        Command::Discover(args) => commands::discover(engine, cancel, args, out, err).await,
        Command::List(args) => commands::list(engine, cancel, args, out, err).await,
        Command::Names(args) => commands::names(engine, cancel, args, out, err).await,
        Command::Convert(args) => commands::convert(engine, cancel, args, out, err).await,
        Command::Copy(args) => commands::copy(engine, cancel, args, out, err).await,
        Command::Run(args) => commands::request(engine, cancel, args, out, err).await,
    }
}
