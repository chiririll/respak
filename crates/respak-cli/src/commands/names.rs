//! `respak names`: finds the names of asset guids in the strings the input can give.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use respak_core::{CancellationToken, DiscoverOptions, Engine, ErrorKind};
use respak_respawn::{parse_guids, Finder};

/// Arguments of `names`.
#[derive(Debug, Args)]
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
    let wanted = match &args.hashes {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
            let wanted = parse_guids(&text);
            anyhow::ensure!(!wanted.is_empty(), "{} has no guids", path.display());
            writeln!(
                err,
                "looking for {} guids from {}",
                wanted.len(),
                path.display()
            )?;
            Some(wanted)
        }
        None => {
            writeln!(
                err,
                "no --hashes: looking for the assets of the sources that have no name"
            )?;
            None
        }
    };
    let mut finder = Finder::new(wanted, args.prefix, args.suffix);
    let mut failed = false;
    let mut assets = 0usize;
    let mut paths = Vec::new();
    for source in &args.sources {
        match engine.expand(source, cancel).await {
            Ok(found) => paths.extend(found),
            Err(error) => {
                writeln!(err, "{source}: {error}")?;
                failed = true;
            }
        }
    }
    for (n, path) in paths.iter().enumerate() {
        if finder.is_complete() {
            break;
        }
        writeln!(err, "[{}/{}] reading {path}", n + 1, paths.len())?;
        match engine.inspect(path, cancel).await {
            Ok(entries) => {
                assets += entries.len();
                for entry in entries {
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
                writeln!(
                    err,
                    "  {assets} assets read, {} names found",
                    finder.found().len()
                )?;
            }
            // Not a container: a file of no known format may hold paths as text, any other holds none.
            Err(error) if matches!(error.kind(), ErrorKind::NotFound { .. }) => {
                let unknown = match engine
                    .discover(path, &DiscoverOptions::default(), cancel)
                    .await
                {
                    Ok(found) => found.entries.first().is_some_and(|e| e.format.is_none()),
                    Err(_) => false,
                };
                if unknown {
                    writeln!(err, "  no known format, reading as text")?;
                    match engine
                        .read_file(path, cancel, &mut |chunk| finder.feed_bytes(chunk))
                        .await
                    {
                        Ok(()) => finder.feed_bytes(&[]),
                        Err(error) => {
                            writeln!(err, "{path}: {error}")?;
                            failed = true;
                        }
                    }
                }
            }
            Err(error) => {
                writeln!(err, "{path}: {error}")?;
                failed = true;
            }
        }
    }
    finder.finish();
    for (guid, name) in finder.found() {
        if args.guids {
            writeln!(out, "0x{guid:016X}\t{name}")?;
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
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
