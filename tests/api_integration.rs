//! End-to-end HTTP integration tests for ToolStow v0.2:
//! fail-closed read-only mode, admin session login/logout/rate-limiting, site settings,
//! dual-key isolation, Hugo-style draft visibility & PATCH toggle, decoupled `data/theme/`
//! hot overrides, CSRF/Origin/Zip-Slip/413/CSP security matrix, and real bundled examples.

use std::fs;
use std::io::{Cursor, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_ORIGIN, AUTHORIZATION, CACHE_CONTROL, CONTENT_SECURITY_POLICY,
    CONTENT_TYPE, COOKIE, HOST, LOCATION, ORIGIN, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use toolstow::auth::{AuthManager, SESSION_COOKIE_NAME};
use toolstow::config::Config;
use toolstow::registry::ToolRegistry;
use toolstow::routes::{AppState, build_router};
use toolstow::settings::SettingsStore;

const BOUNDARY: &str = "----ToolStowTestBoundary7MA4YWxkTrZu0gW";
const ADMIN_KEY: &str = "admin-secret-key-at-least-16-chars";
const API_KEY: &str = "api-machine-key-at-least-16-chars";

struct TestEnv {
    _temp: TempDir,
    data_dir: PathBuf,
    tools_dir: PathBuf,
    theme_dir: PathBuf,
    themes_dir: PathBuf,
    app: Router,
    registry: Arc<ToolRegistry>,
}

fn build_test_env(
    admin_key: Option<&str>,
    api_key: Option<&str>,
    max_upload_mb: usize,
    include_drafts: bool,
    cookie_secure: bool,
) -> TestEnv {
    let temp = TempDir::new().expect("create temp dir");
    let data_dir = temp.path().join("data");
    let tools_dir = data_dir.join("tools");
    let theme_dir = data_dir.join("theme");
    let themes_dir = data_dir.join("themes");
    fs::create_dir_all(&tools_dir).expect("create tools dir");
    fs::create_dir_all(&themes_dir).expect("create themes dir");

    let config = Config {
        data_dir: data_dir.clone(),
        tools_dir: tools_dir.clone(),
        theme_dir: theme_dir.clone(),
        themes_dir: themes_dir.clone(),
        max_upload_mb,
        include_drafts,
        cookie_secure,
        admin_key: admin_key.map(str::to_string),
        api_key: api_key.map(str::to_string),
        ..Config::default()
    };

    let registry = Arc::new(ToolRegistry::new(tools_dir.clone()));
    let settings = SettingsStore::load(config.settings_path());
    let auth = AuthManager::new(
        config.admin_key.clone(),
        config.api_key.clone(),
        config.cookie_secure,
    );

    let app = build_router(AppState {
        registry: Arc::clone(&registry),
        config: Arc::new(config),
        settings,
        auth,
    });

    TestEnv {
        _temp: temp,
        data_dir,
        tools_dir,
        theme_dir,
        themes_dir,
        app,
        registry,
    }
}

fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for (name, data) in entries {
        writer
            .start_file(*name, options)
            .expect("start_file in test zip");
        writer.write_all(data).expect("write data to test zip");
    }

    writer
        .finish()
        .expect("finish test zip")
        .into_inner()
}

fn make_stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    for (name, data) in entries {
        writer
            .start_file(*name, options)
            .expect("start_file in stored zip");
        writer.write_all(data).expect("write data to stored zip");
    }

    writer
        .finish()
        .expect("finish stored zip")
        .into_inner()
}

fn make_zip_from_dir(dir: &Path, root_prefix: Option<&str>) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    fn add_recursive(
        writer: &mut ZipWriter<Cursor<Vec<u8>>>,
        base: &Path,
        current: &Path,
        prefix: Option<&str>,
        options: SimpleFileOptions,
    ) {
        let mut entries: Vec<_> = fs::read_dir(current)
            .expect("read_dir")
            .map(|e| e.expect("dir entry"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                add_recursive(writer, base, &path, prefix, options);
            } else if path.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .expect("strip_prefix")
                    .to_string_lossy()
                    .replace('\\', "/");
                let zip_name = match prefix {
                    Some(p) => format!("{p}/{rel}"),
                    None => rel,
                };
                writer.start_file(zip_name, options).expect("start_file");
                let bytes = fs::read(&path).expect("read example file");
                writer.write_all(&bytes).expect("write example file");
            }
        }
    }

    add_recursive(&mut writer, dir, dir, root_prefix, options);
    writer.finish().expect("finish dir zip").into_inner()
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dst dir");
    for entry in fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let ty = entry.file_type().expect("file_type");
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path);
        } else if ty.is_file() {
            fs::copy(entry.path(), dst_path).expect("copy file");
        }
    }
}

fn create_disk_tool(tools_dir: &Path, id: &str, manifest_json: &str, html: &str) {
    let dir = tools_dir.join(id);
    fs::create_dir_all(&dir).expect("create tool dir");
    fs::write(dir.join("manifest.json"), manifest_json).expect("write manifest");
    fs::write(dir.join("index.html"), html).expect("write index.html");
}

fn build_multipart_body(field_name: &str, filename: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{field_name}\"; filename=\"{filename}\"\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
    body.extend_from_slice(payload);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn admin_upload_request_base(zip_bytes: &[u8]) -> Request<Body> {
    let body = build_multipart_body("file", "tool.zip", zip_bytes);
    Request::builder()
        .method(Method::POST)
        .uri("/api/admin/tools/upload")
        .header(HOST, "127.0.0.1:8080")
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("build upload request")
}

fn bearer_upload_request(zip_bytes: &[u8], token: &str) -> Request<Body> {
    let mut req = admin_upload_request_base(zip_bytes);
    req.headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    req
}

fn with_peer_ip(mut req: Request<Body>, octets: [u8; 4]) -> Request<Body> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), 54321);
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

fn extract_cookie_pair(resp: &axum::response::Response) -> String {
    let set_cookie = resp
        .headers()
        .get(SET_COOKIE)
        .expect("expected Set-Cookie header")
        .to_str()
        .expect("utf-8 Set-Cookie");
    set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .trim()
        .to_string()
}

async fn login_as_admin(app: &Router, key: &str) -> String {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/admin/login")
        .header(HOST, "127.0.0.1:8080")
        .header(ORIGIN, "http://127.0.0.1:8080")
        .header("x-toolstow-client", "1")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "key": key }).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    extract_cookie_pair(&resp)
}

async fn read_body_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes()
        .to_vec()
}

async fn read_body_json(response: axum::response::Response) -> Value {
    let bytes = read_body_bytes(response).await;
    serde_json::from_slice(&bytes).expect("parse json response")
}

