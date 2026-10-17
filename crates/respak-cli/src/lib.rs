//! respak-cli library: the command line and its dispatch; each command lives in `commands/`.
mod commands;
mod config;
mod settings;
mod setup;

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};
use respak_core::broadcast::error::{RecvError, TryRecvError};
#[cfg(feature = "steam")]
use respak_core::broadcast::Receiver;
use respak_core::{CancellationToken, Engine, Event};

#[cfg(feature = "steam")]
pub use commands::steam::{run_steam, SteamCommand};
pub use commands::{ConvertArgs, CopyArgs, DiscoverArgs, DumpArgs, ListArgs, NamesArgs, RunArgs};
pub use config::{load as load_config, Config};
#[cfg(feature = "steam")]
pub use settings::SteamSource;
pub use settings::{Settings, Source};
pub use setup::{build_engine, fs, open_input, OpenedInput};

/// `respak [OPTIONS] [PROFILE] <COMMAND>`: the options may also come after the command, and
/// anything they leave out is read from `respak.toml` (or `respak-<PROFILE>.toml`).
#[derive(Debug, Parser)]
#[command(
    name = "respak",
    arg_required_else_help = true,
    override_usage = "respak [OPTIONS] [PROFILE] <COMMAND>",
    after_help = "Defaults can be kept in a config file instead of being typed every time. Without a \
PROFILE, `respak.toml` is read from the working directory or the nearest parent that has one, if \
any. With one (`respak apex convert ...`), `respak-apex.toml` is read instead, and must exist. \
Flags given here override the file. See docs/cli.md."
)]
pub struct Cli {
    /// Input root directory (or `--steam`; or `root` in the config file).
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    /// Output directory (or `out` in the config file).
    #[arg(long, global = true)]
    pub out: Option<PathBuf>,
    /// Where assets that a pak refers to but does not contain are looked for, after the pak itself
    /// and the `common.rpak` beside it: a glob relative to `--root`, e.g. `paks/Win64/*.rpak`.
    /// Repeat for several. Nothing else is ever scanned.
    #[arg(long, value_name = "GLOB", global = true)]
    pub search: Vec<String>,
    /// Do not apply rpak patches: `patch_master.rpak` is not consulted and a patch file such as
    /// `common(01).rpak` is refused instead of being completed from the files below it.
    #[arg(long, global = true)]
    pub no_patches: bool,
    /// The skeleton animation sequences (`aseq`) are bound to: `auto` (the default: the `arig` or
    /// `mdl_` that lists the sequence, looked for in its pak and the `common.rpak` beside it), or
    /// the guid (`0x...`) or path of an `arig` / `mdl_`.
    #[arg(long, value_name = "AUTO|GUID|PATH", global = true)]
    pub rig: Option<String>,
    /// How the studio layout of an `arig` is found: `auto` (the default: a `mdl_` in the same pak,
    /// else the block itself) or a layout name: `v16`, `v17`, `v19_2`.
    #[arg(long, value_name = "AUTO|LAYOUT", global = true)]
    pub studio: Option<String>,
    /// Directory for the plugin cache (e.g. the guid -> pak index); created if missing.
    #[arg(long, value_name = "DIR", global = true)]
    pub cache: Option<PathBuf>,
    /// Write the JSON result to this file instead of stdout (errors still go to stderr).
    #[arg(long, value_name = "FILE", global = true)]
    pub report: Option<PathBuf>,
    /// Read the input from Steam depots of this app id instead of `--root` (needs `--depot`).
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "APP", conflicts_with = "root", global = true)]
    pub steam: Option<u32>,
    /// Steam branch whose current manifests `--depot ID` takes (default `public`).
    #[cfg(feature = "steam")]
    #[arg(long, global = true)]
    pub branch: Option<String>,
    /// A depot to read: `ID` (the branch's current manifest) or `ID=MANIFEST`. Repeat for several;
    /// on a path collision the later one wins.
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "ID[=MANIFEST]", global = true)]
    pub depot: Vec<respak_steam::DepotSpec>,
    /// Cap of the Steam chunk cache in `.respak/steam/chunks`, e.g. `500MiB` or `8GiB` (default 8GiB).
    #[cfg(feature = "steam")]
    #[arg(long, value_name = "SIZE", value_parser = parse_size, global = true)]
    pub steam_cache_size: Option<u64>,
    /// Read the defaults from this file instead of searching for `respak.toml`.
    #[arg(long, value_name = "FILE", conflicts_with = "no_config", global = true)]
    pub config: Option<PathBuf>,
    /// Do not read any config file.
    #[arg(long, global = true)]
    pub no_config: bool,
    /// Read `respak-<NAME>.toml` instead of `respak.toml`. The same as writing the name before the
    /// command; use it for a profile named like a command.
    #[arg(short, long, value_name = "NAME", value_parser = profile_name, global = true)]
    pub profile: Option<String>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parses the process arguments; on a usage error or `--help`, prints and exits.
    pub fn parse_args() -> Self {
        Self::try_parse_args(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

    /// Parses `args` (the program name first). A word in the place of the command that is not a
    /// command is a profile name: `respak apex convert ...` is `respak -p apex convert ...`.
    pub fn try_parse_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        let cli = Self::try_parse_from(&args)?;
        let Command::Profile(rest) = &cli.command else {
            return Ok(cli);
        };
        let fail = |kind, message: String| Self::command().error(kind, message);
        let at = args.len() - rest.len();
        let name = rest[0].to_str().ok_or_else(|| {
            fail(
                ErrorKind::InvalidUtf8,
                "the profile name is not UTF-8".into(),
            )
        })?;
        let name = profile_name(name).map_err(|message| fail(ErrorKind::InvalidValue, message))?;
        args.remove(at);
        let mut cli = Self::try_parse_from(&args)?;
        if cli.profile.is_some() {
            return Err(fail(
                ErrorKind::ArgumentConflict,
                format!("the profile is given twice (`{name}` and `--profile`)"),
            ));
        }
        if let Command::Profile(rest) = &cli.command {
            let command = rest[0].to_string_lossy();
            return Err(fail(
                ErrorKind::InvalidSubcommand,
                format!("unrecognized command `{command}` after the profile `{name}`"),
            ));
        }
        cli.profile = Some(name);
        Ok(cli)
    }
}

