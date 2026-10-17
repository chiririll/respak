//! `respak names`: finds the names of asset guids in the strings the input can give.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, Content, Engine, EntryInfo, Error, ErrorKind};
use respak_respawn::{parse_guids, Finder};

use super::failures::Failures;

/// Arguments of `names`.
#[derive(Debug, Args)]
#[command(arg_required_else_help = true)]
pub struct NamesArgs {
    /// Text file with the wanted guids (`0x0123456789ABCDEF`, extra text on a line is ignored),
    /// e.g. the output of `list`. A path relative to the working directory. Without it, the
    /// wanted guids are the assets of the sources that have no name of their own.
    #[arg(long, value_name = "FILE")]
    hashes: Option<PathBuf>,
    /// Also try each string with this prefix in front (e.g. `datatable/`). Repeatable.
    #[arg(long, value_name = "TEXT")]
    prefix: Vec<String>,
    /// Also try each string with this suffix added (e.g. `.rpak`). Repeatable.
    #[arg(long, value_name = "TEXT")]
    suffix: Vec<String>,
    /// Print each name as `0x<GUID><TAB><name>` instead of the bare name.
    #[arg(long)]
    guids: bool,
    /// Paths or globs relative to `--root` (or the Steam depots): paks to read, files to scan.
    #[arg(required = true)]
    sources: Vec<String>,
}

/// Reads each source: a container gives, per entry, its stored name, its dependencies' names and
/// what its format offers as possible names; a file of no known format is read for printable
/// text; any other file holds no names and is skipped. Matches go to `out` as
/// `<name>`, or `0x<GUID><TAB><name>` with `--guids`; progress and the summary to `err`.
pub(crate) async fn run(
    engine: &Engine,
    cancel: &CancellationToken,
    args: NamesArgs,
    out: &mut impl Write,
    err: &mut impl Write,
) -> anyhow::Result<ExitCode> {
    let wanted = read_wanted(args.hashes.as_deref(), err)?;
    let mut finder = Finder::new(wanted, args.prefix, args.suffix);
    let mut failures = Failures::default();
    let mut assets = 0usize;
    let mut paths = Vec::new();
    for source in &args.sources {
        match engine.expand(source, cancel).await {
            Ok(found) => paths.extend(found),
            Err(error) => failures.report(err, source, error)?,
        }
    }
    for (n, path) in paths.iter().enumerate() {
        if finder.is_complete() {
            break;
        }
        writeln!(err, "[{}/{}] reading {path}", n + 1, paths.len())?;
        match engine.content(path, cancel).await {
            Ok(Content::Container(entries)) => {
                assets += entries.len();
                entries
                    .iter()
                    .for_each(|entry| feed_entry(&mut finder, entry));
                writeln!(
                    err,
                    "  {assets} assets read, {} names found",
                    finder.found().len()
                )?;
            }
            Ok(Content::Unrecognized) => {
                writeln!(err, "  no known format, reading as text")?;
                let read = engine
                    .read_file(path, cancel, &mut |chunk| finder.feed_bytes(chunk))
                    .await;
                match read {
                    Ok(()) => finder.feed_bytes(&[]),
                    Err(error) => failures.report(err, path, error)?,
                }
            }
            Ok(Content::Opaque) => {}
            Err(error) => failures.report(err, path, error)?,
        }
    }
    finder.finish();
    for (guid, name) in finder.found() {
        if args.guids {
            writeln!(out, "0x{guid:016X}	{name}")?;
        } else {
            writeln!(out, "{name}")?;
        }
    }
    writeln!(
        err,
        "{} of {} guids named, {assets} assets read",
        finder.found().len(),
        finder.wanted()
    )?;
    Ok(failures.exit_code())
}

/// The guids to look for: those in the `--hashes` file, or `None` for the unnamed assets.
fn read_wanted(path: Option<&Path>, err: &mut impl Write) -> anyhow::Result<Option<HashSet<u64>>> {
    let Some(path) = path else {
        writeln!(
            err,
            "no --hashes: looking for the assets of the sources that have no name"
        )?;
        return Ok(None);
    };
    let invalid = |message| Error::new(ErrorKind::InvalidRequest { message });
    let text = std::fs::read_to_string(path)
        .map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    let wanted = parse_guids(&text);
    if wanted.is_empty() {
        return Err(invalid(format!("{} has no guids", path.display())).into());
    }
    writeln!(
        err,
        "looking for {} guids from {}",
        wanted.len(),
        path.display()
    )?;
    Ok(Some(wanted))
}

/// Gives the finder the entry's guid and every string the entry offers as a name.
fn feed_entry(finder: &mut Finder, entry: &EntryInfo) {
    let guid = entry
        .guid
        .or_else(|| parse_guids(&entry.path).into_iter().next());
    if let Some(guid) = guid {
        finder.asset(guid, entry.name.is_some());
    }
    entry.name.iter().for_each(|n| finder.feed(n));
    entry
        .dependencies
        .iter()
        .filter_map(|d| d.name.as_deref())
        .for_each(|n| finder.feed(n));
    entry.name_candidates.iter().for_each(|n| finder.feed(n));
}