#[tokio::test]
async fn read_only_fail_closed_mode_and_draft_visibility() {
    let env = build_test_env(None, None, 50, false, false);

    create_disk_tool(
        &env.tools_dir,
        "public-tool",
        r#"{"id":"public-tool","name":"Public Tool","category":"Dev","tags":["util"]}"#,
        "<h1>Public</h1>",
    );
    create_disk_tool(
        &env.tools_dir,
        "secret-draft",
        r#"{"id":"secret-draft","name":"Secret Draft","category":"Dev","draft":true}"#,
        "<h1>Draft</h1>",
    );
    assert_eq!(env.registry.scan_all().unwrap(), 2);

    // GET /api/me -> defaults, isAdmin false, adminEnabled false, themeSource embedded, no-store
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-store");
    let me = read_body_json(resp).await;
    assert_eq!(me["siteTitle"], "ToolStow");
    assert_eq!(me["siteDescription"], "Personal Micro-App Toolbox");
    assert_eq!(me["isAdmin"], false);
    assert_eq!(me["adminEnabled"], false);
    assert_eq!(me["activeTheme"], "default");
    assert_eq!(me["themeSource"], "embedded");

    // GET /api/admin/session -> 200 JSON { authenticated: false, adminEnabled: false }
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-store");
    let session = read_body_json(resp).await;
    assert_eq!(session["authenticated"], false);
    assert_eq!(session["adminEnabled"], false);

    // /admin and /admin/* -> 404 when admin disabled
    for uri in [
        "/admin",
        "/admin/",
        "/admin/admin.js",
        "/admin/admin.css",
        "/admin/ui.css",
    ] {
        let resp = env
            .app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "expected 404 for {uri}");
    }

    // All /api/admin/* write/read endpoints -> 404 fail-closed
    let admin_probes = [
        (Method::GET, "/api/admin/tools"),
        (Method::GET, "/api/admin/settings"),
        (Method::PUT, "/api/admin/settings"),
        (Method::PATCH, "/api/admin/tools/public-tool"),
        (Method::DELETE, "/api/admin/tools/public-tool"),
        // Old v0.1 unauthenticated routes must also be 404
        (Method::POST, "/api/tools/upload"),
        (Method::DELETE, "/api/tools/public-tool"),
    ];
    for (method, uri) in admin_probes {
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "expected 404 for {method} {uri}"
        );
    }

    // POST /api/admin/login -> 404 when admin disabled
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/login")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"key": "anything"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // POST /api/admin/tools/upload -> 404 fail-closed
    let dummy_zip = make_zip(&[
        ("manifest.json", br#"{"id":"x","name":"X"}"#),
        ("index.html", b"x"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(admin_upload_request_base(&dummy_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Public GET /api/tools hides drafts
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let tools = read_body_json(resp).await;
    let arr = tools.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "public-tool");
    assert_eq!(arr[0]["draft"], false);

    // Anonymous access to draft tool returns 404 on redirect and asset paths
    for uri in ["/t/secret-draft", "/t/secret-draft/", "/t/secret-draft/index.html"] {
        let resp = env
            .app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "expected 404 for {uri}");
    }

    // Public tool is accessible
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/public-tool/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>Public</h1>");

    // Embedded frontend assets served without shadowing /api/*
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()[CONTENT_TYPE].to_str().unwrap().contains("text/html"));

    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/app.js").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let js = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(js.contains("/api/me") || js.contains("ToolStowHost"));

    // With include_drafts = true, drafts ARE exposed publicly
    let dev_env = build_test_env(None, None, 50, true, false);
    create_disk_tool(
        &dev_env.tools_dir,
        "local-draft",
        r#"{"id":"local-draft","name":"Local Draft","draft":true}"#,
        "<h1>Dev Draft</h1>",
    );
    assert_eq!(dev_env.registry.scan_all().unwrap(), 1);

    let resp = dev_env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 1);

    let resp = dev_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/local-draft/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>Dev Draft</h1>");
}

#[tokio::test]
async fn admin_session_rate_limit_cookie_flags_settings_and_draft_preview() {
    let env = build_test_env(Some(ADMIN_KEY), None, 50, false, false);

    create_disk_tool(
        &env.tools_dir,
        "preview-draft",
        r#"{"id":"preview-draft","name":"Preview Draft","draft":true}"#,
        "<h1>Draft Preview OK</h1>",
    );
    assert_eq!(env.registry.scan_all().unwrap(), 1);

    // /admin, /admin/, /admin/admin.css, /admin/admin.js served when admin_key configured
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/admin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-store");
    let admin_html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(admin_html.contains("/admin/admin.js"));

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/admin.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()[CONTENT_TYPE].to_str().unwrap().contains("javascript"));

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/admin.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()[CONTENT_TYPE].to_str().unwrap().contains("text/css"));

    // Rate limit: 5 wrong attempts from 10.0.0.1 -> 401; 6th attempt -> 429 even with valid key
    for attempt in 1..=5 {
        let req = with_peer_ip(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/login")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"key": "wrong-key"}).to_string()))
                .unwrap(),
            [10, 0, 0, 1],
        );
        let resp = env.app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "attempt #{attempt}");
    }

    let rate_limited_req = with_peer_ip(
        Request::builder()
            .method(Method::POST)
            .uri("/api/admin/login")
            .header(HOST, "127.0.0.1:8080")
            .header(ORIGIN, "http://127.0.0.1:8080")
            .header("x-toolstow-client", "1")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"key": ADMIN_KEY}).to_string()))
            .unwrap(),
        [10, 0, 0, 1],
    );
    let resp = env.app.clone().oneshot(rate_limited_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(read_body_json(resp).await["code"], "too_many_requests");

    // Different client IP 10.0.0.2 over plain HTTP -> Set-Cookie HttpOnly; SameSite=Strict (no Secure)
    let plain_login = with_peer_ip(
        Request::builder()
            .method(Method::POST)
            .uri("/api/admin/login")
            .header(HOST, "127.0.0.1:8080")
            .header(ORIGIN, "http://127.0.0.1:8080")
            .header("x-toolstow-client", "1")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"key": ADMIN_KEY}).to_string()))
            .unwrap(),
        [10, 0, 0, 2],
    );
    let resp = env.app.clone().oneshot(plain_login).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp.headers()[SET_COOKIE].to_str().unwrap().to_string();
    assert!(set_cookie.starts_with(&format!("{SESSION_COOKIE_NAME}=")));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Strict"));
    assert!(set_cookie.contains("Path=/"));
    assert!(!set_cookie.contains("; Secure"));
    let cookie_pair = extract_cookie_pair(&resp);
    assert_eq!(read_body_json(resp).await["ok"], true);

    // Login via X-Forwarded-Proto: https -> includes `; Secure`
    let https_login = with_peer_ip(
        Request::builder()
            .method(Method::POST)
            .uri("/api/admin/login")
            .header(HOST, "tools.example.com")
            .header(ORIGIN, "https://tools.example.com")
            .header("x-toolstow-client", "1")
            .header("x-forwarded-proto", "https")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"key": ADMIN_KEY}).to_string()))
            .unwrap(),
        [10, 0, 0, 3],
    );
    let resp = env.app.clone().oneshot(https_login).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let https_set_cookie = resp.headers()[SET_COOKIE].to_str().unwrap();
    assert!(https_set_cookie.contains("; Secure"));

    // GET /api/me with valid cookie -> isAdmin: true
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let me = read_body_json(resp).await;
    assert_eq!(me["isAdmin"], true);
    assert_eq!(me["adminEnabled"], true);

    // GET /api/admin/session with valid cookie -> authenticated: true
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/session")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let session = read_body_json(resp).await;
    assert_eq!(session["authenticated"], true);
    assert_eq!(session["adminEnabled"], true);

    // GET & PUT /api/admin/settings persists to settings.json and updates /api/me
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await["siteTitle"], "ToolStow");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/settings")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie_pair)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "siteTitle": "My Personal Lab",
                        "siteDescription": "Monochrome self-hosted utilities"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["siteTitle"], "My Personal Lab");
    assert_eq!(updated["siteDescription"], "Monochrome self-hosted utilities");

    // Verify persisted settings.json on disk
    let persisted_raw = fs::read_to_string(env.data_dir.join("settings.json")).unwrap();
    let persisted: Value = serde_json::from_str(&persisted_raw).unwrap();
    assert_eq!(persisted["siteTitle"], "My Personal Lab");
    assert_eq!(persisted["siteDescription"], "Monochrome self-hosted utilities");

    // Reflected immediately on /api/me
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let me = read_body_json(resp).await;
    assert_eq!(me["siteTitle"], "My Personal Lab");
    assert_eq!(me["siteDescription"], "Monochrome self-hosted utilities");

    // Draft preview: anonymous -> 404, admin cookie -> 307 / 200
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/preview-draft/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/preview-draft")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        resp.headers()[LOCATION].to_str().unwrap(),
        "/t/preview-draft/index.html"
    );

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/preview-draft/index.html")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>Draft Preview OK</h1>");

    // Logout revokes session and clears cookie
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/logout")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.headers()[SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));

    // Revoked cookie no longer grants access
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/preview-draft/index.html")
                .header(COOKIE, &cookie_pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dual_key_isolation_and_ai_bearer_crud_workflow() {
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 50, false, false);

    // 1. TOOLSTOW_API_KEY MUST be rejected on /api/admin/login
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/login")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "key": API_KEY }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. TOOLSTOW_API_KEY Bearer MUST be rejected on GET & PUT /api/admin/settings
    for method in [Method::GET, Method::PUT] {
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri("/api/admin/settings")
                    .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "siteTitle": "Hijacked" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "API key must not unlock {method} /api/admin/settings"
        );
    }

    // 3. TOOLSTOW_ADMIN_KEY MUST be rejected as Bearer on /api/admin/tools
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/tools")
                .header(AUTHORIZATION, format!("Bearer {ADMIN_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 4. TOOLSTOW_API_KEY succeeds on POST /api/admin/tools/upload (201 Created, draft: true)
    let v1_zip = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "ai-tool",
                "name": "AI Generated Tool",
                "version": "1.0.0",
                "description": "Draft tool",
                "category": "AI",
                "tags": ["agent", "draft"],
                "draft": true,
                "sandbox": ["allow-scripts", "allow-same-origin", "allow-forms"]
            }"#,
        ),
        ("index.html", b"<h1>AI Tool v1</h1>"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&v1_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = read_body_json(resp).await;
    assert_eq!(created["id"], "ai-tool");
    assert_eq!(created["draft"], true);
    assert!(!created["sandbox"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "allow-same-origin"));

    // Overwrite same ID -> 200 OK
    let v2_zip = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "ai-tool",
                "name": "AI Generated Tool",
                "version": "2.0.0",
                "description": "Updated draft",
                "category": "AI",
                "tags": ["agent"],
                "draft": true
            }"#,
        ),
        ("index.html", b"<h1>AI Tool v2</h1>"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&v2_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await["version"], "2.0.0");

    // Public /api/tools hides it while draft == true, admin /api/admin/tools lists it
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 0);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/tools?draft=true&category=ai")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = read_body_json(resp).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], "ai-tool");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/tools?draft=false")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 0);

    // 5. PATCH /api/admin/tools/ai-tool -> publish (draft: false), persisted to manifest.json
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/admin/tools/ai-tool")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "draft": false }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = read_body_json(resp).await;
    assert_eq!(patched["id"], "ai-tool");
    assert_eq!(patched["draft"], false);

    // Persisted on disk in manifest.json
    let manifest_raw = fs::read_to_string(env.tools_dir.join("ai-tool/manifest.json")).unwrap();
    let manifest_json: Value = serde_json::from_str(&manifest_raw).unwrap();
    assert_eq!(manifest_json["draft"], false);
    assert_eq!(manifest_json["version"], "2.0.0");

    // Immediately visible in public GET /api/tools and GET /t/ai-tool/index.html
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let pub_list = read_body_json(resp).await;
    assert_eq!(pub_list.as_array().unwrap().len(), 1);
    assert_eq!(pub_list[0]["id"], "ai-tool");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/ai-tool/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>AI Tool v2</h1>");

    // 6. DELETE /api/admin/tools/ai-tool -> 204, second delete -> 404
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/tools/ai-tool")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/tools/ai-tool")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn decoupled_custom_theme_hot_override_route_locking_and_symlink_protection() {
    let env = build_test_env(Some(ADMIN_KEY), None, 50, false, false);

    create_disk_tool(
        &env.tools_dir,
        "demo",
        r#"{"id":"demo","name":"Demo"}"#,
        "<h1>Real Tool</h1>",
    );
    assert_eq!(env.registry.scan_all().unwrap(), 1);

    // Initially embedded
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await["themeSource"], "embedded");

    // Hot-drop custom theme into data/theme/
    fs::create_dir_all(env.theme_dir.join("admin")).unwrap();
    fs::create_dir_all(env.theme_dir.join("api")).unwrap();
    fs::create_dir_all(env.theme_dir.join("t/demo")).unwrap();
    fs::write(
        env.theme_dir.join("index.html"),
        "<!doctype html><h1>Custom Theme Live</h1>",
    )
    .unwrap();
    fs::write(env.theme_dir.join("custom.css"), "body { color: black; }").unwrap();
    // Attempt to shadow locked routes
    fs::write(env.theme_dir.join("admin/index.html"), "<h1>FAKE ADMIN</h1>").unwrap();
    fs::write(env.theme_dir.join("api/tools"), "FAKE API").unwrap();
    fs::write(env.theme_dir.join("t/demo/index.html"), "<h1>FAKE TOOL</h1>").unwrap();

    // /api/me immediately reports themeSource: "custom"
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await["themeSource"], "custom");

    // GET / serves custom theme index.html
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(
        read_body_bytes(resp).await,
        b"<!doctype html><h1>Custom Theme Live</h1>"
    );

    // GET /custom.css serves custom theme CSS
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/custom.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()[CONTENT_TYPE].to_str().unwrap().contains("text/css"));
    assert_eq!(read_body_bytes(resp).await, b"body { color: black; }");

    // Missing file in custom theme returns 404 (not embedded app.js)
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Locked routes /admin, /api/tools, /t/demo/index.html are NEVER shadowed!
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/admin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let admin_html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(admin_html.contains("ToolStow Admin"));
    assert!(!admin_html.contains("FAKE ADMIN"));

    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await[0]["id"], "demo");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/demo/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>Real Tool</h1>");

    // Symlink escape inside custom theme directory must be blocked with 403
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        let secret = outside.path().join("outside-secret.txt");
        fs::write(&secret, "top-secret").unwrap();
        symlink(&secret, env.theme_dir.join("leak.txt")).unwrap();

        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/leak.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // Removing data/theme/index.html immediately falls back to embedded frontend
    fs::remove_file(env.theme_dir.join("index.html")).unwrap();
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await["themeSource"], "embedded");
}