/// A profile name: letters, digits, `-` and `_`, so it cannot name a path.
fn profile_name(text: &str) -> Result<String, String> {
    let valid = !text.is_empty()
        && !text.starts_with('-')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(text.to_owned())
    } else {
        Err(format!(
            "`{text}` is not a profile name (letters, digits, `-` and `_` only)"
        ))
    }
}

/// Commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Steam account: log in once, the stored token is reused.
    #[cfg(feature = "steam")]
    #[command(subcommand_required = true, arg_required_else_help = true)]
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
    /// Write the raw bytes of a pak asset (or every asset a glob matches) to `--out`, whatever its
    /// kind: the header, and the streamed data (`.stream`, `.opt`) if it has any. Needs no support
    /// for the asset's format. The asset's CPU data pages are not included.
    Dump(DumpArgs),
    /// Run a request file (`-` reads stdin): any number of outputs, with globs and lists.
    Run(RunArgs),
    /// Convert a file, or every file a glob matches.
    Convert(ConvertArgs),
    /// A profile name and the command after it; `Cli::try_parse_args` resolves it.
    #[command(external_subcommand)]
    Profile(Vec<OsString>),
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

/// Prints the engine's events to stderr from a background task, until the sender is gone.
#[cfg(feature = "steam")]
pub(crate) fn print_events_to_stderr(mut rx: Receiver<Event>) {
    tokio::spawn(async move {
        let mut events = std::io::stderr();
        loop {
            let printed = match rx.recv().await {
                Ok(event) => print_event(&mut events, &event),
                Err(RecvError::Lagged(missed)) => print_skipped(&mut events, missed),
                Err(RecvError::Closed) => break,
            };
            if printed.is_err() {
                break;
            }
        }
    });
}

