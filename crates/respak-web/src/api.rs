//! The handlers of `/api` (docs/design/webui.md section 5).

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use respak_core::{DiscoverOptions, FormatId, TargetInfo, TargetSpec};
use respak_host::{Config, Overrides, Settings, DEFAULT_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json_};
use std::path::{Path as FsPath, PathBuf};

use crate::error::{ApiError, ApiResult};
use crate::jobs::{BasketItem, JobView};
use crate::session::Session;
use crate::tree::{self, clean, SearchQuery, TreeQuery};
use crate::App;

type Json2<T> = ApiResult<Json<T>>;

// ---------------------------------------------------------------- settings

/// Where the config lives: the nearest `respak.toml`, else the one to create here.
fn config_file(app: &App) -> (PathBuf, bool) {
    match respak_host::find_up(&app.cwd, DEFAULT_FILE) {
        Some(file) => (file, true),
        None => (app.cwd.join(DEFAULT_FILE), false),
    }
}

/// The config anchored at its file, resolved to settings.
fn settings_of(app: &App) -> anyhow::Result<Settings> {
    let (file, exists) = config_file(app);
    anyhow::ensure!(
        exists,
        "there are no settings yet: choose a source and save them first"
    );
    let mut config = respak_host::load_file(&file)?;
    respak_host::anchor_all(&mut config, &file);
    Settings::resolve(&Overrides::default(), config)
}

/// `GET /api/config`.
pub async fn get_config(State(app): State<App>) -> Json2<Json_> {
    let (file, exists) = config_file(&app);
    let config = if exists {
        respak_host::load_file(&file)?
    } else {
        Config::default()
    };
    Ok(Json(json!({
        "file": file.display().to_string(),
        "exists": exists,
        "cwd": app.cwd.display().to_string(),
        "steamAvailable": cfg!(feature = "steam"),
        "config": config,
    })))
}

/// `PUT /api/config`: saves the settings; the answer says whether they can be connected as they are.
pub async fn put_config(State(app): State<App>, Json(body): Json<Json_>) -> Json2<Json_> {
    let config: Config = serde_json::from_value(body)
        .map_err(|e| ApiError::bad_request(format!("invalid settings: {e}")))?;
    let (file, _) = config_file(&app);
    respak_host::save(&file, &config)?;
    let mut anchored = config;
    respak_host::anchor_all(&mut anchored, &file);
    let problem = Settings::resolve(&Overrides::default(), anchored).err();
    Ok(Json(json!({
        "file": file.display().to_string(),
        "problem": problem.map(|e| e.to_string()),
    })))
}

// ----------------------------------------------------------------- session

/// `GET /api/session`.
pub async fn get_session(State(app): State<App>) -> Json2<Json_> {
    Ok(Json(match app.session().await {
        Some(session) => json!({
            "connected": true,
            "session": session.info(),
            "steam": session.steam_stats(),
        }),
        None => json!({ "connected": false }),
    }))
}

/// `POST /api/session`: builds the engine from the saved settings. The old session ends first.
pub async fn connect(State(app): State<App>) -> Json2<Json_> {
    let settings = settings_of(&app)?;
    app.end_session().await;
    let session = Session::connect(settings, app.ui.clone()).await?;
    let info = session.info();
    app.set_session(session).await;
    let _ = app
        .ui
        .send(crate::events::UiEvent::Session { connected: true });
    Ok(Json(json!({ "connected": true, "session": info })))
}

/// `DELETE /api/session`.
pub async fn disconnect(State(app): State<App>) -> Json2<Json_> {
    app.end_session().await;
    Ok(Json(json!({ "connected": false })))
}

// ------------------------------------------------------------------- files

/// `GET /api/tree`.
pub async fn get_tree(State(app): State<App>, Query(query): Query<TreeQuery>) -> Json2<tree::Page> {
    let session = app.need_session().await?;
    Ok(Json(tree::page(&session, query).await?))
}