#[tokio::test]
async fn security_matrix_zip_slip_oversized_csrf_and_sandbox_csp() {
    // 1 MiB limit for fast 413 testing
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 1, false, false);
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;

    // Seed valid tool
    let good_zip = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "sec-tool",
                "name": "Security Tool",
                "entry": "app/main.html",
                "sandbox": ["allow-scripts", "allow-same-origin", "allow-downloads"]
            }"#,
        ),
        ("app/main.html", b"<!doctype html><p>safe</p>"),
    ]);

    // A. Origin: null -> 403 (even with valid Bearer or valid cookie + client header)
    let mut req = bearer_upload_request(&good_zip, API_KEY);
    req.headers_mut().insert(ORIGIN, "null".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = env.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // B. Sec-Fetch-Site: cross-site -> 403
    let mut req = bearer_upload_request(&good_zip, API_KEY);
    req.headers_mut()
        .insert("sec-fetch-site", "cross-site".parse().unwrap());
    let resp = env.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // C. Cookie CSRF missing X-ToolStow-Client -> 403 (both with same-origin Origin and without Origin)
    let mut req = admin_upload_request_base(&good_zip);
    req.headers_mut().insert(COOKIE, cookie.parse().unwrap());
    req.headers_mut()
        .insert(ORIGIN, "http://127.0.0.1:8080".parse().unwrap());
    let resp = env.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let mut req = admin_upload_request_base(&good_zip);
    req.headers_mut().insert(COOKIE, cookie.parse().unwrap());
    let resp = env.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Cookie CSRF on PUT /api/admin/settings missing X-ToolStow-Client -> 403
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/settings")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "siteTitle": "CSRF" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Cookie upload WITH same-origin + X-ToolStow-Client: 1 -> 201 Created
    let mut req = admin_upload_request_base(&good_zip);
    req.headers_mut().insert(COOKIE, cookie.parse().unwrap());
    req.headers_mut()
        .insert(ORIGIN, "http://127.0.0.1:8080".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = env.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // D. Zip Slip -> 400 Bad Request, existing tool unaffected
    let slip_zip = make_zip(&[
        ("manifest.json", br#"{"id":"sec-tool","name":"Evil"}"#),
        ("../escape.html", b"evil"),
        ("index.html", b"evil"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&slip_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // E. Invalid manifest runtime: script -> 422
    let bad_manifest_zip = make_zip(&[
        (
            "manifest.json",
            br#"{"id":"sec-tool","name":"Script","runtime":"script"}"#,
        ),
        ("index.html", b"nope"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&bad_manifest_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // F. Oversized upload -> 413 Payload Too Large (both handler check and DefaultBodyLimit)
    let slightly_over = vec![b'A'; 1_200_000];
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&slightly_over, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(read_body_json(resp).await["code"], "PAYLOAD_TOO_LARGE");

    let way_over = make_stored_zip(&[("big.bin", &vec![0u8; 2_500_000])]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&way_over, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(read_body_json(resp).await["code"], "PAYLOAD_TOO_LARGE");

    // G. CSP sandbox (NO allow-same-origin), nosniff, no-cache, .staging blocked, symlink blocked
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/sec-tool/app/main.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.starts_with("sandbox "));
    assert!(csp.contains("allow-scripts"));
    assert!(csp.contains("allow-downloads"));
    assert!(
        !csp.contains("allow-same-origin"),
        "CSP must never include allow-same-origin: {csp}"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(read_body_bytes(resp).await, b"<!doctype html><p>safe</p>");

    // .staging directory must always return 404
    fs::create_dir_all(env.tools_dir.join(".staging/secret")).unwrap();
    fs::write(env.tools_dir.join(".staging/secret/leak.txt"), "secret").unwrap();
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/.staging/secret/leak.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Symlink escape inside tool directory -> 403
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        let secret_file = outside.path().join("secret.txt");
        fs::write(&secret_file, "outside-secret").unwrap();
        symlink(&secret_file, env.tools_dir.join("sec-tool/escape.txt")).unwrap();

        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/t/sec-tool/escape.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn loads_and_serves_real_bundled_example_tools() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let docker_src = manifest_dir.join("examples/tools/docker-cheatsheet");
    let favicon_src = manifest_dir.join("examples/tools/dynamic-favicon");

    // Part A: Disk scan via ToolRegistry::scan_all
    let disk_env = build_test_env(None, None, 50, false, false);
    copy_dir_recursive(&docker_src, &disk_env.tools_dir.join("docker-cheatsheet"));
    copy_dir_recursive(&favicon_src, &disk_env.tools_dir.join("dynamic-favicon"));
    assert_eq!(disk_env.registry.scan_all().unwrap(), 2);

    let resp = disk_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let tools = read_body_json(resp).await;
    let arr = tools.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert!(
        arr.iter()
            .any(|t| t["id"] == "docker-cheatsheet" && t["themeAware"] == true)
    );
    assert!(
        arr.iter()
            .any(|t| t["id"] == "dynamic-favicon" && t["themeAware"] == true)
    );

    // Filter by tag & keyword
    let resp = disk_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools?tag=favicon")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let filtered = read_body_json(resp).await;
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["id"], "dynamic-favicon");

    for (id, expected_snippet) in [
        ("docker-cheatsheet", "Docker"),
        ("dynamic-favicon", "Favicon"),
    ] {
        let resp = disk_env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/t/{id}/index.html"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let csp = resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.starts_with("sandbox "));
        assert!(!csp.contains("allow-same-origin"));
        let html = String::from_utf8(read_body_bytes(resp).await).unwrap();
        assert!(html.contains(expected_snippet));
    }

    // Part B: Upload both real examples via Bearer API key (flat + wrapped folder)
    let upload_env = build_test_env(None, Some(API_KEY), 50, false, false);
    let docker_zip = make_zip_from_dir(&docker_src, None);
    let favicon_zip = make_zip_from_dir(&favicon_src, Some("dynamic-favicon-wrapper"));

    let resp = upload_env
        .app
        .clone()
        .oneshot(bearer_upload_request(&docker_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(read_body_json(resp).await["id"], "docker-cheatsheet");

    let resp = upload_env
        .app
        .clone()
        .oneshot(bearer_upload_request(&favicon_zip, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(read_body_json(resp).await["id"], "dynamic-favicon");

    // Both immediately listed and served
    let resp = upload_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn locked_ui_v1_assets_and_theme_ui_css_cascade_override() {
    let env = build_test_env(Some(ADMIN_KEY), None, 50, false, false);

    // 1. GET & HEAD /ui/v1/toolstow.css
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/css; charset=utf-8"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    let base_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(base_css.contains("--ts-canvas"));
    assert!(!base_css.contains("ToolStow Custom Theme Override"));

    let head_css = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head_css.status(), StatusCode::OK);
    assert_eq!(
        head_css.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/css; charset=utf-8"
    );
    assert_eq!(
        head_css.headers()[CACHE_CONTROL].to_str().unwrap(),
        "no-cache"
    );
    assert_eq!(
        head_css.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert!(read_body_bytes(head_css).await.is_empty());

    // 2. GET & HEAD /ui/v1/theme.js
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    let theme_js = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(theme_js.contains("toolstow:theme"));

    let head_js = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/ui/v1/theme.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head_js.status(), StatusCode::OK);
    assert_eq!(
        head_js.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/javascript; charset=utf-8"
    );
    assert!(read_body_bytes(head_js).await.is_empty());

    // 3. Unknown /ui paths return 404 JSON error
    for missing_uri in ["/ui/v1/missing.css", "/ui", "/ui/"] {
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(missing_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "expected 404 for {missing_uri}"
        );
        assert_eq!(read_body_json(resp).await["code"], "NOT_FOUND");
    }

    // 4. Active custom theme (data/theme/index.html) CANNOT shadow /ui/*
    fs::create_dir_all(env.theme_dir.join("ui/v1")).unwrap();
    fs::write(
        env.theme_dir.join("index.html"),
        "<!doctype html><h1>Custom Theme Active</h1>",
    )
    .unwrap();
    fs::write(
        env.theme_dir.join("ui/v1/toolstow.css"),
        "/* HIJACKED CSS */",
    )
    .unwrap();
    fs::write(env.theme_dir.join("ui/v1/theme.js"), "// HIJACKED JS").unwrap();
    fs::write(
        env.theme_dir.join("ui/v1/missing.css"),
        "/* SHOULD NEVER SERVE */",
    )
    .unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let css_with_custom_theme = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(css_with_custom_theme.contains("--ts-canvas"));
    assert!(!css_with_custom_theme.contains("HIJACKED CSS"));

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let js_with_custom_theme = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(js_with_custom_theme.contains("toolstow:theme"));
    assert!(!js_with_custom_theme.contains("HIJACKED JS"));

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/missing.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 5. data/theme/ui.css cascade override appends to base CSS and cleanly reverts on removal
    let override_css = ":root { --ts-accent: #7c3aed; }\n.custom-reskin { border-radius: 0; }\n";
    fs::write(env.theme_dir.join("ui.css"), override_css).unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let combined_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(combined_css.contains("--ts-canvas"));
    assert!(combined_css.contains("/* --- ToolStow Custom Theme Override (data/theme/ui.css) --- */"));
    assert!(combined_css.contains("--ts-accent: #7c3aed;"));
    assert!(combined_css.ends_with(override_css));

    // Admin uses the embedded defaults with the same active theme installed.
    let admin_css = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/ui.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_css.status(), StatusCode::OK);
    assert_eq!(admin_css.headers()[CONTENT_TYPE], "text/css; charset=utf-8");
    assert_eq!(read_body_bytes(admin_css).await, base_css.as_bytes());

    fs::remove_file(env.theme_dir.join("ui.css")).unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let reverted_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert_eq!(reverted_css, base_css);

    // 6. Symlink escape at data/theme/ui.css pointing outside theme_dir -> 403 Forbidden
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        let outside_secret = outside.path().join("secret.css");
        fs::write(&outside_secret, "body { content: 'secret'; }").unwrap();
        symlink(&outside_secret, env.theme_dir.join("ui.css")).unwrap();

        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ui/v1/toolstow.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(read_body_json(resp).await["code"], "FORBIDDEN");
    }
}

#[tokio::test]
async fn upload_overwrite_confirmation_returns_409_until_confirmed() {
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 50, false, false);

    let v1_zip = make_zip(&[
        (
            "manifest.json",
            br#"{"id":"conflict-tool","name":"Conflict Tool","version":"1.0.0","draft":false}"#,
        ),
        ("index.html", b"<h1>v1 content</h1>"),
    ]);
    let v2_zip = make_zip(&[
        (
            "manifest.json",
            br#"{"id":"conflict-tool","name":"Conflict Tool New","version":"2.0.0","draft":true}"#,
        ),
        ("index.html", b"<h1>v2 content</h1>"),
    ]);

    let make_upload = |uri: &str, bytes: &[u8]| {
        let body = build_multipart_body("file", "bundle.zip", bytes);
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(HOST, "127.0.0.1:8080")
            .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(body))
            .unwrap()
    };

    // 1. First install with overwrite=false -> 201 Created
    let resp = env
        .app
        .clone()
        .oneshot(make_upload("/api/admin/tools/upload?overwrite=false", &v1_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(read_body_json(resp).await["version"], "1.0.0");

    // 2. Duplicate ID with overwrite=false -> 409 Conflict with existing & incoming metadata
    let resp = env
        .app
        .clone()
        .oneshot(make_upload("/api/admin/tools/upload?overwrite=false", &v2_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let conflict = read_body_json(resp).await;
    assert_eq!(conflict["code"], "TOOL_EXISTS");
    assert_eq!(conflict["existing"]["id"], "conflict-tool");
    assert_eq!(conflict["existing"]["version"], "1.0.0");
    assert_eq!(conflict["existing"]["draft"], false);
    assert_eq!(conflict["incoming"]["id"], "conflict-tool");
    assert_eq!(conflict["incoming"]["name"], "Conflict Tool New");
    assert_eq!(conflict["incoming"]["version"], "2.0.0");
    assert_eq!(conflict["incoming"]["draft"], true);

    // Existing live tool remains completely untouched
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/conflict-tool/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>v1 content</h1>");

    // 3. Confirmed overwrite with overwrite=true -> 200 OK
    let resp = env
        .app
        .clone()
        .oneshot(make_upload("/api/admin/tools/upload?overwrite=true", &v2_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["version"], "2.0.0");
    assert_eq!(updated["draft"], true);
}

fn admin_theme_upload_request(uri: &str, zip_bytes: &[u8], cookie: &str) -> Request<Body> {
    let body = build_multipart_body("file", "theme.zip", zip_bytes);
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(HOST, "127.0.0.1:8080")
        .header(ORIGIN, "http://127.0.0.1:8080")
        .header("x-toolstow-client", "1")
        .header(COOKIE, cookie)
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("build theme upload request")
}

#[tokio::test]
async fn multi_theme_admin_auth_fail_closed_and_bearer_isolation() {
    let sample_theme_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"paper-warm","name":"Paper Warm","version":"1.0.0"}"#,
        ),
        ("ui.css", b":root { --ts-canvas: #faf7f2; }\n"),
    ]);

    // 1. Fail-closed 404 when TOOLSTOW_ADMIN_KEY is unset (even when TOOLSTOW_API_KEY IS set)
    let api_only_env = build_test_env(None, Some(API_KEY), 50, false, false);

    let probes: Vec<(Method, &str, Vec<u8>, Option<&str>)> = vec![
        (Method::GET, "/api/admin/themes", Vec::new(), None),
        (
            Method::GET,
            "/api/admin/themes/paper-warm/preview",
            Vec::new(),
            None,
        ),
        (
            Method::POST,
            "/api/admin/themes/upload",
            build_multipart_body("file", "theme.zip", &sample_theme_zip),
            Some("multipart"),
        ),
        (
            Method::PUT,
            "/api/admin/themes/active",
            json!({ "short": "default" }).to_string().into_bytes(),
            Some("json"),
        ),
        (
            Method::DELETE,
            "/api/admin/themes/paper-warm",
            Vec::new(),
            None,
        ),
    ];

    for (method, uri, body, kind) in &probes {
        let mut builder = Request::builder()
            .method(method.clone())
            .uri(*uri)
            .header(HOST, "127.0.0.1:8080")
            .header(AUTHORIZATION, format!("Bearer {API_KEY}"));
        if *kind == Some("multipart") {
            builder = builder.header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            );
        } else if *kind == Some("json") {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        let resp = api_only_env
            .app
            .clone()
            .oneshot(builder.body(Body::from(body.clone())).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "expected fail-closed 404 for {method} {uri} when admin key unset"
        );
    }

    // 2. When TOOLSTOW_ADMIN_KEY IS set, Bearer API key MUST be rejected with 401 on all theme endpoints
    let dual_env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 50, false, false);
    for (method, uri, body, kind) in &probes {
        let mut builder = Request::builder()
            .method(method.clone())
            .uri(*uri)
            .header(HOST, "127.0.0.1:8080")
            .header(AUTHORIZATION, format!("Bearer {API_KEY}"));
        if *kind == Some("multipart") {
            builder = builder.header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            );
        } else if *kind == Some("json") {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        let resp = dual_env
            .app
            .clone()
            .oneshot(builder.body(Body::from(body.clone())).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "AI Bearer key must be rejected with 401 on {method} {uri}"
        );
    }

    // 3. CSRF enforcement on mutating theme routes (missing X-ToolStow-Client -> 403, Origin: null -> 403)
    let cookie = login_as_admin(&dual_env.app, ADMIN_KEY).await;

    let resp = dual_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "default" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = dual_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/themes/paper-warm")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "null")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn multi_theme_upload_conflict_preview_skin_only_full_shell_and_delete_revert() {
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 50, false, false);
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;

    create_disk_tool(
        &env.tools_dir,
        "demo",
        r#"{"id":"demo","name":"Demo Tool"}"#,
        "<h1>Real Demo Tool</h1>",
    );
    assert_eq!(env.registry.scan_all().unwrap(), 1);

    // Baseline embedded /ui/v1/toolstow.css
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let base_ui_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(base_ui_css.contains("--ts-canvas"));

    // Initial GET /api/admin/themes -> [default (active: true)], Cache-Control: no-store
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/themes")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-store");
    let initial_themes = read_body_json(resp).await;
    let initial_arr = initial_themes.as_array().unwrap();
    assert_eq!(initial_arr.len(), 1);
    assert_eq!(initial_arr[0]["short"], "default");
    assert_eq!(initial_arr[0]["builtin"], true);
    assert_eq!(initial_arr[0]["active"], true);

    // 1. Upload full-shell theme v1 -> 201 Created
    let preview_svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 9"><rect width="16" height="9" fill="#18181b"/></svg>"##;
    let full_v1_zip = make_zip(&[
        (
            "theme.json",
            br#"{
                "short": "neon-grid",
                "name": "Neon Grid",
                "version": "1.0.0",
                "description": "Full shell v1",
                "author": "Tester",
                "preview": "preview.svg"
            }"#,
        ),
        ("dist/index.html", b"<!doctype html><h1>Neon Grid v1</h1>"),
        ("dist/theme.js", b"console.log('neon-v1');"),
        ("ui.css", b":root { --ts-accent: #111111; }\n"),
        ("preview.svg", preview_svg),
    ]);

    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload?overwrite=false",
            &full_v1_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = read_body_json(resp).await;
    assert_eq!(created["short"], "neon-grid");
    assert_eq!(created["version"], "1.0.0");
    assert_eq!(created["active"], false);
    assert_eq!(created["hasCustomShell"], true);
    assert_eq!(created["hasUiCss"], true);
    assert_eq!(created["previewUrl"], "/api/admin/themes/neon-grid/preview");
    assert!(env.themes_dir.join("neon-grid/theme.json").is_file());

    // 2. Duplicate upload with ?overwrite=false -> 409 Conflict THEME_EXISTS with existing & incoming
    let full_v2_zip = make_zip(&[
        (
            "theme.json",
            br#"{
                "short": "neon-grid",
                "name": "Neon Grid Pro",
                "version": "2.0.0",
                "description": "Full shell v2",
                "author": "Tester",
                "preview": "preview.svg"
            }"#,
        ),
        ("dist/index.html", b"<!doctype html><h1>Neon Grid v2</h1>"),
        ("dist/theme.js", b"console.log('neon-v2');"),
        // Attempt to shadow locked routes inside custom theme web_root
        ("dist/admin/index.html", b"<h1>FAKE THEME ADMIN</h1>"),
        ("dist/api/tools", b"FAKE THEME API"),
        ("dist/t/demo/index.html", b"<h1>FAKE THEME TOOL</h1>"),
        ("dist/ui/v1/toolstow.css", b"/* FAKE THEME UI CSS */"),
        ("ui.css", b":root { --ts-accent: #222222; }\n"),
        ("preview.svg", preview_svg),
    ]);

    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload?overwrite=false",
            &full_v2_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let conflict = read_body_json(resp).await;
    assert_eq!(conflict["code"], "THEME_EXISTS");
    assert_eq!(conflict["existing"]["short"], "neon-grid");
    assert_eq!(conflict["existing"]["version"], "1.0.0");
    assert_eq!(conflict["incoming"]["short"], "neon-grid");
    assert_eq!(conflict["incoming"]["name"], "Neon Grid Pro");
    assert_eq!(conflict["incoming"]["version"], "2.0.0");

    // 3. Confirmed overwrite with ?overwrite=true -> 200 OK
    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload?overwrite=true",
            &full_v2_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["version"], "2.0.0");
    assert_eq!(updated["name"], "Neon Grid Pro");
    assert_eq!(updated["active"], false);

    // 4. Preview endpoint -> 200 with CSP: sandbox;, nosniff, no-cache
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/themes/neon-grid/preview")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap(),
        "sandbox;"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert!(
        resp.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("image/svg+xml")
    );
    assert_eq!(read_body_bytes(resp).await, preview_svg);

    // 5. Upload skin-only theme (ui.css only, no index.html)
    let skin_css = ":root { --ts-canvas: #faf7f2; --ts-ink: #1c1917; }\n";
    let skin_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"paper-warm","name":"Paper Warm","version":"1.0.0"}"#,
        ),
        ("ui.css", skin_css.as_bytes()),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload?overwrite=false",
            &skin_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let skin_entry = read_body_json(resp).await;
    assert_eq!(skin_entry["short"], "paper-warm");
    assert_eq!(skin_entry["hasCustomShell"], false);
    assert_eq!(skin_entry["hasUiCss"], true);
    assert!(skin_entry["previewUrl"].is_null());

    // Place legacy data/theme/ files to verify named themes do NOT mix legacy data/theme/!
    fs::create_dir_all(&env.theme_dir).unwrap();
    fs::write(
        env.theme_dir.join("index.html"),
        "<!doctype html><h1>Legacy Dir Shell</h1>",
    )
    .unwrap();
    fs::write(env.theme_dir.join("ui.css"), "/* LEGACY_UI_CSS */\n").unwrap();

    // Activate skin-only theme -> 200 { activeTheme: "paper-warm" }
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "paper-warm" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await["activeTheme"], "paper-warm");

    // /api/me reports activeTheme: "paper-warm", themeSource: "custom"
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let me = read_body_json(resp).await;
    assert_eq!(me["activeTheme"], "paper-warm");
    assert_eq!(me["themeSource"], "custom");

    // GET / stays on embedded shell (NOT legacy data/theme/index.html)
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let root_html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(root_html.contains("/app.js"));
    assert!(!root_html.contains("Legacy Dir Shell"));

    // GET /ui/v1/toolstow.css appends paper-warm/ui.css (and NOT legacy data/theme/ui.css)
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let skin_combined_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(skin_combined_css.contains("--ts-canvas"));
    assert!(
        skin_combined_css.contains("/* --- ToolStow Active Theme Override (paper-warm/ui.css) --- */")
    );
    assert!(skin_combined_css.ends_with(skin_css));
    assert!(!skin_combined_css.contains("LEGACY_UI_CSS"));

    // Upload a shell-only theme (no ui.css) and verify legacy data/theme/ui.css is NOT mixed into it
    let shell_only_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"bare-shell","name":"Bare Shell","version":"1.0.0"}"#,
        ),
        ("index.html", b"<!doctype html><h1>Bare Shell</h1>"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload",
            &shell_only_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "bare-shell" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bare_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert_eq!(bare_css, base_ui_css);
    assert!(!bare_css.contains("LEGACY_UI_CSS"));

    // Clean up legacy data/theme/ so default revert restores embedded mode
    fs::remove_dir_all(&env.theme_dir).unwrap();

    // 6. Activate full-shell theme neon-grid -> switches /, fallback assets, and /ui/v1/toolstow.css
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "neon-grid" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await["activeTheme"], "neon-grid");

    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let me = read_body_json(resp).await;
    assert_eq!(me["activeTheme"], "neon-grid");
    assert_eq!(me["themeSource"], "custom");

    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        read_body_bytes(resp).await,
        b"<!doctype html><h1>Neon Grid v2</h1>"
    );

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/theme.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"console.log('neon-v2');");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let neon_css = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(neon_css.contains("--ts-canvas"));
    assert!(neon_css.contains("/* --- ToolStow Active Theme Override (neon-grid/ui.css) --- */"));
    assert!(neon_css.contains("--ts-accent: #222222;"));
    assert!(!neon_css.contains("FAKE THEME UI CSS"));

    // Locked routes stay locked even when full-shell theme tries to shadow them
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/admin").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let admin_html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(admin_html.contains("ToolStow Admin"));
    assert!(!admin_html.contains("FAKE THEME ADMIN"));

    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/tools").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await[0]["id"], "demo");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/demo/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_body_bytes(resp).await, b"<h1>Real Demo Tool</h1>");

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        String::from_utf8(read_body_bytes(resp).await)
            .unwrap()
            .contains("toolstow:theme")
    );

    // Activating nonexistent theme -> 404
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "does-not-exist" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Re-uploading the currently active theme sets active: true in the 200 response
    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload?overwrite=true",
            &full_v2_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await["active"], true);

    // 7. Deleting "default" -> 400 Bad Request; deleting missing -> 404 Not Found
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/themes/default")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/themes/missing-theme")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 8. Deleting the currently active theme -> 204 No Content & auto-reverts to "default"
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/themes/neon-grid")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // /api/me reverts to default + embedded
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let me = read_body_json(resp).await;
    assert_eq!(me["activeTheme"], "default");
    assert_eq!(me["themeSource"], "embedded");

    // settings.json persisted activeTheme: "default"
    let settings_raw = fs::read_to_string(env.data_dir.join("settings.json")).unwrap();
    let settings_json: Value = serde_json::from_str(&settings_raw).unwrap();
    assert_eq!(settings_json["activeTheme"], "default");

    // GET / restored to embedded shell and /ui/v1/toolstow.css restored to base CSS
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(
        String::from_utf8(read_body_bytes(resp).await)
            .unwrap()
            .contains("/app.js")
    );

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/toolstow.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        String::from_utf8(read_body_bytes(resp).await).unwrap(),
        base_ui_css
    );

    // GET /api/admin/themes marks default active again and no longer lists neon-grid
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/themes")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let final_themes = read_body_json(resp).await;
    let arr = final_themes.as_array().unwrap();
    assert_eq!(arr[0]["short"], "default");
    assert_eq!(arr[0]["active"], true);
    assert!(!arr.iter().any(|t| t["short"] == "neon-grid"));
}

