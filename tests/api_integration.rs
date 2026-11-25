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
    AUTHORIZATION, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST, LOCATION,
    ORIGIN, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
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
    fs::create_dir_all(&tools_dir).expect("create tools dir");

    let config = Config {
        data_dir: data_dir.clone(),
        tools_dir: tools_dir.clone(),
        theme_dir: theme_dir.clone(),
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
    assert_eq!(me["defaultAccent"], "#18181b");
    assert_eq!(me["isAdmin"], false);
    assert_eq!(me["adminEnabled"], false);
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
    for uri in ["/admin", "/admin/", "/admin/admin.js", "/admin/admin.css"] {
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
    assert!(js.contains("/api/me"));

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
                        "siteDescription": "Monochrome self-hosted utilities",
                        "defaultAccent": "#2563eb"
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
    assert_eq!(updated["defaultAccent"], "#2563eb");

    // Verify persisted settings.json on disk
    let persisted_raw = fs::read_to_string(env.data_dir.join("settings.json")).unwrap();
    let persisted: Value = serde_json::from_str(&persisted_raw).unwrap();
    assert_eq!(persisted["siteTitle"], "My Personal Lab");
    assert_eq!(persisted["defaultAccent"], "#2563eb");

    // Reflected immediately on /api/me
    let resp = env
        .app
        .clone()
        .oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let me = read_body_json(resp).await;
    assert_eq!(me["siteTitle"], "My Personal Lab");
    assert_eq!(me["defaultAccent"], "#2563eb");

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
    let docker_src = manifest_dir.join("examples/docker-cheatsheet");
    let favicon_src = manifest_dir.join("examples/dynamic-favicon");

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
    let env = build_test_env(None, None, 50, false, false);

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
