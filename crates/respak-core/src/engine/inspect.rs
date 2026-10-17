//! Looking at the input without converting it: `discover`, `list` and `inspect`.

use super::parallel::parallel_map_ordered;
use super::{Engine, NAME_CONCURRENCY};
use crate::capability::Container;
use crate::discover::{DiscoverOptions, Discovery, DiscoveryEntry, EntryInfo, ListedEntry};
use crate::error::Result;
use crate::mediator::{Cx, Mounted};
use crate::value::{Value, FILE};
use crate::volume::list_volume;
use crate::CancellationToken;

impl Engine {
    /// Detects what `path` is and which formats it can be converted to, without decoding.
    /// `NotFound` if the path does not exist; an unrecognised file is an entry with no format.
    ///
    /// `depth` is how many container levels to open: 0 describes the item itself; 1 lists the
    /// contents of a container (one entry per item, instead of the container's own entry); and so on.
    pub async fn discover(
        &self,
        path: &str,
        options: &DiscoverOptions,
        cancel: &CancellationToken,
    ) -> Result<Discovery> {
        let cx = self.cx(cancel);
        let value = self.mediator.open(path, &cx).await?;
        let mut entries = Vec::new();
        self.describe(
            &value,
            path.trim_matches('/'),
            options,
            options.depth,
            &cx,
            &mut entries,
        )
        .await?;
        Ok(Discovery {
            path: path.to_owned(),
            entries,
        })
    }

    /// Opens `path` as a container and lists its files (the start of `list` and `inspect`).
    async fn mounted_files(&self, path: &str, cx: &Cx) -> Result<(Mounted, Vec<String>)> {
        let value = self.mediator.open(path, cx).await?;
        let mounted = self.mediator.mount_container(value, cx).await?;
        let files = list_volume(&mounted.container().volume()).await?;
        Ok((mounted, files))
    }

    /// Lists the entries of the container at `path`, sorted, as paths relative to the container
    /// (`assets/some_asset.type`). Without `names` nothing is opened (no detection, no sibling
    /// files). With `names` every entry is read like `discover` does, and its `Metadata` name
    /// (if it has one that differs from the path) is reported too; an entry that cannot be read
    /// is listed without a name. `NotFound` if `path` is not a container.
    pub async fn list(
        &self,
        path: &str,
        names: bool,
        cancel: &CancellationToken,
    ) -> Result<Vec<ListedEntry>> {
        use futures::StreamExt;
        let cx = self.cx(cancel);
        let (mounted, files) = self.mounted_files(path, &cx).await?;
        if !names {
            return Ok(files
                .into_iter()
                .map(|path| ListedEntry { path, name: None })
                .collect());
        }
        let (cx, c) = (&cx, mounted.container());
        Ok(futures::stream::iter(files)
            .map(|path| async move {
                ListedEntry {
                    name: self.entry_name(c, &path, cx).await,
                    path,
                }
            })
            .buffered(NAME_CONCURRENCY)
            .collect()
            .await)
    }

    /// Reads every entry of the container at `path` and reports what each one is made of: its
    /// `Metadata` (name, kind, guid, dependencies) and the strings it offers as possible names of
    /// other assets (`NameSource`). Entries are read like `list` with names does, several at a
    /// time, on as many threads as there are cores (on one, in a single-threaded runtime). An entry
    /// that cannot be read is reported with its path only, and the entries come in the container's
    /// order. `NotFound` if `path` is not a container. Publishes `Progress` every
    /// [`INSPECT_PROGRESS`](super::INSPECT_PROGRESS) entries.
    pub async fn inspect(&self, path: &str, cancel: &CancellationToken) -> Result<Vec<EntryInfo>> {
        let cx = self.cx(cancel);
        let (mounted, files) = self.mounted_files(path, &cx).await?;
        let (cx, c) = (&cx, mounted.container());
        parallel_map_ordered(&files, cx, &format!("read {path}"), |file| {
            self.entry_info(c, file.clone(), cx)
        })
        .await
    }