#[tokio::test]
async fn theme_assets_endpoint_and_custom_theme_cors_and_security() {
    let env = build_test_env(Some(ADMIN_KEY), None, 50, false, false);
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;

    // 1. Built-in default with no legacy data/theme/ -> 404
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/bg.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Legacy data/theme/ works on default when present
    let legacy_png: &[u8] = b"\x89PNG\r\n\x1a\nLEGACY";
    fs::create_dir_all(env.theme_dir.join("assets")).unwrap();
    fs::write(env.theme_dir.join("assets/legacy.png"), legacy_png).unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/legacy.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "image/png"
    );
    assert_eq!(read_body_bytes(resp).await, legacy_png);

    // 2. Upload and activate a skin-only theme (no index.html)
    let skin_svg = br##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="4" height="4" fill="#111"/></svg>"##;
    let fake_woff2: &[u8] = b"wOF2FAKEFONTDATA";
    let fake_png: &[u8] = b"\x89PNG\r\n\x1a\nTILE";
    let asset_html: &[u8] = b"<!doctype html><script>alert(1)</script>";

    let skin_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"paper-assets","name":"Paper Assets","version":"1.0.0"}"#,
        ),
        (
            "ui.css",
            b":root { --ts-canvas: #faf7f2; }\nbody { background: url('/ui/v1/theme-assets/assets/bg.svg'); }\n",
        ),
        ("assets/bg.svg", skin_svg),
        ("fonts/custom.woff2", fake_woff2),
        ("assets/tile.png", fake_png),
        ("assets/page.html", asset_html),
    ]);

    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload",
            &skin_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let activate_theme = |short: &str| {
        Request::builder()
            .method(Method::PUT)
            .uri("/api/admin/themes/active")
            .header(HOST, "127.0.0.1:8080")
            .header(ORIGIN, "http://127.0.0.1:8080")
            .header("x-toolstow-client", "1")
            .header(COOKIE, &cookie)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "short": short }).to_string()))
            .unwrap()
    };

    let resp = env
        .app
        .clone()
        .oneshot(activate_theme("paper-assets"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // SVG on skin-only theme -> 200, ACAO: *, nosniff, no-cache, CSP: sandbox;
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/bg.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "image/svg+xml"
    );
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(
        resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap(),
        "sandbox;"
    );
    assert_eq!(read_body_bytes(resp).await, skin_svg);

    // HEAD request on SVG -> 200, same headers, empty body
    let head_resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/ui/v1/theme-assets/assets/bg.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head_resp.status(), StatusCode::OK);
    assert_eq!(
        head_resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        head_resp.headers()[CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap(),
        "sandbox;"
    );
    assert!(read_body_bytes(head_resp).await.is_empty());

    // .woff2 font on skin-only theme -> 200, font/woff2, ACAO: *, no CSP
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/fonts/custom.woff2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "font/woff2"
    );
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert!(resp.headers().get(CONTENT_SECURITY_POLICY).is_none());
    assert_eq!(read_body_bytes(resp).await, fake_woff2);

    // .png texture on skin-only theme -> 200, image/png, ACAO: *, no CSP
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/tile.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "image/png"
    );
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert!(resp.headers().get(CONTENT_SECURITY_POLICY).is_none());
    assert_eq!(read_body_bytes(resp).await, fake_png);

    // HTML asset under theme-assets -> 200 with CSP: sandbox;
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/page.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(
        resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap(),
        "sandbox;"
    );

    // theme.json and directory requests -> 404; legacy data/theme/ is not mixed in
    for uri in [
        "/ui/v1/theme-assets/theme.json",
        "/ui/v1/theme-assets/assets",
        "/ui/v1/theme-assets/assets/legacy.png",
    ] {
        let resp = env
            .app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "expected 404 for {uri}"
        );
    }

    // Dot-prefixed hidden paths and traversal -> 400 Bad Request
    fs::write(env.themes_dir.join("paper-assets/.hidden"), "secret").unwrap();
    fs::write(env.themes_dir.join("paper-assets/assets/.env"), "secret").unwrap();
    for bad_uri in [
        "/ui/v1/theme-assets/.hidden",
        "/ui/v1/theme-assets/assets/.env",
        "/ui/v1/theme-assets/.staging/foo",
        "/ui/v1/theme-assets/../secret",
        "/ui/v1/theme-assets/assets/../../secret",
        "/ui/v1/theme-assets/%2e%2e/secret",
        "/ui/v1/theme-assets/assets%5cbg.svg",
    ] {
        let resp = env
            .app
            .clone()
            .oneshot(Request::builder().uri(bad_uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 for {bad_uri}"
        );
    }

    // Symlink escape outside theme root -> 403 Forbidden
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        let outside_secret = outside.path().join("outside.txt");
        fs::write(&outside_secret, "top-secret").unwrap();
        symlink(
            &outside_secret,
            env.themes_dir.join("paper-assets/assets/escape.txt"),
        )
        .unwrap();

        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ui/v1/theme-assets/assets/escape.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Broken symlink pointing outside -> 403 Forbidden
        symlink(
            outside.path().join("missing.txt"),
            env.themes_dir.join("paper-assets/assets/dangling.txt"),
        )
        .unwrap();
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ui/v1/theme-assets/assets/dangling.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // 3. Upload and activate a full-shell theme
    let shell_svg = br##"<svg xmlns="http://www.w3.org/2000/svg"><circle r="4"/></svg>"##;
    let shell_woff2: &[u8] = b"wOF2SHELLFONT";
    let shell_css: &[u8] = b"body { color: #222; }\n";

    let full_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"full-assets","name":"Full Assets","version":"1.0.0"}"#,
        ),
        ("dist/index.html", b"<!doctype html><h1>Full Assets</h1>"),
        ("dist/style.css", shell_css),
        ("ui.css", b":root { --ts-accent: #333; }\n"),
        ("assets/bg.svg", shell_svg),
        ("fonts/shell.woff2", shell_woff2),
    ]);

    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload",
            &full_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = env
        .app
        .clone()
        .oneshot(activate_theme("full-assets"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // /ui/v1/theme-assets/* serves from full-shell theme root_dir
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/assets/bg.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap(),
        "sandbox;"
    );
    assert_eq!(read_body_bytes(resp).await, shell_svg);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/theme-assets/fonts/shell.woff2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "font/woff2"
    );
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(read_body_bytes(resp).await, shell_woff2);

    // serve_custom_theme_asset (/style.css on active full theme) includes Access-Control-Allow-Origin: *
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/style.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap(),
        "*"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(read_body_bytes(resp).await, shell_css);
}

