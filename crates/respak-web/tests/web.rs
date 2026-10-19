//! The server end to end, without a network: requests go straight into the router.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use respak_respawn_testing::{build_tex_pak, TexSpec};
use respak_web::{router, App};
use serde_json::{json, Value};
use tower::ServiceExt;

const PORT: u16 = 4321;

struct Harness {
    dir: tempfile::TempDir,
    app: Router,
}

impl Harness {
    /// A game folder with a pak holding one texture, and a working folder with no settings yet.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        std::fs::create_dir_all(root.join("paks")).unwrap();
        std::fs::create_dir_all(root.join("misc")).unwrap();
        std::fs::write(root.join("notes.txt"), b"hello").unwrap();
        std::fs::write(
            root.join("paks/tex.rpak"),
            build_tex_pak(8, &[TexSpec::new(0xABCD, 10, 4, 4, 0, 1, vec![7; 16])]),
        )
        .unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let app = router(App::new(work), PORT);
        Self { dir, app }
    }

    fn root(&self) -> String {
        self.dir.path().join("game").display().to_string()
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, body.to_vec())
    }

    fn request(method: Method, uri: &str, body: Option<&Value>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, format!("127.0.0.1:{PORT}"));
        let body = match body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        builder.body(body).unwrap()
    }

    async fn get(&self, uri: &str) -> (StatusCode, Vec<u8>) {
        let (status, _, body) = self.send(Self::request(Method::GET, uri, None)).await;
        (status, body)
    }

    async fn get_json(&self, uri: &str) -> Value {
        let (status, body) = self.get(uri).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{uri}: {}",
            String::from_utf8_lossy(&body)
        );
        serde_json::from_slice(&body).unwrap()
    }

    async fn call(&self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
        let (status, _, bytes) = self.send(Self::request(method, uri, Some(&body))).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Saves settings that point at the game folder and connects.
    async fn connect(&self) {
        let config = json!({ "root": self.root(), "out": self.dir.path().join("out").display().to_string() });
        let (status, body) = self.call(Method::PUT, "/api/config", config).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["problem"], Value::Null);
        let (status, body) = self.call(Method::POST, "/api/session", json!({})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
}

fn names(page: &Value) -> Vec<String> {
    page["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_foreign_host_origin_or_content_type_is_refused() {
    let h = Harness::new();

    let wrong_host = Request::builder()
        .uri("/api/info")
        .header(header::HOST, "evil.example:4321")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.send(wrong_host).await.0, StatusCode::FORBIDDEN);

    let foreign_origin = Request::builder()
        .uri("/api/info")
        .header(header::HOST, format!("127.0.0.1:{PORT}"))
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.send(foreign_origin).await.0, StatusCode::FORBIDDEN);

    let cross_site = Request::builder()
        .uri("/api/info")
        .header(header::HOST, format!("127.0.0.1:{PORT}"))
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.send(cross_site).await.0, StatusCode::FORBIDDEN);

    let form = Request::builder()
        .method(Method::POST)
        .uri("/api/session")
        .header(header::HOST, format!("127.0.0.1:{PORT}"))
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(h.send(form).await.0, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Its own origin is fine, and nothing grants other sites anything.
    let own = Request::builder()
        .uri("/api/info")
        .header(header::HOST, format!("127.0.0.1:{PORT}"))
        .header(header::ORIGIN, format!("http://127.0.0.1:{PORT}"))
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = h.send(own).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers
        .keys()
        .all(|k| !k.as_str().starts_with("access-control-")));
}