/// `GET /api/search`.
pub async fn get_search(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Json2<tree::SearchResult> {
    let session = app.need_session().await?;
    Ok(Json(tree::search(&session, query).await?))
}

/// The query of the endpoints about one path.
#[derive(Debug, Deserialize)]
pub struct PathQuery {
    /// Virtual path.
    pub path: String,
    /// Mip level, for images.
    pub mip: Option<u32>,
    /// Audio track.
    pub track: Option<u32>,
    /// Target format, for text.
    pub target: Option<String>,
}

/// How a page should show an entry.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Viewer {
    /// A picture with mips.
    Image,
    /// A UI atlas with its sprites.
    Atlas,
    /// Sound.
    Audio,
    /// Something that converts to text.
    Text,
    /// A container.
    Container,
    /// Details only.
    None,
}

/// What the page needs to show one entry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    #[serde(flatten)]
    discovery: Option<respak_core::DiscoveryEntry>,
    path: String,
    viewer: Viewer,
    size: Option<u64>,
    meta: Option<Json_>,
    bitmap: Option<Json_>,
    audio: Option<Json_>,
    /// Targets whose result can be read as text, for the text viewer.
    text_targets: Vec<String>,
    /// Whether the bytes can be shown as hex.
    hex: bool,
}

fn viewer_of(
    session: &Session,
    format: Option<&str>,
    targets: &[TargetInfo],
) -> (Viewer, Vec<String>) {
    let has = |name: &str| targets.iter().any(|t| t.format == name);
    let text: Vec<String> = targets
        .iter()
        .filter(|t| {
            let name = t.format.as_str();
            name.ends_with("json") || name.ends_with("csv") || name == "txt"
        })
        .filter(|t| t.format != "uimg.json")
        .map(|t| t.format.clone())
        .collect();
    let container = format.is_some_and(|f| {
        session
            .graph
            .formats
            .iter()
            .any(|i| i.name == f && i.provides.iter().any(|c| c == "Container"))
    });
    let viewer = if container {
        Viewer::Container
    } else if has("uimg.json") {
        Viewer::Atlas
    } else if has("png") {
        Viewer::Image
    } else if has("wav") {
        Viewer::Audio
    } else if !text.is_empty() {
        Viewer::Text
    } else {
        Viewer::None
    };
    (viewer, text)
}

/// `GET /api/entry`: what the path is and how to show it.
pub async fn get_entry(State(app): State<App>, Query(query): Query<PathQuery>) -> Json2<EntryView> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let found = session
        .engine
        .discover(&path, &DiscoverOptions::default(), &session.cancel)
        .await?;
    let discovery = found.entries.into_iter().next();
    let (viewer, text_targets) = match &discovery {
        Some(d) => viewer_of(&session, d.format.as_deref(), &d.targets),
        None => (Viewer::None, Vec::new()),
    };
    let in_container = tree::split_container(&session, &path).is_some();
    let size = if in_container {
        None
    } else {
        session
            .input
            .stat(&path)
            .await
            .ok()
            .map(|m| m.content_length())
    };
    let (mut meta, mut bitmap, mut audio) = (None, None, None);
    if let Ok(value) = session.engine.open_read(&path, &session.cancel).await {
        meta = value.as_metadata().map(|m| {
            json!({
                "name": m.name(),
                "kind": m.kind(),
                "guid": m.guid().map(|g| format!("0x{g:016X}")),
                "dependencies": m.dependencies().iter().map(|d| json!({
                    "guid": format!("0x{:016X}", d.guid),
                    "name": d.name,
                })).collect::<Vec<_>>(),
            })
        });
        bitmap = value.as_bitmap().map(|b| {
            let i = b.info();
            json!({
                "width": i.width, "height": i.height, "depth": i.depth,
                "layers": i.layer_count(), "mips": i.mip_count,
                "pixelFormat": format!("{:?}", i.format),
            })
        });
        audio = value.as_audio().map(|a| {
            json!(a
                .tracks()
                .iter()
                .map(|t| json!({
                    "channels": t.channels, "sampleRate": t.sample_rate,
                }))
                .collect::<Vec<_>>())
        });
    }
    Ok(Json(EntryView {
        hex: !in_container && size.is_some(),
        path,
        discovery,
        viewer,
        size,
        meta,
        bitmap,
        audio,
        text_targets,
    }))
}

