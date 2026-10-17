//! Turns [`Settings`] into an input and an [`Engine`].

use std::path::Path;

use opendal::{services::Fs, Operator};
use respak_core::{CancellationToken, Engine};
use respak_formats::FormatsPlugin;
use respak_nihav::NihavPlugin;
use respak_rad::RadPlugin;
use respak_respawn::RespawnPlugin;

use crate::{Settings, Source};

/// An operator rooted at `root`, which is created if missing.
pub fn fs(root: &Path) -> anyhow::Result<Operator> {
    std::fs::create_dir_all(root)?;
    Ok(Operator::new(Fs::default().root(&root.to_string_lossy()))?)
}

/// The opened input.
pub struct OpenedInput {
    /// What the engine reads.
    pub operator: Operator,
    #[cfg(feature = "steam")]
    steam: Option<respak_steam::SteamInput>,
}

impl OpenedInput {
    /// A line about what Steam did, if the input is Steam.
    pub fn summary(&self) -> Option<String> {
        #[cfg(feature = "steam")]
        if let Some(steam) = &self.steam {
            let s = steam.stats();
            return Some(format!(
                "steam: {} chunks requested: {} downloaded ({}), {} served from the cache",
                s.chunks_downloaded + s.cache_hits,
                s.chunks_downloaded,
                crate::format_size(s.bytes_downloaded),
                s.cache_hits
            ));
        }
        None
    }
}

/// Opens the input: the directory, or the Steam depots (progress goes to stderr).
#[cfg_attr(not(feature = "steam"), allow(unused_variables))]
pub async fn open_input(
    settings: &Settings,
    cancel: &CancellationToken,
) -> anyhow::Result<OpenedInput> {
    match &settings.source {
        Source::Root(root) => Ok(OpenedInput {
            operator: fs(root)?,
            #[cfg(feature = "steam")]
            steam: None,
        }),
        #[cfg(feature = "steam")]
        Source::Steam(steam) => {
            let mut options =
                respak_steam::SteamOptions::new(steam.app, steam.depots.clone(), ".respak");
            options.branch = steam.branch.clone();
            if let Some(cap) = steam.cache_size {
                options.cache_cap = cap;
            }
            let events = respak_core::Events::new(respak_core::event::DEFAULT_EVENT_CAPACITY);
            crate::print_events_to_stderr(events.subscribe());
            let opened = respak_steam::open(options, cancel, &events).await?;
            Ok(OpenedInput {
                operator: opened.operator.clone(),
                steam: Some(opened),
            })
        }
    }
}

/// The engine over `input`, writing to `settings.out`, with every plugin registered.
pub fn build_engine(settings: &Settings, input: Operator) -> anyhow::Result<Engine> {
    let mut builder = Engine::builder()
        .input(input)
        .output(fs(&settings.out)?)
        .plugin(FormatsPlugin)
        .plugin(NihavPlugin)
        .plugin(RadPlugin)
        .plugin(
            RespawnPlugin::new()
                .search(settings.search.clone())
                .ignore_patches(settings.no_patches)
                .rig(settings.rig.clone())
                .studio(settings.studio.clone()),
        );
    if let Some(dir) = &settings.cache {
        builder = builder.cache(fs(dir)?);
    }
    Ok(builder.build()?)
}