#[tokio::test]
async fn settings_are_written_to_respak_toml_and_typos_are_refused() {
    let h = Harness::new();
    let before = h.get_json("/api/config").await;
    assert_eq!(before["exists"], false);

    h.connect().await;
    let file = h.dir.path().join("work/respak.toml");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("root =") && text.contains("out ="), "{text}");
    let after = h.get_json("/api/config").await;
    assert_eq!(after["exists"], true);
    assert_eq!(after["config"]["root"], h.root());

    let (status, body) = h
        .call(Method::PUT, "/api/config", json!({ "rooot": "x" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[tokio::test]
async fn nothing_works_before_connecting() {
    let h = Harness::new();
    let (status, body) = h.get("/api/tree").await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "{}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn the_tree_is_lazy_and_opens_a_pak_only_when_asked() {
    let h = Harness::new();
    h.connect().await;

    let root = h.get_json("/api/tree?path=").await;
    assert_eq!(names(&root), ["misc", "paks", "notes.txt"]);

    let paks = h.get_json("/api/tree?path=paks").await;
    assert_eq!(paks["nodes"][0]["kind"], "container");
    // Listing the folder did not open the pak.
    let session = h.get_json("/api/session").await;
    assert_eq!(session["connected"], true);
    let found = h.get_json("/api/search?q=abcd&scope=loaded").await;
    assert_eq!(found["hits"].as_array().unwrap().len(), 0);

    let inside = h.get_json("/api/tree?path=paks/tex.rpak").await;
    assert_eq!(inside["container"], "paks/tex.rpak");
    let asset = &inside["nodes"][0];
    assert_eq!(asset["kind"], "asset");
    assert_eq!(asset["path"], "paks/tex.rpak/0x000000000000ABCD.txtr");

    let kinds = h.get_json("/api/tree?path=paks/tex.rpak&view=kind").await;
    assert_eq!(kinds["nodes"][0]["kind"], "group");
    assert_eq!(kinds["nodes"][0]["ext"], "txtr");
    let group = h
        .get_json("/api/tree?path=paks/tex.rpak&view=kind&group=txtr")
        .await;
    assert_eq!(group["total"], 1);

    // Now it is opened: the assets are searchable.
    let found = h.get_json("/api/search?q=abcd&scope=loaded").await;
    assert_eq!(found["hits"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn paging_stops_at_the_limit() {
    let h = Harness::new();
    let root = h.root();
    for i in 0..5 {
        std::fs::write(std::path::Path::new(&root).join(format!("f{i}.bin")), b"x").unwrap();
    }
    h.connect().await;
    let page = h.get_json("/api/tree?path=&limit=3").await;
    assert_eq!(page["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(page["total"], 8);
    let rest = h.get_json("/api/tree?path=&offset=3&limit=3").await;
    assert_eq!(rest["nodes"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn file_names_are_searched_without_regard_to_case() {
    let h = Harness::new();
    h.connect().await;
    let found = h.get_json("/api/search?q=NOTES&scope=files").await;
    assert_eq!(found["hits"][0]["path"], "notes.txt");
    let found = h.get_json("/api/search?q=pak+tex&scope=files").await;
    assert_eq!(found["hits"][0]["path"], "paks/tex.rpak");
}

#[tokio::test]
async fn a_texture_is_recognised_and_previewed_as_a_png() {
    let h = Harness::new();
    h.connect().await;
    let path = "paks/tex.rpak/0x000000000000ABCD.txtr";
    let entry = h.get_json(&format!("/api/entry?path={path}")).await;
    assert_eq!(entry["viewer"], "image", "{entry}");
    assert_eq!(entry["format"], "txtr");

    let (status, png) = h
        .get(&format!("/api/preview/image?path={path}&mip=0"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (4, 4));

    // A mip that does not exist is an error the page can show, not a crash.
    let (status, _) = h
        .get(&format!("/api/preview/image?path={path}&mip=9"))
        .await;
    assert!(status.is_client_error() || status.is_server_error());
    // Previews never reach the output directory.
    assert!(
        !h.dir.path().join("out").exists()
            || std::fs::read_dir(h.dir.path().join("out"))
                .unwrap()
                .next()
                .is_none()
    );
}

#[tokio::test]
async fn a_job_converts_downloads_and_reports_failures_per_output() {
    let h = Harness::new();
    h.connect().await;
    let items = json!({ "items": [
        { "from": "paks/tex.rpak/0x000000000000ABCD.txtr", "to": { "type": "png" } },
        { "from": "notes.txt", "to": { "type": "png" } },
    ]});
    let (status, job) = h.call(Method::POST, "/api/convert", items).await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let id = job["id"].as_u64().unwrap();

    let mut view = Value::Null;
    for _ in 0..200 {
        let jobs = h.get_json("/api/jobs").await;
        view = jobs
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == id)
            .unwrap()
            .clone();
        if view["state"] != "running" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(view["state"], "done", "{view}");
    // The texture came out; the text file has no way to a PNG and says so, without stopping anything.
    let good = &view["rows"][0]["outputs"][0];
    assert_eq!(good["ok"], true, "{view}");
    let file = good["files"][0].as_str().unwrap();
    let (status, bytes) = h.get(&format!("/api/output/{file}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[1..4], b"PNG");
    let bad = &view["rows"][1];
    assert!(
        bad["error"].is_object() || bad["outputs"][0]["ok"] == false,
        "{view}"
    );

    // Nothing outside the output directory is served.
    for path in [
        "..%2Fgame%2Fnotes.txt",
        "%2E%2E/game/notes.txt",
        "C:/Windows/win.ini",
        "/etc/passwd",
    ] {
        let (status, _) = h.get(&format!("/api/output/{path}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }

    let (status, _) = h
        .call(Method::DELETE, &format!("/api/jobs/{id}"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(h.get_json("/api/jobs").await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_job_can_be_cancelled() {
    let h = Harness::new();
    h.connect().await;
    let items = json!({ "items": [
        { "from": "paks/tex.rpak/0x000000000000ABCD.txtr", "to": { "type": "png" } },
    ]});
    let (_, job) = h.call(Method::POST, "/api/convert", items).await;
    let id = job["id"].as_u64().unwrap();
    let (status, _) = h
        .call(Method::DELETE, &format!("/api/jobs/{id}"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h.call(Method::DELETE, "/api/jobs/9999", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_frontend_is_served_for_any_page_path() {
    let h = Harness::new();
    let (status, body) = h.get("/some/page").await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains("<div id=\"app\">"));
    let (status, _) = h.get("/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_catalog_merges_user_files_and_resolves_a_version() {
    let h = Harness::new();
    let dir = h.dir.path().join("work/.respak/catalog");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mine.toml"),
        r#"
        [apex]
        name = "Apex Legends"
        app_id = 1172470
        [[apex.versions]]
        id = "a"
        name = "First"
        date = 2024-01-01
        group = "season-1"
        manifests = [ { depot = 1172471, manifest = "111" } ]
        [[apex.versions]]
        id = "b"
        name = "Second"
        date = 2024-02-01
        "#,
    )
    .unwrap();
    std::fs::write(dir.join("broken.toml"), "[apex").unwrap();

    let catalog = h.get_json("/api/catalog").await;
    let apex = catalog["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["key"] == "apex")
        .unwrap();
    // Newest first; the shipped depot is still there.
    assert_eq!(apex["versions"][0]["id"], "b");
    assert_eq!(apex["depots"][0]["id"], 1172471);
    assert_eq!(catalog["problems"].as_array().unwrap().len(), 1);

    // `b` lists no depot of its own: it takes the manifest of the closest earlier version.
    let (status, body) = h
        .call(
            Method::POST,
            "/api/catalog/resolve",
            json!({ "game": "apex", "version": "b" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["depots"], json!(["1172471=111"]));
    assert_eq!(body["app"], 1172470);

    let (status, _) = h
        .call(
            Method::POST,
            "/api/catalog/resolve",
            json!({ "game": "apex", "version": "nope" }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[cfg(feature = "steam")]
#[tokio::test]
async fn the_steam_login_endpoints_refuse_what_makes_no_sense_and_never_show_a_token() {
    let h = Harness::new();
    // No code is being asked for.
    let (status, _) = h
        .call(
            Method::POST,
            "/api/steam/login/code",
            json!({ "code": "12345" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // A login needs both fields.
    let (status, _) = h
        .call(
            Method::POST,
            "/api/steam/login/credentials",
            json!({ "username": "", "password": "" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // The status says whether there is a login, and nothing more.
    let status = h.get_json("/api/steam/status").await;
    assert!(status.get("refreshToken").is_none() && status.get("refresh_token").is_none());
    assert_eq!(h.get_json("/api/steam/login").await["running"], false);
}