fn target(name: &str, options: serde_json::Map<String, Json_>) -> TargetSpec {
    TargetSpec {
        format: FormatId::new(name.to_owned()),
        via: Vec::new(),
        options,
    }
}

fn bytes_response(content_type: &str, bytes: Bytes) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from(bytes),
    )
        .into_response()
}

/// `GET /api/preview/image`: one mip as a PNG.
pub async fn preview_image(
    State(app): State<App>,
    Query(query): Query<PathQuery>,
) -> ApiResult<Response> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let mut options = serde_json::Map::new();
    options.insert("mips".into(), json!(query.mip.unwrap_or(0)));
    let files = session
        .preview(&path, target("png", options), false)
        .await?;
    let (_, png) = files
        .first()
        .ok_or_else(|| ApiError::not_found("no image was produced"))?;
    Ok(bytes_response("image/png", png.clone()))
}

/// `GET /api/preview/audio`: a track as a WAV file.
pub async fn preview_audio(
    State(app): State<App>,
    Query(query): Query<PathQuery>,
) -> ApiResult<Response> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let mut options = serde_json::Map::new();
    options.insert("track".into(), json!(query.track.unwrap_or(0)));
    let files = session
        .preview(&path, target("wav", options), false)
        .await?;
    let (_, wav) = files
        .first()
        .ok_or_else(|| ApiError::not_found("no audio was produced"))?;
    Ok(bytes_response("audio/wav", wav.clone()))
}

/// `GET /api/preview/atlas`: the sprites of a `uimg`, as the engine writes them in `uimg.json`.
pub async fn preview_atlas(State(app): State<App>, Query(query): Query<PathQuery>) -> Json2<Json_> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let files = session
        .preview(&path, target("uimg.json", Default::default()), true)
        .await?;
    let (_, json) = files
        .iter()
        .find(|(name, _)| name == "uimg.json")
        .ok_or_else(|| ApiError::not_found("no atlas description was produced"))?;
    let atlas: Json_ = serde_json::from_slice(json)
        .map_err(|e| ApiError::bad_request(format!("uimg.json is not valid: {e}")))?;
    Ok(Json(atlas))
}

/// `GET /api/preview/atlas-image`: the atlas texture of a `uimg` as a PNG.
pub async fn preview_atlas_image(
    State(app): State<App>,
    Query(query): Query<PathQuery>,
) -> ApiResult<Response> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let files = session
        .preview(&path, target("uimg.json", Default::default()), true)
        .await?;
    let (_, png) = files
        .iter()
        .find(|(name, _)| name.starts_with("atlas."))
        .ok_or_else(|| ApiError::not_found("no atlas image was produced"))?;
    Ok(bytes_response("image/png", png.clone()))
}

/// The largest text a preview returns.
const TEXT_LIMIT: usize = 2 << 20;

/// `GET /api/preview/text`: the entry converted to a text target.
pub async fn preview_text(
    State(app): State<App>,
    Query(query): Query<PathQuery>,
) -> ApiResult<Response> {
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    let name = query
        .target
        .ok_or_else(|| ApiError::bad_request("`target` is required"))?;
    let files = session
        .preview(&path, target(&name, Default::default()), true)
        .await?;
    let (_, data) = files
        .first()
        .ok_or_else(|| ApiError::not_found("nothing was produced"))?;
    let shown = data.slice(..data.len().min(TEXT_LIMIT));
    let text = String::from_utf8_lossy(&shown).into_owned();
    Ok(bytes_response("text/plain; charset=utf-8", text.into()))
}