    /// Reads the entry `path` of `c` as far as its format is known. Whatever cannot be read is
    /// left out, so the entry always has its path.
    async fn entry_info(&self, c: &dyn Container, path: String, cx: &Cx) -> EntryInfo {
        let mut info = EntryInfo {
            path,
            ..EntryInfo::default()
        };
        let Ok(mut value) = c.entry(&info.path).await else {
            return info;
        };
        if value.as_metadata().and_then(|m| m.name()).is_none() {
            if let Ok(read) = self.mediator.identify(value.clone(), cx).await {
                value = read;
            }
        }
        if let Some(meta) = value.as_metadata() {
            info.name = meta.name().map(str::to_owned);
            info.kind = meta.kind().map(str::to_owned);
            info.guid = meta.guid();
            info.dependencies = meta.dependencies();
        }
        if let Some(source) = value.as_name_source() {
            info.name_candidates = source.name_candidates();
        }
        info
    }

    /// The `Metadata` name of the entry `path`, with the entry's extension (names are stored
    /// without the kind), unless it is just the path again.
    async fn entry_name(&self, c: &dyn Container, path: &str, cx: &Cx) -> Option<String> {
        let name = self.entry_info(c, path.to_owned(), cx).await.name?;
        let name = match path.rsplit_once('.') {
            Some((_, ext)) if !name.ends_with(&format!(".{ext}")) => format!("{name}.{ext}"),
            _ => name,
        };
        (name != path).then_some(name)
    }

    async fn describe(
        &self,
        value: &Value,
        path: &str,
        options: &DiscoverOptions,
        depth: u32,
        cx: &Cx,
        out: &mut Vec<DiscoveryEntry>,
    ) -> Result<()> {
        let entry = self.describe_item(value, path, options, cx).await?;
        let opens = depth > 0
            && entry
                .format
                .as_deref()
                .is_some_and(|f| self.mediator.provides_container(f));
        if !opens {
            out.push(entry);
            return Ok(());
        }
        self.describe_children(value, options, depth, cx, out).await
    }

    /// The entry for the item itself: detected (or typed, or unrecognised), with its metadata.
    async fn describe_item(
        &self,
        value: &Value,
        path: &str,
        options: &DiscoverOptions,
        cx: &Cx,
    ) -> Result<DiscoveryEntry> {
        let mut hits = self.mediator.detect(value, cx).await?.into_iter();
        let mut entry = match hits.next() {
            Some(winner) => {
                let targets = self.mediator.targets_from(winner.format.name());
                DiscoveryEntry::from_detected(path, winner, hits, targets)
            }
            // A typed entry handed out by a container carries its own format.
            None if value.format().name() != FILE => DiscoveryEntry::typed(
                path,
                value.format(),
                self.mediator.targets_from(value.format().name()),
            ),
            None => DiscoveryEntry::unrecognised(path),
        };
        if let Some(meta) = value.as_metadata() {
            entry.apply_meta(meta);
        }
        if options.names && entry.name.is_none() && entry.format.is_some() {
            if let Ok(read) = self.mediator.identify(value.clone(), cx).await {
                if let Some(meta) = read.as_metadata() {
                    entry.apply_meta(meta);
                }
            }
        }
        Ok(entry)
    }

    /// Describes every entry of the container `value`, `depth - 1` levels deep.
    async fn describe_children(
        &self,
        value: &Value,
        options: &DiscoverOptions,
        depth: u32,
        cx: &Cx,
        out: &mut Vec<DiscoveryEntry>,
    ) -> Result<()> {
        let mounted = self.mediator.mount_container(value.clone(), cx).await?;
        let c = mounted.container();
        let mount_path = c.mount_path().to_owned();
        for rel in list_volume(&c.volume()).await? {
            cx.check_cancelled()?;
            let child_path = format!("{}/{rel}", mount_path.trim_end_matches('/'));
            match c.entry(&rel).await {
                Ok(child) => {
                    Box::pin(self.describe(&child, &child_path, options, depth - 1, cx, out))
                        .await?
                }
                Err(error) => {
                    cx.warn(format!("cannot describe {child_path}: {error}"));
                    out.push(DiscoveryEntry::unrecognised(&child_path));
                }
            }
        }
        Ok(())
    }
}
