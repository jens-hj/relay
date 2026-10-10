use super::*;
use futures_util::StreamExt;
use reqwest::{Client, Response as HttpResponse};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const ORIGIN: &str = "https://relay.example.test";
const TOKEN: &str = "native-secret-01234567890123456789";
const CODE: &str = "enrollment-code-012345678901234567890123456789";
struct Server {
    dir: tempfile::TempDir,
    url: String,
    client: Client,
    task: tokio::task::JoinHandle<()>,
    shutdown: Shutdown,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn restart(&mut self, token: &str, config: BrowserConfig) {
        self.task.abort();
        self.shutdown.shutdown().await.unwrap();
        let (app, shutdown) = router_with_browser(
            self.dir.path().join("db"),
            token.into(),
            DirectorProfile::default(),
            RuntimeConfig::default(),
            config,
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        self.url = format!("http://{}", listener.local_addr().unwrap());
        self.task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        self.shutdown = shutdown;
    }
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("setup"), CODE).unwrap();
        let web = dir.path().join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(
            web.join("index.html"),
            "<html><script src=./app.js></script></html>",
        )
        .unwrap();
        std::fs::write(web.join("app.js"), "/* public code */").unwrap();
        std::fs::write(web.join("app.wasm"), b"\0asm").unwrap();
        std::fs::write(
            web.join("app.wasm.gz"),
            [
                31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 99, 72, 44, 206, 5, 0, 219, 73, 64, 95, 4, 0, 0,
                0,
            ],
        )
        .unwrap();
        let (app, shutdown) = router_with_browser(
            dir.path().join("db"),
            TOKEN.into(),
            DirectorProfile::default(),
            RuntimeConfig::default(),
            BrowserConfig {
                public_origin: Some(ORIGIN.into()),
                setup_token_file: Some(dir.path().join("setup")),
                web_root: Some(web),
            },
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            dir,
            url,
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            task,
            shutdown,
        }
    }
    fn db(&self) -> Connection {
        Connection::open(self.dir.path().join("db")).unwrap()
    }
    async fn form_token(&self, path: &str) -> (String, String) {
        let response = self
            .client
            .get(format!("{}{path}", self.url))
            .send()
            .await
            .unwrap();
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let html = response.text().await.unwrap();
        let csrf = html
            .split("name=csrf value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned();
        (cookie, csrf)
    }
    async fn form(&self, path: &str, username: &str, password: &str, code: &str) -> HttpResponse {
        let (cookie, csrf) = self.form_token(path).await;
        self.client
            .post(format!("{}{path}", self.url))
            .header("origin", ORIGIN)
            .header("cookie", cookie)
            .form(&[
                ("username", username),
                ("password", password),
                ("code", code),
                ("csrf", csrf.as_str()),
            ])
            .send()
            .await
            .unwrap()
    }
    async fn enroll(&self) -> (String, Vec<String>) {
        let response = self
            .form("/setup", "owner", "correct password 123", CODE)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(
            response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("Secure; HttpOnly; SameSite=Lax; Path=/")
        );
        let html = response.text().await.unwrap();
        let codes = html
            .split("<pre>")
            .nth(1)
            .unwrap()
            .split("</pre>")
            .next()
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        (cookie, codes)
    }
    async fn session(&self, cookie: &str) -> HttpResponse {
        self.client
            .get(format!("{}/auth/session", self.url))
            .header("cookie", cookie)
            .send()
            .await
            .unwrap()
    }
    async fn websocket(
        &self,
        path: &str,
        cookie: &str,
        origin: &str,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::Error,
    > {
        let mut request = format!("{}{path}", self.url.replacen("http:", "ws:", 1))
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("cookie", cookie.parse().unwrap());
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        connect_async(request).await.map(|r| r.0)
    }
}
#[tokio::test]
async fn actual_http_enrollment_login_csrf_native_and_static() {
    let s = Server::start().await;
    assert_eq!(
        s.client
            .get(format!("{}/v1/snapshot", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for path in ["/", "/app/", "/app/index.html", "/app/%69ndex.html"] {
        let r = s
            .client
            .get(format!("{}{path}", s.url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 303);
        assert_eq!(r.headers()["location"], "/login");
    }
    for path in ["/app/app.wasm", "/app/app.wasm.gz", "/app/app.js"] {
        assert_eq!(
            s.client
                .get(format!("{}{path}", s.url))
                .header("accept-encoding", "gzip")
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    assert_eq!(
        s.client
            .get(format!("{}/app/../setup", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let (cookie, codes) = s.enroll().await;
    let wasm = s
        .client
        .get(format!("{}/app/app.wasm", s.url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(wasm.status(), 200);
    assert_eq!(wasm.headers()["content-type"], "application/wasm");
    let policy = wasm.headers()["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("script-src 'self' 'wasm-unsafe-eval';"));
    assert!(policy.contains("style-src 'self' 'unsafe-inline';"));
    let compressed = s
        .client
        .get(format!("{}/app/app.wasm", s.url))
        .header("cookie", &cookie)
        .header("accept-encoding", "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(compressed.status(), 200);
    assert_eq!(compressed.headers()["content-type"], "application/wasm");
    assert_eq!(compressed.headers()["content-encoding"], "gzip");
    let bytes = compressed.bytes().await.unwrap();
    assert_eq!(&bytes[..3], &[31, 139, 8]);
    assert_eq!(bytes.len(), 24);
    let session: serde_json::Value = s.session(&cookie).await.json().await.unwrap();
    assert_eq!(session["username"], "owner");
    assert_eq!(session["authenticated"], true);
    let csrf = session["csrf_token"].as_str().unwrap();
    for (origin, csrf_header, protocol, expected) in [
        (None, Some(csrf), Some("2"), 403),
        (Some(ORIGIN), None, Some("2"), 403),
        (Some(ORIGIN), Some(csrf), None, 409),
    ] {
        let mut request = s
            .client
            .post(format!("{}/v1/commands", s.url))
            .header("cookie", &cookie)
            .json(&serde_json::json!({}));
        if let Some(value) = origin {
            request = request.header("origin", value);
        }
        if let Some(value) = csrf_header {
            request = request.header("x-relay-csrf", value);
        }
        if let Some(value) = protocol {
            request = request.header("x-relay-protocol", value);
        }
        assert_eq!(request.send().await.unwrap().status(), expected);
    }
    assert_eq!(
        s.client
            .get(format!("{}/app/", s.url))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        s.client
            .get(format!("{}/", s.url))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .headers()["location"],
        "/app/"
    );
    assert_eq!(
        s.client
            .get(format!("{}/v1/snapshot", s.url))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        s.client
            .get(format!("{}/v1/snapshot", s.url))
            .header("cookie", &cookie)
            .header("authorization", "Bearer invalid")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        s.client
            .get(format!("{}/v1/snapshot", s.url))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    for (origin, csrf_header, expected) in [
        ("https://hostile.test", csrf, 403),
        (ORIGIN, "bad", 403),
        (ORIGIN, csrf, 422),
    ] {
        let r = s
            .client
            .post(format!("{}/v1/commands", s.url))
            .header("cookie", &cookie)
            .header("origin", origin)
            .header("x-relay-csrf", csrf_header)
            .header("x-relay-protocol", "2")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), expected);
    }
    let initial: Snapshot = s
        .client
        .get(format!("{}/v1/snapshot", s.url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let envelope = CommandEnvelope {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: initial.revision,
        command: Command::AddComment {
            message_id: "m2".into(),
            quote: None,
            author: "Owner".into(),
            body: "Saved from the browser".into(),
        },
    };
    let response = s
        .client
        .post(format!("{}/v1/commands", s.url))
        .header("cookie", &cookie)
        .header("origin", ORIGIN)
        .header("x-relay-csrf", csrf)
        .header("x-relay-protocol", "2")
        .json(&envelope)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Snapshot>().await.unwrap().revision,
        initial.revision + 1
    );
    let asset_id = uuid::Uuid::new_v4().to_string();
    let response = s
        .client
        .post(format!("{}/v1/assets/{asset_id}", s.url))
        .header("cookie", &cookie)
        .header("origin", ORIGIN)
        .header("x-relay-csrf", csrf)
        .header("x-relay-protocol", "2")
        .header("content-type", "text/html")
        .body("<script>alert(1)</script>")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        s.client
            .get(format!("{}/v1/assets/{asset_id}", s.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let asset = s
        .client
        .get(format!("{}/v1/assets/{asset_id}", s.url))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(asset.status(), 200);
    assert_eq!(asset.headers()["content-type"], "application/octet-stream");
    assert_eq!(asset.headers()["content-disposition"], "attachment");
    assert_eq!(asset.headers()["cache-control"], "no-store");
    assert_eq!(asset.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        s.form("/setup", "other", "correct password 123", CODE)
            .await
            .status(),
        401
    );
    assert_eq!(
        s.form("/login", "unknown", "wrong password", "")
            .await
            .status(),
        401
    );
    let response = s.form("/login", "owner", "correct password 123", "").await;
    assert_eq!(response.status(), 303);
    assert_eq!(response.headers()["location"], "/app/");
    let db = s.db();
    let password: String = db
        .query_row("SELECT password FROM browser_owner", [], |r| r.get(0))
        .unwrap();
    assert!(password.starts_with("$argon2id$"));
    assert!(!password.contains("correct password"));
    let persisted: String = db
        .query_row(
            "SELECT session_hash FROM browser_sessions LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!cookie.contains(&persisted));
    for code in &codes {
        let n: i64 = db
            .query_row(
                "SELECT count(*) FROM browser_recovery WHERE code_hash=?1",
                [hash(code)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
#[tokio::test]
async fn actual_websockets_close_on_logout_and_recovery_replay_fails() {
    let s = Server::start().await;
    let (cookie, codes) = s.enroll().await;
    assert!(
        s.websocket("/v1/events", &cookie, "https://hostile.test")
            .await
            .is_err()
    );
    assert!(
        s.websocket("/v1/drafts/events", &cookie, "https://hostile.test")
            .await
            .is_err()
    );
    let mut events = s.websocket("/v1/events", &cookie, ORIGIN).await.unwrap();
    let mut drafts = s
        .websocket("/v1/drafts/events", &cookie, ORIGIN)
        .await
        .unwrap();
    assert!(events.next().await.unwrap().unwrap().is_text());
    assert!(drafts.next().await.unwrap().unwrap().is_text());
    let value: serde_json::Value = s.session(&cookie).await.json().await.unwrap();
    let response = s
        .client
        .post(format!("{}/auth/logout", s.url))
        .header("cookie", &cookie)
        .header("origin", ORIGIN)
        .header("x-relay-csrf", value["csrf_token"].as_str().unwrap())
        .header("x-relay-protocol", "2")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    for socket in [&mut events, &mut drafts] {
        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(frame, Message::Close(_)));
    }
    assert_eq!(s.session(&cookie).await.status(), 401);
    let response = s
        .form("/recover", "", "replacement password 456", &codes[0])
        .await;
    assert_eq!(response.status(), 303);
    assert_eq!(
        s.form("/recover", "", "another password 789", &codes[0])
            .await
            .status(),
        401
    );
    assert_eq!(
        s.form("/login", "owner", "correct password 123", "")
            .await
            .status(),
        401
    );
    assert_eq!(
        s.form("/login", "owner", "replacement password 456", "")
            .await
            .status(),
        303
    );
}
#[tokio::test]
async fn form_origin_csrf_limits_and_expiry_fail_closed() {
    let s = Server::start().await;
    let (cookie, csrf) = s.form_token("/setup").await;
    for (origin, csrf) in [("https://evil.test", csrf.as_str()), (ORIGIN, "wrong")] {
        let r = s
            .client
            .post(format!("{}/setup", s.url))
            .header("origin", origin)
            .header("cookie", &cookie)
            .form(&[
                ("username", "owner"),
                ("password", "correct password 123"),
                ("code", CODE),
                ("csrf", csrf),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
    }
    let r = s
        .client
        .post(format!("{}/login", s.url))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("a".repeat(9000))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 413);
    let (cookie, _) = s.enroll().await;
    for column in ["created", "seen"] {
        let mut socket = s.websocket("/v1/events", &cookie, ORIGIN).await.unwrap();
        socket.next().await.unwrap().unwrap();
        s.db()
            .execute(&format!("UPDATE browser_sessions SET {column}=0"), [])
            .unwrap();
        assert_eq!(s.session(&cookie).await.status(), 401);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .is_close()
        );
        s.db()
            .execute(
                "UPDATE browser_sessions SET created=?1,seen=?1",
                [seconds()],
            )
            .unwrap();
    }
    for _ in 0..19 {
        assert_eq!(s.form("/login", "other", "wrong", "").await.status(), 401);
    }
    assert_eq!(s.form("/login", "owner", "wrong", "").await.status(), 429);
}
#[tokio::test]
async fn migration_rotation_restart_and_disabled_enrollment() {
    let s = Server::start().await;
    let (cookie, _) = s.enroll().await;
    let store = Store::open(&s.dir.path().join("db"), DirectorProfile::default()).unwrap();
    let before = store.snapshot().unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 5)
        .unwrap();
    drop(store);
    let mut store = Store::open(&s.dir.path().join("db"), DirectorProfile::default()).unwrap();
    assert_eq!(store.snapshot().unwrap(), before);
    assert_eq!(
        store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
    BrowserAuth::initialize(
        &mut store,
        TOKEN,
        s.shutdown.workspace.browser.config.clone(),
    )
    .unwrap();
    assert_eq!(s.session(&cookie).await.status(), 200);
    BrowserAuth::initialize(
        &mut store,
        "rotated-native-secret-0123456789",
        s.shutdown.workspace.browser.config.clone(),
    )
    .unwrap();
    assert_eq!(s.session(&cookie).await.status(), 401);
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM browser_owner", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let disabled = router(
        s.dir.path().join("disabled"),
        TOKEN.into(),
        DirectorProfile::default(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, disabled).await.unwrap();
    });
    assert_eq!(
        s.client
            .get(format!("{url}/v1/snapshot"))
            .header("cookie", cookie)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    task.abort();
}

#[tokio::test]
async fn enrollment_expiry_rotation_races_and_recovery_revocation() {
    let mut s = Server::start().await;
    s.db()
        .execute("UPDATE browser_setup SET expires=0", [])
        .unwrap();
    let config = s.shutdown.workspace.browser.config.clone();
    s.restart(TOKEN, config.clone()).await;
    assert_eq!(
        s.form("/setup", "owner", "correct password 123", CODE)
            .await
            .status(),
        401
    );
    let renewed = "fresh-enrollment-code-012345678901234567890123456789";
    std::fs::write(s.dir.path().join("setup"), renewed).unwrap();
    s.restart(TOKEN, config.clone()).await;
    // A native-token change cancels the newly opened enrollment window.
    s.restart("rotated-token-01234567890123456789", config.clone())
        .await;
    assert_eq!(
        s.form("/setup", "owner", "correct password 123", renewed)
            .await
            .status(),
        401
    );
    let renewed = "second-fresh-code-012345678901234567890123456789";
    std::fs::write(s.dir.path().join("setup"), renewed).unwrap();
    s.restart("rotated-token-01234567890123456789", config)
        .await;
    let (a, b) = tokio::join!(
        s.form("/setup", "owner", "correct password 123", renewed),
        s.form("/setup", "other", "correct password 123", renewed)
    );
    let (good, bad) = if a.status() == 200 { (a, b) } else { (b, a) };
    assert_eq!(good.status(), 200);
    assert_eq!(bad.status(), 401);
    let cookie = good.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let html = good.text().await.unwrap();
    let code = html.split("<pre>").nth(1).unwrap().lines().next().unwrap();
    let mut events = s.websocket("/v1/events", &cookie, ORIGIN).await.unwrap();
    let mut drafts = s
        .websocket("/v1/drafts/events", &cookie, ORIGIN)
        .await
        .unwrap();
    events.next().await.unwrap().unwrap();
    drafts.next().await.unwrap().unwrap();
    let (a, b) = tokio::join!(
        s.form("/recover", "", "replacement password 123", code),
        s.form("/recover", "", "replacement password 123", code)
    );
    assert!((a.status() == 303 && b.status() == 401) || (a.status() == 401 && b.status() == 303));
    assert_eq!(s.session(&cookie).await.status(), 401);
    for socket in [&mut events, &mut drafts] {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .is_close()
        );
    }
}
#[test]
fn real_schema_five_migration_preserves_snapshot_receipts_and_assets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(&path, DirectorProfile::default()).unwrap();
    let snapshot = store.snapshot().unwrap();
    store.connection.execute_batch("DROP TABLE browser_owner; DROP TABLE browser_setup; DROP TABLE browser_sessions; DROP TABLE browser_recovery; DROP TABLE browser_state; PRAGMA user_version=5; INSERT INTO assets VALUES('kept','metadata',X'0102'); INSERT INTO receipts VALUES('receipt','kept');").unwrap();
    drop(store);
    let store = Store::open(&path, DirectorProfile::default()).unwrap();
    assert_eq!(snapshot, store.snapshot().unwrap());
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT request FROM receipts WHERE request_id='receipt'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "kept"
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT bytes FROM assets WHERE id='kept'", [], |r| r
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM browser_owner", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn setup_file_is_private_new_and_origin_configuration_is_strict() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("code");
    initialize_setup_code(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.len(), 43);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(initialize_setup_code(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
    for origin in [
        "http://relay.test",
        "https://relay.test/",
        "https://relay.test/path",
        "https://relay.test?query",
        "https://user@relay.test",
        "not an origin",
    ] {
        assert!(
            BrowserConfig {
                public_origin: Some(origin.into()),
                ..BrowserConfig::default()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        BrowserConfig {
            public_origin: Some(ORIGIN.into()),
            ..BrowserConfig::default()
        }
        .validate()
        .is_ok()
    );
    assert_eq!(escape("<\"'&>"), "&lt;&quot;&#39;&amp;&gt;");
}

#[tokio::test]
async fn phone_forms_use_public_css_and_invalid_setup_is_rejected_before_password_validation() {
    let s = Server::start().await;
    for path in ["/login", "/setup", "/recover"] {
        let response = s
            .client
            .get(format!("{}{path}", s.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let html = response.text().await.unwrap();
        assert!(html.contains("width=device-width, initial-scale=1"));
        assert!(html.contains("href=/auth/style.css"));
        assert!(!html.contains("<style>"));
        assert!(!html.contains("<script"));
        assert!(html.contains("autocapitalize=none"));
    }
    let response = s
        .client
        .get(format!("{}/auth/style.css", s.url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/css; charset=utf-8"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(response.text().await.unwrap().contains("min-height: 44px"));
    // Enrollment eligibility is checked before even password-length feedback.
    assert_eq!(
        s.form("/setup", "owner", "short", "wrong enrollment code")
            .await
            .status(),
        401
    );
    assert_eq!(s.form("/setup", "owner", "short", CODE).await.status(), 200);
    let response = s
        .form("/setup", "owner", "correct password 123", CODE)
        .await;
    assert_eq!(response.status(), 200);
    let html = response.text().await.unwrap();
    assert!(html.contains("width=device-width, initial-scale=1"));
    assert!(html.contains("href=/auth/style.css"));
    assert_eq!(s.form("/setup", "owner", "short", CODE).await.status(), 401);
}