/// `GET /api/preview/hex`: the first bytes of a file of the input.
pub async fn preview_hex(State(app): State<App>, Query(query): Query<PathQuery>) -> Json2<Json_> {
    const SHOWN: u64 = 1024;
    let session = app.need_session().await?;
    let path = clean(&query.path)?;
    if tree::split_container(&session, &path).is_some() {
        return Err(ApiError::bad_request(
            "entries of containers have no bytes of their own",
        ));
    }
    let size = session
        .input
        .stat(&path)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?
        .content_length();
    let data = session
        .input
        .read_with(&path)
        .range(0..SHOWN.min(size))
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?
        .to_bytes();
    let rows: Vec<Json_> = data
        .chunks(16)
        .enumerate()
        .map(|(i, row)| {
            json!({
                "offset": format!("{:08X}", i * 16),
                "hex": row.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
                "ascii": row.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect::<String>(),
            })
        })
        .collect();
    Ok(Json(json!({ "size": size, "rows": rows })))
}

// ----------------------------------------------------------------- convert

/// The body of `POST /api/convert`.
#[derive(Debug, Deserialize)]
pub struct ConvertBody {
    /// The basket.
    pub items: Vec<BasketItem>,
}

/// `POST /api/convert`: starts a job.
pub async fn start_job(State(app): State<App>, Json(body): Json<ConvertBody>) -> Json2<JobView> {
    let session = app.need_session().await?;
    if body.items.is_empty() {
        return Err(ApiError::bad_request("the basket is empty"));
    }
    let mut items = body.items;
    for item in &mut items {
        item.from = clean(&item.from)
            .map_err(|_| ApiError::bad_request("`..` is not allowed in a path"))?;
        if let Some(path) = &item.path {
            item.path = Some(clean(path)?);
        }
    }
    Ok(Json(app.jobs.start(session, items)))
}

/// `GET /api/jobs`.
pub async fn list_jobs(State(app): State<App>) -> Json2<Vec<JobView>> {
    Ok(Json(app.jobs.list()))
}

/// `DELETE /api/jobs/{id}`: cancels a running job, or removes a finished one.
pub async fn cancel_job(State(app): State<App>, Path(id): Path<u64>) -> Json2<Json_> {
    if app.jobs.cancel_or_dismiss(id) {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("no job {id}")))
    }
}

/// The query of `GET /api/output/…`.
#[derive(Debug, Deserialize)]
pub struct OutputQuery {
    /// Ask the browser to save the file.
    #[serde(default)]
    pub download: bool,
}

/// `GET /api/output/{path}`: a file written to the output directory.
pub async fn get_output(
    State(app): State<App>,
    Path(path): Path<String>,
    Query(query): Query<OutputQuery>,
) -> ApiResult<Response> {
    let session = app.need_session().await?;
    // Absolute and `..` paths are refused outright; the operator is rooted at the output
    // directory and could not leave it either.
    let path = clean(&path).map_err(|_| ApiError::not_found("no such file"))?;
    if path.is_empty() || path.contains(':') {
        return Err(ApiError::not_found("no such file"));
    }
    let data = session
        .output
        .read(&path)
        .await
        .map_err(|_| ApiError::not_found("no such file"))?
        .to_bytes();
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let name = path.rsplit('/').next().unwrap_or(&path).replace('"', "");
    let disposition = if query.download {
        "attachment"
    } else {
        "inline"
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("{disposition}; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from(data),
    )
        .into_response())
}

// ---------------------------------------------------------------- browsing

/// The query of `GET /api/fs`.
#[derive(Debug, Deserialize)]
pub struct FsQuery {
    /// Directory on this computer; empty lists the roots.
    #[serde(default)]
    pub path: String,
}