#[tokio::test]
async fn healthz_returns_ok_and_cannot_be_shadowed_by_custom_theme() {
    let env = build_test_env(None, None, 50, false, false);
    fs::create_dir_all(&env.theme_dir).unwrap();
    fs::write(
        env.theme_dir.join("index.html"),
        "<!doctype html><h1>Custom</h1>",
    )
    .unwrap();
    fs::write(env.theme_dir.join("healthz"), "shadowed-file").unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-store");
    let body = read_body_json(resp).await;
    assert_eq!(
        body,
        json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
        })
    );
}

#[tokio::test]
async fn ui_host_js_is_locked_on_get_and_head_and_cannot_be_shadowed_by_custom_theme() {
    let env = build_test_env(Some(ADMIN_KEY), None, 50, false, false);

    // 1. GET /ui/v1/host.js -> 200, text/javascript; charset=utf-8, nosniff, no-cache
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/host.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    let embedded_bytes = read_body_bytes(resp).await;
    assert!(!embedded_bytes.is_empty());

    // 2. HEAD /ui/v1/host.js -> 200, same security/caching headers, empty body
    let head_resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .uri("/ui/v1/host.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head_resp.status(), StatusCode::OK);
    assert_eq!(
        head_resp.headers()[CONTENT_TYPE].to_str().unwrap(),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        head_resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(
        head_resp.headers()[CACHE_CONTROL].to_str().unwrap(),
        "no-cache"
    );
    assert!(read_body_bytes(head_resp).await.is_empty());

    // 3. Legacy data/theme/ui/v1/host.js cannot shadow /ui/v1/host.js
    fs::create_dir_all(env.theme_dir.join("ui/v1")).unwrap();
    fs::write(
        env.theme_dir.join("index.html"),
        "<!doctype html><h1>Legacy Custom</h1>",
    )
    .unwrap();
    fs::write(
        env.theme_dir.join("ui/v1/host.js"),
        "// shadowed by legacy theme",
    )
    .unwrap();

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/host.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_body_bytes(resp).await;
    assert_eq!(body, embedded_bytes);
    assert_ne!(body, b"// shadowed by legacy theme");

    // 4. Active uploaded full-shell theme cannot shadow /ui/v1/host.js either
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;
    let shadow_zip = make_zip(&[
        (
            "theme.json",
            br#"{"short":"shadow-host","name":"Shadow Host","version":"1.0.0"}"#,
        ),
        ("index.html", b"<!doctype html><h1>Shadow</h1>"),
        ("ui/v1/host.js", b"// shadowed by uploaded theme"),
    ]);

    let resp = env
        .app
        .clone()
        .oneshot(admin_theme_upload_request(
            "/api/admin/themes/upload",
            &shadow_zip,
            &cookie,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/admin/themes/active")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "short": "shadow-host" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ui/v1/host.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, embedded_bytes);
}

#[tokio::test]
async fn single_html_tool_ingestion_auth_csrf_injection_conflict_and_serving() {
    // 1. Fail-closed when neither admin_key nor api_key is configured -> 404
    let closed_env = build_test_env(None, None, 50, false, false);
    let resp = closed_env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "id": "blocked-tool",
                        "name": "Blocked",
                        "html": "<h1>No</h1>"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 2. Dual-key environment
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 1, false, false);
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;

    let sample_payload = json!({
        "id": "quick-calc",
        "name": "Quick Calc",
        "html": "<!doctype html><html><head><title>Calc</title></head><body><h1>Calc v1</h1></body></html>",
        "description": "Fast single-file calculator",
        "category": "Math",
        "tags": ["calc", "math"],
        "icon": "🧮",
        "draft": false,
        "injectSdk": true
    });

    // Unauthenticated -> 401
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Wrong Bearer token -> 401
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, "Bearer wrong-api-key")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Admin cookie missing X-ToolStow-Client -> 403
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Admin cookie with Origin: null -> 403
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "null")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Admin cookie with Sec-Fetch-Site: cross-site -> 403
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header("sec-fetch-site", "cross-site")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 3. Validation errors: invalid id, empty name, empty html -> 422; oversized -> 413
    for invalid_body in [
        json!({ "id": "Bad_ID", "name": "Ok", "html": "<h1>ok</h1>" }),
        json!({ "id": "../escape", "name": "Ok", "html": "<h1>ok</h1>" }),
        json!({ "id": "valid-id", "name": "   ", "html": "<h1>ok</h1>" }),
        json!({ "id": "valid-id", "name": "Ok", "html": "  \n\t " }),
    ] {
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/tools/import/html")
                    .header(HOST, "127.0.0.1:8080")
                    .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(invalid_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "expected 422 for payload {invalid_body}"
        );
    }

    // Oversized HTML (> 1 MiB limit configured above) -> 413
    let oversized_html = "a".repeat(1024 * 1024 + 128);
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "id": "too-big",
                        "name": "Too Big",
                        "html": oversized_html
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // 4. Bearer API key creates new HTML tool with SDK injection -> 201 Created
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(sample_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = read_body_json(resp).await;
    assert_eq!(created["id"], "quick-calc");
    assert_eq!(created["name"], "Quick Calc");
    assert_eq!(created["description"], "Fast single-file calculator");
    assert_eq!(created["category"], "Math");
    assert_eq!(created["tags"], json!(["calc", "math"]));
    assert_eq!(created["icon"], "🧮");
    assert_eq!(created["entry"], "index.html");
    assert_eq!(created["entryUrl"], "/t/quick-calc/index.html");
    assert_eq!(created["runtime"], "static");
    assert_eq!(created["themeAware"], true);
    assert_eq!(created["draft"], false);

    // Immediately listed in public GET /api/tools
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let public_tools = read_body_json(resp).await;
    let arr = public_tools.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], "quick-calc");
    assert_eq!(arr[0]["themeAware"], true);

    // Served at /t/quick-calc/index.html with CSP sandbox (never allow-same-origin) and injected SDK
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/quick-calc/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.starts_with("sandbox "));
    assert!(!csp.contains("allow-same-origin"));
    let served_html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(served_html.contains(r#"<link rel="stylesheet" href="/ui/v1/toolstow.css">"#));
    assert!(served_html.contains(r#"<script src="/ui/v1/theme.js"></script>"#));
    assert!(served_html.find("/ui/v1/theme.js").unwrap() < served_html.find("</head>").unwrap());

    // 5. Admin Cookie + CSRF with omitted overwrite (defaults to false) and explicit ?overwrite=false -> 409 TOOL_EXISTS
    let v2_payload = json!({
        "id": "quick-calc",
        "name": "Quick Calc v2",
        "html": "<!doctype html><html><head><title>Calc v2</title></head><body><h1>Calc v2</h1></body></html>",
        "description": "Updated calculator",
        "injectSdk": false
    });
    for conflict_uri in [
        "/api/admin/tools/import/html",
        "/api/admin/tools/import/html?overwrite=false",
    ] {
        let resp = env
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(conflict_uri)
                    .header(HOST, "127.0.0.1:8080")
                    .header(ORIGIN, "http://127.0.0.1:8080")
                    .header("x-toolstow-client", "1")
                    .header(COOKIE, &cookie)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(v2_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let conflict = read_body_json(resp).await;
        assert_eq!(conflict["code"], "TOOL_EXISTS");
        assert_eq!(conflict["existing"]["id"], "quick-calc");
        assert_eq!(conflict["existing"]["name"], "Quick Calc");
        assert_eq!(conflict["incoming"]["id"], "quick-calc");
        assert_eq!(conflict["incoming"]["name"], "Quick Calc v2");
    }

    // 6. Admin Cookie + CSRF with ?overwrite=true -> 200 OK and updates disk + registry
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html?overwrite=true")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(v2_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["name"], "Quick Calc v2");
    assert_eq!(updated["themeAware"], false);

    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/quick-calc/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let served_v2 = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(served_v2.contains("<h1>Calc v2</h1>"));
    assert!(!served_v2.contains("/ui/v1/theme.js"));

    // 7. Draft single-HTML tool via Admin Cookie -> hidden from /api/tools & anonymous /t/, visible to admin
    let draft_payload = json!({
        "id": "draft-note",
        "name": "Draft Note",
        "html": "<!doctype html><head><script src=\"/ui/v1/theme.js\"></script></head><body>Secret Note</body>",
        "draft": true,
        "injectSdk": false
    });
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(draft_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let draft_tool = read_body_json(resp).await;
    assert_eq!(draft_tool["draft"], true);
    assert_eq!(draft_tool["themeAware"], true);

    // Public /api/tools still only has quick-calc
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let public_arr = read_body_json(resp).await;
    assert_eq!(public_arr.as_array().unwrap().len(), 1);
    assert_eq!(public_arr[0]["id"], "quick-calc");

    // Anonymous /t/draft-note/index.html -> 404
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/draft-note/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Admin cookie /t/draft-note/index.html -> 200 with CSP sandbox (never allow-same-origin)
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/draft-note/index.html")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.starts_with("sandbox "));
    assert!(!csp.contains("allow-same-origin"));
    assert!(
        String::from_utf8(read_body_bytes(resp).await)
            .unwrap()
            .contains("Secret Note")
    );
}

#[tokio::test]
async fn single_html_import_safe_create_default_preserves_existing_multifile_zip_tool() {
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 5, false, false);

    // 1. Install a multi-file .zip tool with extra assets
    let zip_bytes = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "multi-zip-tool",
                "name": "Original Zip Tool",
                "description": "Multi-file zip package"
            }"#,
        ),
        ("index.html", b"<!doctype html><html><body>Original Zip HTML</body></html>"),
        ("assets/extra.txt", b"precious-asset-payload"),
    ]);
    let resp = env
        .app
        .clone()
        .oneshot(bearer_upload_request(&zip_bytes, API_KEY))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let extra_asset_path = env.tools_dir.join("multi-zip-tool/assets/extra.txt");
    let index_html_path = env.tools_dir.join("multi-zip-tool/index.html");
    assert_eq!(
        fs::read_to_string(&extra_asset_path).unwrap(),
        "precious-asset-payload"
    );
    assert!(
        fs::read_to_string(&index_html_path)
            .unwrap()
            .contains("Original Zip HTML")
    );

    // 2. Importing single HTML with the same ID and omitted `overwrite` -> 409 TOOL_EXISTS
    let replacement_payload = json!({
        "id": "multi-zip-tool",
        "name": "Single HTML Replacement",
        "html": "<!doctype html><html><body>Replaced Single HTML</body></html>",
        "injectSdk": false
    });
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(replacement_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let conflict = read_body_json(resp).await;
    assert_eq!(conflict["code"], "TOOL_EXISTS");
    assert_eq!(conflict["existing"]["id"], "multi-zip-tool");
    assert_eq!(conflict["existing"]["name"], "Original Zip Tool");
    assert_eq!(conflict["incoming"]["id"], "multi-zip-tool");
    assert_eq!(conflict["incoming"]["name"], "Single HTML Replacement");

    // Existing multi-file tool directory and extra assets must remain completely untouched
    assert_eq!(
        fs::read_to_string(&extra_asset_path).unwrap(),
        "precious-asset-payload"
    );
    assert!(
        fs::read_to_string(&index_html_path)
            .unwrap()
            .contains("Original Zip HTML")
    );
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/multi-zip-tool/assets/extra.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"precious-asset-payload");

    // 3. Explicit `?overwrite=true` -> 200 OK and replaces the existing tool
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html?overwrite=true")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(replacement_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["id"], "multi-zip-tool");
    assert_eq!(updated["name"], "Single HTML Replacement");

    assert!(!extra_asset_path.exists());
    assert!(
        fs::read_to_string(&index_html_path)
            .unwrap()
            .contains("Replaced Single HTML")
    );
}

