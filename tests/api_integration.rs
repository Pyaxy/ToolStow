//! End-to-end HTTP integration tests for ToolStow routes, CSRF protection,
//! ZIP upload/overwrite/delete flows, static sandbox hosting, and bundled examples.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, LOCATION, ORIGIN,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use toolstow::config::Config;
use toolstow::registry::ToolRegistry;
use toolstow::routes::{AppState, build_router};

const BOUNDARY: &str = "----ToolStowTestBoundary7MA4YWxkTrZu0gW";

fn test_app(tools_dir: PathBuf, max_upload_mb: usize) -> (Router, Arc<ToolRegistry>) {
    let registry = Arc::new(ToolRegistry::new(tools_dir.clone()));
    let config = Arc::new(Config {
        tools_dir,
        max_upload_mb,
        ..Config::default()
    });
    let app = build_router(AppState {
        registry: Arc::clone(&registry),
        config,
    });
    (app, registry)
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

fn upload_request(zip_bytes: &[u8]) -> Request<Body> {
    let body = build_multipart_body("file", "tool.zip", zip_bytes);
    Request::builder()
        .method(Method::POST)
        .uri("/api/tools/upload")
        .header(HOST, "127.0.0.1:8080")
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("build upload request")
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
async fn serves_embedded_frontend_assets_without_shadowing_api_or_tools() {
    let temp = TempDir::new().unwrap();
    let (app, _registry) = test_app(temp.path().to_path_buf(), 50);

    // GET /
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers()[CONTENT_TYPE].to_str().unwrap();
    assert!(ct.contains("text/html"), "unexpected content-type: {ct}");
    let html = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(html.contains("ToolStow"));

    // GET /app.js
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp.headers()[CONTENT_TYPE].to_str().unwrap();
    assert!(
        ct.contains("javascript"),
        "expected javascript content-type, got {ct}"
    );
    let js = String::from_utf8(read_body_bytes(resp).await).unwrap();
    assert!(js.contains("X-ToolStow-Client"));

    // GET /style.css
    let resp = app
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
    let ct = resp.headers()[CONTENT_TYPE].to_str().unwrap();
    assert!(ct.contains("text/css"), "unexpected content-type: {ct}");

    // Unknown API path returns 404 JSON (not frontend HTML)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let json = read_body_json(resp).await;
    assert_eq!(json["code"], "NOT_FOUND");
}

#[tokio::test]
async fn upload_create_overwrite_bad_package_isolation_and_delete() {
    let temp = TempDir::new().unwrap();
    let (app, _registry) = test_app(temp.path().to_path_buf(), 50);

    // 1. Initial upload -> 201 Created
    let v1_zip = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "demo-tool",
                "name": "Demo Tool",
                "version": "1.0.0",
                "description": "First version",
                "category": "Dev",
                "tags": ["alpha", "utility"],
                "sandbox": ["allow-scripts", "allow-same-origin", "allow-forms"]
            }"#,
        ),
        ("index.html", b"<h1>Demo v1</h1>"),
    ]);

    let resp = app.clone().oneshot(upload_request(&v1_zip)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = read_body_json(resp).await;
    assert_eq!(created["id"], "demo-tool");
    assert_eq!(created["version"], "1.0.0");
    assert_eq!(created["entryUrl"], "/t/demo-tool/index.html");
    let sandbox = created["sandbox"].as_array().unwrap();
    assert!(
        !sandbox.iter().any(|v| v == "allow-same-origin"),
        "allow-same-origin must be stripped: {sandbox:?}"
    );

    // 2. Verify immediately listed in GET /api/tools with filters
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools?q=First&category=dev&tag=alpha")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = read_body_json(resp).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], "demo-tool");

    // Empty / whitespace query parameters are treated as None
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools?q=%20%20&category=&tag=%20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 1);

    // 3. Same-ID overwrite -> 200 OK
    let v2_zip = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "demo-tool",
                "name": "Demo Tool",
                "version": "2.0.0",
                "description": "Updated version",
                "category": "Dev",
                "tags": ["beta"]
            }"#,
        ),
        ("index.html", b"<h1>Demo v2</h1>"),
    ]);

    let resp = app.clone().oneshot(upload_request(&v2_zip)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated = read_body_json(resp).await;
    assert_eq!(updated["version"], "2.0.0");

    // 4. Zip Slip attempt targeting same ID -> 400 Bad Request, existing tool remains intact
    let slip_zip = make_zip(&[
        (
            "manifest.json",
            br#"{"id": "demo-tool", "name": "Evil"}"#,
        ),
        ("../escape.html", b"evil"),
        ("index.html", b"evil"),
    ]);
    let resp = app.clone().oneshot(upload_request(&slip_zip)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 5. Invalid manifest (runtime: script) targeting same ID -> 422, existing v2 stays intact
    let invalid_manifest_zip = make_zip(&[
        (
            "manifest.json",
            br#"{"id": "demo-tool", "name": "Script Tool", "runtime": "script"}"#,
        ),
        ("index.html", b"nope"),
    ]);
    let resp = app
        .clone()
        .oneshot(upload_request(&invalid_manifest_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Confirm existing v2 content and metadata were NOT broken
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/demo-tool/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(read_body_bytes(resp).await, b"<h1>Demo v2</h1>");

    // 6. Missing `file` field -> 400 Bad Request
    let wrong_field_body = build_multipart_body("other", "tool.zip", &v2_zip);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/tools/upload")
        .header(HOST, "127.0.0.1:8080")
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(wrong_field_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 7. DELETE /api/tools/demo-tool -> 204 No Content
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/tools/demo-tool")
                .header(HOST, "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Immediately removed from /api/tools and /t/demo-tool/index.html
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read_body_json(resp).await.as_array().unwrap().len(), 0);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/demo-tool/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Deleting again returns 404 Not Found
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/tools/demo-tool")
                .header(HOST, "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_rejects_oversized_payload_with_413() {
    let temp = TempDir::new().unwrap();
    // Limit to 1 MiB for fast testing
    let (app, _registry) = test_app(temp.path().to_path_buf(), 1);

    // Case A: Just over max_upload_bytes (1.2 MiB) -> rejected inside handler with 413
    let slightly_over = vec![b'A'; 1_200_000];
    let resp = app
        .clone()
        .oneshot(upload_request(&slightly_over))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json = read_body_json(resp).await;
    assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");

    // Case B: Over DefaultBodyLimit (1 MiB + 1 MiB = 2 MiB; send 2.5 MiB) -> rejected with 413 JSON
    let way_over = make_stored_zip(&[("big.bin", &vec![0u8; 2_500_000])]);
    let resp = app.clone().oneshot(upload_request(&way_over)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json = read_body_json(resp).await;
    assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");
}

#[tokio::test]
async fn csrf_guard_protects_upload_and_delete_while_allowing_same_origin_and_cli() {
    let temp = TempDir::new().unwrap();
    let (app, _registry) = test_app(temp.path().to_path_buf(), 50);

    let tool_zip = make_zip(&[
        ("manifest.json", br#"{"id": "csrf-tool", "name": "CSRF Tool"}"#),
        ("index.html", b"<p>ok</p>"),
    ]);

    // 1. Origin: null (sandboxed iframe) -> 403
    let mut req = upload_request(&tool_zip);
    req.headers_mut().insert(ORIGIN, "null".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 2. Sec-Fetch-Site: cross-site -> 403
    let mut req = upload_request(&tool_zip);
    req.headers_mut()
        .insert("sec-fetch-site", "cross-site".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 3. Cross-origin Host mismatch -> 403
    let mut req = upload_request(&tool_zip);
    req.headers_mut()
        .insert(ORIGIN, "http://evil.local:8080".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 4. Same-origin WITHOUT X-ToolStow-Client -> 403
    let mut req = upload_request(&tool_zip);
    req.headers_mut()
        .insert(ORIGIN, "http://127.0.0.1:8080".parse().unwrap());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 5. Same-origin WITH X-ToolStow-Client: 1 -> 201 Created
    let mut req = upload_request(&tool_zip);
    req.headers_mut()
        .insert(ORIGIN, "http://127.0.0.1:8080".parse().unwrap());
    req.headers_mut()
        .insert("x-toolstow-client", "1".parse().unwrap());
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 6. DELETE with Origin: null -> 403
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/tools/csrf-tool")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "null")
                .header("x-toolstow-client", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 7. DELETE with same-origin WITHOUT X-ToolStow-Client -> 403
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/tools/csrf-tool")
                .header(HOST, "127.0.0.1:8080")
                .header(ORIGIN, "http://127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 8. CLI upload (no Origin, no X-ToolStow-Client) -> 200 OK overwrite
    let resp = app.clone().oneshot(upload_request(&tool_zip)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. CLI delete (no Origin, no X-ToolStow-Client) -> 204 No Content
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/tools/csrf-tool")
                .header(HOST, "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn static_tool_hosting_redirects_sets_sandbox_csp_and_blocks_staging_and_symlinks() {
    let temp = TempDir::new().unwrap();
    let (app, _registry) = test_app(temp.path().to_path_buf(), 50);

    let zip_bytes = make_zip(&[
        (
            "manifest.json",
            br#"{
                "id": "sandbox-demo",
                "name": "Sandbox Demo",
                "entry": "app/main.html",
                "sandbox": ["allow-scripts", "allow-same-origin", "allow-downloads"]
            }"#,
        ),
        ("app/main.html", b"<!doctype html><p>sandboxed</p>"),
    ]);

    let resp = app
        .clone()
        .oneshot(upload_request(&zip_bytes))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // GET /t/sandbox-demo -> 307 redirect to /t/sandbox-demo/app/main.html
    for uri in ["/t/sandbox-demo", "/t/sandbox-demo/"] {
        let resp = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            resp.headers()[LOCATION].to_str().unwrap(),
            "/t/sandbox-demo/app/main.html"
        );
    }

    // GET /t/sandbox-demo/app/main.html -> 200 with CSP sandbox (NO allow-same-origin), nosniff, no-cache
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/t/sandbox-demo/app/main.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers()[CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.starts_with("sandbox "), "unexpected CSP: {csp}");
    assert!(csp.contains("allow-scripts"), "unexpected CSP: {csp}");
    assert!(csp.contains("allow-downloads"), "unexpected CSP: {csp}");
    assert!(
        !csp.contains("allow-same-origin"),
        "CSP must NEVER include allow-same-origin: {csp}"
    );
    assert_eq!(
        resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
    assert_eq!(
        read_body_bytes(resp).await,
        b"<!doctype html><p>sandboxed</p>"
    );

    // .staging directory and unknown IDs must always return 404
    fs::create_dir_all(temp.path().join(".staging/secret")).unwrap();
    fs::write(temp.path().join(".staging/secret/leak.txt"), "top-secret").unwrap();

    for uri in [
        "/t/.staging/secret/leak.txt",
        "/t/.staging",
        "/t/unknown-tool/index.html",
        "/t/unknown-tool",
    ] {
        let resp = app
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

    // Symlink escape inside tool directory must be blocked
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = TempDir::new().unwrap();
        let secret_file = outside.path().join("secret.txt");
        fs::write(&secret_file, "outside-secret").unwrap();

        let link_path = temp.path().join("sandbox-demo").join("escape.txt");
        symlink(&secret_file, &link_path).unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/t/sandbox-demo/escape.txt")
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
    let disk_temp = TempDir::new().unwrap();
    copy_dir_recursive(&docker_src, &disk_temp.path().join("docker-cheatsheet"));
    copy_dir_recursive(&favicon_src, &disk_temp.path().join("dynamic-favicon"));

    let (disk_app, disk_registry) = test_app(disk_temp.path().to_path_buf(), 50);
    let count = disk_registry.scan_all().expect("scan bundled examples");
    assert_eq!(count, 2);

    let resp = disk_app
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
    assert!(arr.iter().any(|t| t["id"] == "docker-cheatsheet" && t["themeAware"] == true));
    assert!(arr.iter().any(|t| t["id"] == "dynamic-favicon" && t["themeAware"] == true));

    // Filter by tag & keyword
    let resp = disk_app
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

    let resp = disk_app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tools?q=docker")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let filtered = read_body_json(resp).await;
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["id"], "docker-cheatsheet");

    // Serve both tools' HTML with strict sandbox CSP
    for (id, expected_snippet) in [
        ("docker-cheatsheet", "Docker"),
        ("dynamic-favicon", "Favicon"),
    ] {
        let resp = disk_app
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
        assert_eq!(
            resp.headers()[X_CONTENT_TYPE_OPTIONS].to_str().unwrap(),
            "nosniff"
        );
        assert_eq!(resp.headers()[CACHE_CONTROL].to_str().unwrap(), "no-cache");
        let html = String::from_utf8(read_body_bytes(resp).await).unwrap();
        assert!(html.contains(expected_snippet));
    }

    // Part B: Upload both real example tools via POST /api/tools/upload (one flat, one wrapped in folder)
    let upload_temp = TempDir::new().unwrap();
    let (upload_app, _upload_registry) = test_app(upload_temp.path().to_path_buf(), 50);

    let docker_zip = make_zip_from_dir(&docker_src, None);
    let favicon_zip = make_zip_from_dir(&favicon_src, Some("dynamic-favicon-wrapper"));

    let resp = upload_app
        .clone()
        .oneshot(upload_request(&docker_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(read_body_json(resp).await["id"], "docker-cheatsheet");

    let resp = upload_app
        .clone()
        .oneshot(upload_request(&favicon_zip))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(read_body_json(resp).await["id"], "dynamic-favicon");

    let resp = upload_app
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