/// `GET /api/fs`: the directories of this computer, for choosing the game folder.
pub async fn browse(State(app): State<App>, Query(query): Query<FsQuery>) -> Json2<Json_> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    let shortcuts = json!({ "home": home, "cwd": app.cwd.display().to_string() });
    if query.path.trim().is_empty() {
        return Ok(Json(json!({
            "path": "",
            "parent": null,
            "dirs": roots(),
            "shortcuts": shortcuts,
        })));
    }
    let dir = PathBuf::from(&query.path);
    let read = std::fs::read_dir(&dir)
        .map_err(|e| ApiError::bad_request(format!("cannot open {}: {e}", dir.display())))?;
    let mut dirs = Vec::new();
    let mut files = 0usize;
    let mut containers = 0usize;
    for entry in read.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            dirs.push(json!({ "name": name, "path": entry.path().display().to_string() }));
        } else {
            files += 1;
            if name.ends_with(".rpak") || name.ends_with(".vpk") {
                containers += 1;
            }
        }
    }
    dirs.sort_by_key(|d| d["name"].as_str().unwrap_or("").to_lowercase());
    let parent = match dir.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Some(parent.display().to_string()),
        _ => Some(String::new()),
    };
    Ok(Json(json!({
        "path": dir.display().to_string(),
        "parent": parent,
        "dirs": dirs,
        "files": files,
        "containers": containers,
        "shortcuts": shortcuts,
    })))
}

#[cfg(windows)]
fn roots() -> Vec<Json_> {
    ('A'..='Z')
        .map(|c| format!("{c}:\\"))
        .filter(|root| FsPath::new(root).exists())
        .map(|root| json!({ "name": root, "path": root }))
        .collect()
}

#[cfg(not(windows))]
fn roots() -> Vec<Json_> {
    vec![json!({ "name": "/", "path": "/" })]
}

// ------------------------------------------------------------------- steam

/// `GET /api/steam/status`: whether a login is stored. The token is never returned.
pub async fn steam_status() -> Json2<Json_> {
    #[cfg(feature = "steam")]
    {
        let store = respak_steam::AccountStore::new(".respak");
        if let Ok(account) = store.load() {
            return Ok(Json(json!({
                "loggedIn": true,
                "accountName": if account.account_name.is_empty() {
                    format!("steam id {}", account.steam_id)
                } else {
                    account.account_name
                },
                "steamId": account.steam_id.to_string(),
            })));
        }
    }
    Ok(Json(json!({ "loggedIn": false })))
}

/// What a build without Steam says to the login endpoints.
#[cfg(not(feature = "steam"))]
fn no_steam() -> ApiError {
    ApiError::bad_request("this build has no Steam support")
}

/// `GET /api/steam/login`: whether a login is in progress.
pub async fn steam_login_state(State(app): State<App>) -> Json2<Json_> {
    #[cfg(feature = "steam")]
    return Ok(Json(json!({ "running": app.login.running() })));
    #[cfg(not(feature = "steam"))]
    {
        let _ = app;
        Ok(Json(json!({ "running": false })))
    }
}

/// `POST /api/steam/login/qr`: starts a QR login; the code and the outcome come as events.
pub async fn steam_login_qr(State(app): State<App>) -> Json2<Json_> {
    #[cfg(feature = "steam")]
    {
        app.login
            .start_qr(app.ui.clone(), respak_steam::AccountStore::new(".respak"))?;
        Ok(Json(json!({ "started": true })))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = app;
        Err(no_steam())
    }
}

/// The body of `POST /api/steam/login/credentials`. Never echoed back or logged.
#[derive(Deserialize)]
#[cfg_attr(not(feature = "steam"), allow(dead_code))]
pub struct Credentials {
    username: String,
    password: String,
}