#[tokio::test]
async fn single_html_import_tool_id_html_has_no_route_collision() {
    let env = build_test_env(Some(ADMIN_KEY), Some(API_KEY), 5, false, false);
    let cookie = login_as_admin(&env.app, ADMIN_KEY).await;

    // 1. Create tool with id = "html" via POST /api/admin/tools/import/html -> 201 Created
    let payload = json!({
        "id": "html",
        "name": "HTML Inspector",
        "html": "<!doctype html><html><head><title>HTML Tool</title></head><body><h1>HTML ID Works</h1></body></html>",
        "draft": false,
        "injectSdk": true
    });
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/admin/tools/import/html")
                .header(HOST, "127.0.0.1:8080")
                .header(AUTHORIZATION, format!("Bearer {API_KEY}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = read_body_json(resp).await;
    assert_eq!(created["id"], "html");
    assert_eq!(created["entryUrl"], "/t/html/index.html");

    // 2. Serve tool at /t/html/index.html -> 200 OK
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/html/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        String::from_utf8(read_body_bytes(resp).await)
            .unwrap()
            .contains("<h1>HTML ID Works</h1>")
    );

    // 3. Toggle draft via PATCH /api/admin/tools/html -> 200 OK (no 405 collision)
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/admin/tools/html")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "draft": true }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let patched = read_body_json(resp).await;
    assert_eq!(patched["id"], "html");
    assert_eq!(patched["draft"], true);

    // 4. Delete via DELETE /api/admin/tools/html -> 204 No Content (no 405 collision)
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/admin/tools/html")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .header("x-toolstow-client", "1")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Tool is removed and no longer served
    let resp = env
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/html/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