/// Writes the progress line of `event`, if it has one.
fn print_event(events: &mut impl Write, event: &Event) -> std::io::Result<()> {
    match event_line(event) {
        Some(line) => writeln!(events, "{line}"),
        None => Ok(()),
    }
}

/// Writes that `missed` events were dropped because the printer fell behind.
fn print_skipped(events: &mut impl Write, missed: u64) -> std::io::Result<()> {
    writeln!(events, "({missed} events skipped)")
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
                Ok(event) => print_event(events, &event)?,
                Err(RecvError::Lagged(missed)) => print_skipped(events, missed)?,
                Err(RecvError::Closed) => {}
            },
        }
    };
    // Whatever was published after the last poll.
    loop {
        match rx.try_recv() {
            Ok(event) => print_event(events, &event)?,
            Err(TryRecvError::Lagged(missed)) => print_skipped(events, missed)?,
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
    let out_dir = cli.out.clone();
    match cli.command {
        #[cfg(feature = "steam")]
        Command::Steam { command } => run_steam(command, out).await,
        Command::Formats => commands::formats(engine, out),
        Command::Discover(args) => commands::discover(engine, cancel, args, out, err).await,
        Command::List(args) => commands::list(engine, cancel, args, out, err).await,
        Command::Names(args) => commands::names(engine, cancel, args, out, err).await,
        Command::Convert(args) => commands::convert(engine, cancel, args, out, err).await,
        Command::Copy(args) => commands::copy(engine, cancel, args, out, err).await,
        Command::Dump(args) => commands::dump(engine, cancel, out_dir, args, out, err).await,
        Command::Run(args) => commands::request(engine, cancel, args, out, err).await,
        Command::Profile(_) => {
            anyhow::bail!("the profile was not resolved; use `Cli::try_parse_args`")
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sizes() {
        for (text, bytes) in [
            ("1024", 1024),
            ("1b", 1),
            ("8GiB", 8 << 30),
            ("500 MiB", 500 << 20),
            ("2k", 2 << 10),
            ("2KB", 2_000),
            ("2gb", 2_000_000_000),
            ("1TB", 1_000_000_000_000),
            ("1tib", 1 << 40),
        ] {
            assert_eq!(parse_size(text), Ok(bytes), "{text}");
        }
    }

    #[test]
    fn bad_sizes() {
        for text in ["", "GiB", "-1", "1.5GiB", "5 parsecs", "99999999999TiB"] {
            assert!(parse_size(text).is_err(), "{text}");
        }
    }

    fn custom(name: &str, data: serde_json::Value) -> Event {
        Event::Custom {
            source: "steam".into(),
            name: name.into(),
            data,
        }
    }

    #[test]
    fn event_lines() {
        let progress = Event::Progress {
            task: "convert".into(),
            done: 2,
            total: 5,
        };
        let opened = Event::InputOpened {
            path: "a.rpak".into(),
            entries: 7,
        };
        let written = Event::OutputWritten {
            path: "x.png".into(),
        };
        let warning = Event::Warning {
            message: "odd".into(),
        };
        let depot = custom("depot", json!({"depot": 1, "manifest": 2, "files": 3}));
        let chunk = custom("chunk", json!({"chunks": 4, "bytes": 5}));
        for (event, line) in [
            (&warning, "warning: odd"),
            (&progress, "[2/5] convert"),
            (&opened, "opened a.rpak (7 entries)"),
            (&written, "wrote x.png"),
            (&depot, "steam: depot 1 manifest 2 (3 files)"),
            (&chunk, "steam: 4 chunks, 5 bytes downloaded"),
        ] {
            assert_eq!(event_line(event).as_deref(), Some(line));
        }
    }

    #[test]
    fn quiet_events() {
        let started = Event::RequestStarted { what: "x".into() };
        let other = Event::Custom {
            source: "other".into(),
            name: "depot".into(),
            data: json!({}),
        };
        assert_eq!(event_line(&started), None);
        assert_eq!(event_line(&other), None);
        assert_eq!(event_line(&custom("unknown", json!({}))), None);
    }
}