/// `POST /api/steam/login/credentials`: starts a login with a password; Steam Guard follows as events.
pub async fn steam_login_credentials(
    State(app): State<App>,
    Json(body): Json<Credentials>,
) -> Json2<Json_> {
    #[cfg(feature = "steam")]
    {
        if body.username.trim().is_empty() || body.password.is_empty() {
            return Err(ApiError::bad_request(
                "the account name and the password are required",
            ));
        }
        app.login.start_credentials(
            app.ui.clone(),
            respak_steam::AccountStore::new(".respak"),
            body.username.trim().to_owned(),
            body.password,
        )?;
        Ok(Json(json!({ "started": true })))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = (app, body);
        Err(no_steam())
    }
}

/// The body of `POST /api/steam/login/code`.
#[derive(Deserialize)]
#[cfg_attr(not(feature = "steam"), allow(dead_code))]
pub struct GuardCode {
    code: String,
}

/// `POST /api/steam/login/code`: the Steam Guard code the user typed.
pub async fn steam_login_code(State(app): State<App>, Json(body): Json<GuardCode>) -> Json2<Json_> {
    #[cfg(feature = "steam")]
    {
        let code = body.code.trim().to_owned();
        if code.is_empty() {
            return Err(ApiError::bad_request("the code is empty"));
        }
        app.login.code(code)?;
        Ok(Json(json!({ "ok": true })))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = (app, body);
        Err(no_steam())
    }
}

/// `DELETE /api/steam/login`: gives up on the login in progress.
pub async fn steam_login_cancel(State(app): State<App>) -> Json2<Json_> {
    #[cfg(feature = "steam")]
    app.login.cancel();
    #[cfg(not(feature = "steam"))]
    let _ = app;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/steam/logout`: deletes the stored login.
pub async fn steam_logout() -> Json2<Json_> {
    #[cfg(feature = "steam")]
    respak_steam::AccountStore::new(".respak")
        .delete()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({ "loggedIn": false })))
}

// ----------------------------------------------------------------- catalog

/// Where the user's own catalog files are: beside the settings, with the other state of respak.
fn catalog_dir(app: &App) -> PathBuf {
    app.cwd.join(".respak").join("catalog")
}

/// `GET /api/catalog`: the games and versions that can be picked.
pub async fn get_catalog(State(app): State<App>) -> Json2<crate::catalog::CatalogView> {
    let dir = catalog_dir(&app);
    Ok(Json(
        crate::catalog::Catalog::load(Some(&dir)).view(Some(&dir)),
    ))
}

/// The body of `POST /api/catalog/resolve`.
#[derive(Debug, Deserialize)]
pub struct ResolveBody {
    /// The key of the game.
    pub game: String,
    /// The id of the version.
    pub version: String,
    /// The depots to resolve; the game's default depots when absent.
    pub depots: Option<Vec<u32>>,
}

/// `POST /api/catalog/resolve`: the pinned `ID=MANIFEST` list of a version.
pub async fn resolve_catalog(
    State(app): State<App>,
    Json(body): Json<ResolveBody>,
) -> Json2<crate::catalog::Resolved> {
    let catalog = crate::catalog::Catalog::load(Some(&catalog_dir(&app)));
    let game = catalog.games.get(&body.game).ok_or_else(|| {
        ApiError::not_found(crate::catalog::ResolveError::Game(body.game.clone()).to_string())
    })?;
    let resolved = game
        .resolve(&body.version, body.depots.as_deref())
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    Ok(Json(resolved))
}

// ------------------------------------------------------------------- misc

/// `GET /api/info`.
pub async fn info(State(app): State<App>) -> Json2<Json_> {
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "cwd": app.cwd.display().to_string(),
        "steam": cfg!(feature = "steam"),
    })))
}

/// Anything under `/api` that is not an endpoint.
pub async fn not_found() -> impl IntoResponse {
    ApiError::new(StatusCode::NOT_FOUND, "NotFound", "no such endpoint")
}
