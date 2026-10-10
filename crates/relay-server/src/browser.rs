//! Single-owner browser credentials. Raw secrets are never persisted in SQLite.
use super::*;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Form,
    http::{HeaderValue, Method},
    response::{Html, Redirect},
};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};

const COOKIE: &str = "__Host-relay_session";
const IDLE: i64 = 7 * 86400;
const ABSOLUTE: i64 = 30 * 86400;

#[derive(Clone, Default)]
pub struct BrowserConfig {
    pub public_origin: Option<String>,
    pub setup_token_file: Option<PathBuf>,
    pub web_root: Option<PathBuf>,
}
impl BrowserConfig {
    pub fn from_env() -> Result<Self, Error> {
        let config = Self {
            public_origin: std::env::var("RELAY_PUBLIC_ORIGIN").ok(),
            setup_token_file: std::env::var_os("RELAY_SETUP_TOKEN_FILE").map(PathBuf::from),
            web_root: std::env::var_os("RELAY_WEB_ROOT").map(PathBuf::from),
        };
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), Error> {
        if let Some(origin) = &self.public_origin {
            let uri: axum::http::Uri = origin
                .parse()
                .map_err(|_| Error::invalid("Invalid RELAY_PUBLIC_ORIGIN"))?;
            if uri.scheme_str() != Some("https")
                || uri.authority().is_none()
                || uri.authority().is_some_and(|a| a.as_str().contains('@'))
                || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
                || origin.ends_with('/')
            {
                return Err(Error::invalid(
                    "RELAY_PUBLIC_ORIGIN must be an exact HTTPS origin without a trailing slash",
                ));
            }
        }
        Ok(())
    }
}
struct Attempts {
    since: i64,
    count: u32,
}
pub(super) struct BrowserAuth {
    config: BrowserConfig,
    attempts: Mutex<Attempts>,
    // Random process-local pre-session form token, supplied in HTML and a secure cookie.
    form_token: String,
    dummy_password: String,
    setup_hash: Option<String>,
}
fn seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random() -> String {
    use base64::Engine;
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn password_hash(password: &str) -> Result<String, Error> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(Error::internal)
}
fn password_matches(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}
fn equal(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}
fn denied() -> Error {
    Error::new(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "Authentication required",
    )
}
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|p| p.trim().split_once('='))
        .filter(|(key, _)| *key == name)
        .map(|(_, value)| value);
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    Some(value)
}
impl BrowserAuth {
    pub(super) fn initialize(
        store: &mut Store,
        token: &str,
        config: BrowserConfig,
    ) -> Result<Arc<Self>, Error> {
        config.validate()?;
        let tx = store
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let fingerprint = hash(token);
        let old: Option<String> = tx
            .query_row("SELECT token_hash FROM browser_state WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(Error::internal)?;
        if old.as_deref() != Some(&fingerprint) {
            tx.execute_batch("DELETE FROM browser_sessions; UPDATE browser_setup SET expires=0;")
                .map_err(Error::internal)?;
            tx.execute("INSERT INTO browser_state(id,token_hash) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET token_hash=excluded.token_hash", [&fingerprint]).map_err(Error::internal)?;
        }
        let owner: bool = tx
            .query_row("SELECT EXISTS(SELECT 1 FROM browser_owner)", [], |r| {
                r.get(0)
            })
            .map_err(Error::internal)?;
        let mut setup_hash = None;
        if !owner && let Some(path) = &config.setup_token_file {
            let code = std::fs::read_to_string(path)
                .map_err(|_| Error::invalid("Cannot read RELAY_SETUP_TOKEN_FILE"))?;
            let code = code.trim();
            if code.len() < 32 || code.len() > 256 || !code.bytes().all(|b| b.is_ascii_graphic()) {
                return Err(Error::invalid(
                    "Setup code must contain 32 to 256 printable characters",
                ));
            }
            setup_hash = Some(hash(code));
            tx.execute(
                "UPDATE browser_setup SET expires=0 WHERE code_hash<>?1",
                [hash(code)],
            )
            .map_err(Error::internal)?;
            // A consumed/expired configured code is not rearmed on restart.
            tx.execute(
                "INSERT OR IGNORE INTO browser_setup(code_hash,expires) VALUES(?1,?2)",
                params![hash(code), seconds() + 600],
            )
            .map_err(Error::internal)?;
        }
        tx.commit().map_err(Error::internal)?;
        Ok(Arc::new(Self {
            setup_hash,
            config,
            attempts: Mutex::new(Attempts {
                since: seconds(),
                count: 0,
            }),
            form_token: random(),
            dummy_password: {
                static DUMMY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
                    password_hash(&random()).expect("Argon2 configuration")
                });
                DUMMY.clone()
            },
        }))
    }
    fn origin(&self, headers: &HeaderMap) -> Result<(), Error> {
        let expected = self.config.public_origin.as_deref().ok_or_else(denied)?;
        if headers.get_all("origin").iter().count() != 1
            || headers.get("origin").and_then(|h| h.to_str().ok()) != Some(expected)
        {
            return Err(Error::new(
                StatusCode::FORBIDDEN,
                "origin",
                "Invalid request origin",
            ));
        }
        Ok(())
    }
    fn throttle(&self) -> Result<(), Error> {
        let mut a = self.attempts.lock().map_err(Error::internal)?;
        if seconds() - a.since >= 900 {
            a.since = seconds();
            a.count = 0;
        }
        // Single owner: a global budget also bounds attempts against any account spelling.
        if a.count >= 20 {
            return Err(Error::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit",
                "Try again later",
            ));
        }
        a.count += 1;
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Session {
    pub username: String,
    pub csrf: String,
}
impl Workspace {
    pub(super) fn browser_session(
        &self,
        headers: &HeaderMap,
        touch: bool,
    ) -> Result<Session, Error> {
        if self.browser.config.public_origin.is_none() {
            return Err(denied());
        }
        let raw = cookie(headers, COOKIE).ok_or_else(denied)?;
        if raw.len() > 128 {
            return Err(denied());
        }
        let store = self.store.lock().map_err(Error::internal)?;
        let row: Option<(String,String,i64,i64)> = store.connection.query_row(
            "SELECT o.username,s.csrf,s.created,s.seen FROM browser_sessions s CROSS JOIN browser_owner o WHERE s.session_hash=?1",
            [hash(raw)], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(Error::internal)?;
        let (username, csrf, created, seen) = row.ok_or_else(denied)?;
        if seconds() >= created + ABSOLUTE || seconds() >= seen + IDLE {
            return Err(denied());
        }
        if touch {
            store
                .connection
                .execute(
                    "UPDATE browser_sessions SET seen=?1 WHERE session_hash=?2",
                    params![seconds(), hash(raw)],
                )
                .map_err(Error::internal)?;
        }
        Ok(Session { username, csrf })
    }
    pub(super) fn browser_request(
        &self,
        headers: &HeaderMap,
        method: &Method,
        websocket: bool,
    ) -> Result<(), Error> {
        self.authorize(headers)?;
        if headers.contains_key("authorization") {
            return Ok(());
        }
        if websocket || !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
            self.browser.origin(headers)?;
            if !websocket {
                let session = self.browser_session(headers, false)?;
                if !headers
                    .get("x-relay-csrf")
                    .and_then(|h| h.to_str().ok())
                    .is_some_and(|v| equal(v, &session.csrf))
                {
                    return Err(Error::new(
                        StatusCode::FORBIDDEN,
                        "csrf",
                        "Invalid request token",
                    ));
                }
            }
        }
        self.browser_session(headers, true)?;
        Ok(())
    }
    pub(super) async fn session_ended(&self, headers: &HeaderMap) {
        if headers.contains_key("authorization") {
            std::future::pending::<()>().await;
        }
        loop {
            if self.browser_session(headers, false).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn secure_cookie(name: &str, value: &str, max_age: i64) -> HeaderValue {
    format!("{name}={value}; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}")
        .parse()
        .expect("generated cookie")
}
fn document(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><link rel=stylesheet href=/auth/style.css><title>{}</title></head><body><main>{body}</main></body></html>",
        escape(title)
    ))
}
async fn stylesheet() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("browser/auth.css"),
    )
}
fn page(workspace: &Workspace, flow: &str, error: &str) -> Response {
    let fields = match flow {
        "setup" => {
            "<label>Setup code <input name=code required autocomplete=off autocapitalize=none spellcheck=false maxlength=256></label><label>Username <input name=username required autocomplete=username autocapitalize=none spellcheck=false maxlength=128></label><label>Password <input name=password type=password required autocomplete=new-password></label>"
        }
        "recover" => {
            "<label>Recovery code <input name=code required autocomplete=off autocapitalize=none spellcheck=false maxlength=256></label><label>New password <input name=password type=password required autocomplete=new-password></label>"
        }
        _ => {
            "<label>Username <input name=username required autocomplete=username autocapitalize=none spellcheck=false maxlength=128></label><label>Password <input name=password type=password required autocomplete=current-password></label>"
        }
    };
    let title = match flow {
        "setup" => "Set up owner",
        "recover" => "Recover password",
        _ => "Log in",
    };
    let error = if error.is_empty() {
        String::new()
    } else {
        format!("<p role=alert>{}</p>", escape(error))
    };
    let mut response = document(title, &format!("<h1>{title}</h1>{error}<form method=post action=/{flow}><input type=hidden name=csrf value=\"{}\">{fields}<button type=submit>{title}</button></form><nav aria-label=Authentication><a href=/login>Log in</a><a href=/recover>Recover password</a></nav>",workspace.browser.form_token)).into_response();
    response.headers_mut().append(
        "set-cookie",
        secure_cookie("__Host-relay_form", &workspace.browser.form_token, 900),
    );
    response
}
async fn login_page(State(w): State<Workspace>) -> Response {
    page(&w, "login", "")
}
async fn setup_page(State(w): State<Workspace>) -> Response {
    page(&w, "setup", "")
}
async fn recover_page(State(w): State<Workspace>) -> Response {
    page(&w, "recover", "")
}
async fn submit(
    State(w): State<Workspace>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    Form(form): Form<Credentials>,
) -> Response {
    let flow = uri.path().trim_start_matches('/');
    let result = match flow {
        "setup" => setup(State(w.clone()), headers, Form(form)).await,
        "recover" => recover(State(w.clone()), headers, Form(form)).await,
        _ => login(State(w.clone()), headers, Form(form)).await,
    };
    match result {
        Ok(response) => response,
        Err(error) => {
            let mut response = page(&w, flow, &error.message);
            *response.status_mut() = error.status;
            response
        }
    }
}
#[derive(Deserialize)]
struct Credentials {
    #[serde(default)]
    username: String,
    password: String,
    #[serde(default)]
    code: String,
    csrf: String,
}
fn form_check(w: &Workspace, headers: &HeaderMap, f: &Credentials) -> Result<(), Error> {
    w.browser.origin(headers)?;
    if !equal(&f.csrf, &w.browser.form_token)
        || cookie(headers, "__Host-relay_form") != Some(w.browser.form_token.as_str())
    {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            "csrf",
            "Invalid request token",
        ));
    }
    w.browser.throttle()?;
    if f.password.len() > 1024 || f.username.len() > 128 || f.code.len() > 256 {
        return Err(denied());
    }
    Ok(())
}
fn new_session(connection: &Connection) -> Result<String, Error> {
    connection
        .execute(
            "DELETE FROM browser_sessions WHERE created<=?1 OR seen<=?2",
            params![seconds() - ABSOLUTE, seconds() - IDLE],
        )
        .map_err(Error::internal)?;
    let secret = random();
    connection
        .execute(
            "INSERT INTO browser_sessions(session_hash,csrf,created,seen) VALUES(?1,?2,?3,?3)",
            params![hash(&secret), random(), seconds()],
        )
        .map_err(Error::internal)?;
    Ok(secret)
}
fn session_response(secret: &str) -> Response {
    let mut response = Redirect::to("/app/").into_response();
    response
        .headers_mut()
        .append("set-cookie", secure_cookie(COOKIE, secret, ABSOLUTE));
    response
}
async fn login(
    State(w): State<Workspace>,
    headers: HeaderMap,
    Form(f): Form<Credentials>,
) -> Result<Response, Error> {
    form_check(&w, &headers, &f)?;
    let stored: Option<(String, String)> = w
        .store
        .lock()
        .map_err(Error::internal)?
        .connection
        .query_row(
            "SELECT username,password FROM browser_owner WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(Error::internal)?;
    let encoded = stored
        .as_ref()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| w.browser.dummy_password.clone());
    let verified_hash = encoded.clone();
    let password = f.password;
    let valid = tokio::task::spawn_blocking(move || password_matches(&password, &encoded))
        .await
        .map_err(Error::internal)?;
    if !valid || !stored.is_some_and(|r| equal(&r.0, &f.username)) {
        let mut response = page(&w, "login", "Invalid username or password");
        *response.status_mut() = StatusCode::UNAUTHORIZED;
        return Ok(response);
    }
    // Verify the password hash still matches when creating the session (recovery race).
    let store = w.store.lock().map_err(Error::internal)?;
    let current: String = store
        .connection
        .query_row("SELECT password FROM browser_owner WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(Error::internal)?;
    if current != verified_hash {
        return Err(denied());
    }
    Ok(session_response(&new_session(&store.connection)?))
}
async fn setup(
    State(w): State<Workspace>,
    headers: HeaderMap,
    Form(f): Form<Credentials>,
) -> Result<Response, Error> {
    form_check(&w, &headers, &f)?;
    let code_hash = hash(&f.code);
    if w.browser.setup_hash.as_deref() != Some(code_hash.as_str()) {
        return Err(denied());
    }
    // Reject invalid, expired or closed enrollment before allocating Argon2 work.
    // The transaction below still rechecks and consumes atomically after hashing.
    let eligible: bool = w.store.lock().map_err(Error::internal)?.connection.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM browser_owner) AND EXISTS(SELECT 1 FROM browser_setup WHERE code_hash=?1 AND expires>?2)",
        params![code_hash, seconds()], |r|r.get(0)).map_err(Error::internal)?;
    if !eligible {
        return Err(denied());
    }
    if f.username.trim().is_empty() || f.password.len() < 12 {
        return Ok(page(
            &w,
            "setup",
            "Enter a username and a password of at least 12 characters",
        ));
    }
    let encoded = tokio::task::spawn_blocking(move || password_hash(&f.password))
        .await
        .map_err(Error::internal)??;
    let codes: Vec<String> = (0..8).map(|_| random()).collect();
    let mut store = w.store.lock().map_err(Error::internal)?;
    let tx = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(Error::internal)?;
    let exists: bool = tx
        .query_row("SELECT EXISTS(SELECT 1 FROM browser_owner)", [], |r| {
            r.get(0)
        })
        .map_err(Error::internal)?;
    let consumed = tx
        .execute(
            "UPDATE browser_setup SET expires=0 WHERE code_hash=?1 AND expires>?2",
            params![code_hash, seconds()],
        )
        .map_err(Error::internal)?;
    if exists || consumed != 1 {
        return Err(denied());
    }
    tx.execute(
        "INSERT INTO browser_owner(id,username,password) VALUES(1,?1,?2)",
        params![f.username, encoded],
    )
    .map_err(Error::internal)?;
    for code in &codes {
        tx.execute(
            "INSERT INTO browser_recovery(code_hash) VALUES(?1)",
            [hash(code)],
        )
        .map_err(Error::internal)?;
    }
    tx.execute("UPDATE browser_setup SET expires=0", [])
        .map_err(Error::internal)?;
    let session = new_session(&tx)?;
    tx.commit().map_err(Error::internal)?;
    let mut response = document("Recovery codes", &format!("<h1>Save recovery codes</h1><p>These codes are shown once. Each code can reset your password once.</p><pre>{}</pre><nav><a href=/app/>Continue</a></nav>",codes.join("\n"))).into_response();
    response
        .headers_mut()
        .append("set-cookie", secure_cookie(COOKIE, &session, ABSOLUTE));
    Ok(response)
}
async fn recover(
    State(w): State<Workspace>,
    headers: HeaderMap,
    Form(f): Form<Credentials>,
) -> Result<Response, Error> {
    form_check(&w, &headers, &f)?;
    if f.password.len() < 12 {
        return Ok(page(
            &w,
            "recover",
            "Use a password of at least 12 characters",
        ));
    }
    let encoded = tokio::task::spawn_blocking(move || password_hash(&f.password))
        .await
        .map_err(Error::internal)??;
    let mut store = w.store.lock().map_err(Error::internal)?;
    let tx = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(Error::internal)?;
    if tx
        .execute(
            "DELETE FROM browser_recovery WHERE code_hash=?1",
            [hash(&f.code)],
        )
        .map_err(Error::internal)?
        != 1
    {
        return Err(denied());
    }
    tx.execute("UPDATE browser_owner SET password=?1 WHERE id=1", [encoded])
        .map_err(Error::internal)?;
    tx.execute_batch("DELETE FROM browser_sessions;")
        .map_err(Error::internal)?;
    let session = new_session(&tx)?;
    tx.commit().map_err(Error::internal)?;
    Ok(session_response(&session))
}
async fn session(
    State(w): State<Workspace>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Error> {
    // This endpoint describes browser sessions, never native credentials.
    if headers.contains_key("authorization") {
        w.authorize(&headers)?;
    }
    let s = w.browser_session(&headers, true)?;
    Ok(Json(
        serde_json::json!({"authenticated":true,"username":s.username,"csrf_token":s.csrf}),
    ))
}
async fn logout(State(w): State<Workspace>, headers: HeaderMap) -> Result<Response, Error> {
    w.browser_request(&headers, &Method::POST, false)?;
    if headers
        .get("x-relay-protocol")
        .and_then(|v| v.to_str().ok())
        != Some("3")
    {
        return Err(Error::new(
            StatusCode::CONFLICT,
            "protocol_mismatch",
            "Relay protocol 3 is required",
        ));
    }
    if let Some(raw) = cookie(&headers, COOKIE) {
        w.store
            .lock()
            .map_err(Error::internal)?
            .connection
            .execute(
                "DELETE FROM browser_sessions WHERE session_hash=?1",
                [hash(raw)],
            )
            .map_err(Error::internal)?;
    }
    let mut response = Redirect::to("/login").into_response();
    response
        .headers_mut()
        .append("set-cookie", secure_cookie(COOKIE, "", 0));
    Ok(response)
}
async fn root(State(w): State<Workspace>, headers: HeaderMap) -> Redirect {
    Redirect::to(if w.authorize(&headers).is_ok() {
        "/app/"
    } else {
        "/login"
    })
}
async fn app_gate(
    State(w): State<Workspace>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let decoded = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8_lossy();
    if decoded != "/app" && !decoded.starts_with("/app/") {
        return next.run(request).await;
    }
    let entry = decoded.ends_with(".html") || decoded.ends_with('/') || decoded == "/app";
    if w.authorize(request.headers()).is_err() {
        return if entry {
            Redirect::to("/login").into_response()
        } else {
            denied().into_response()
        };
    }
    next.run(request).await
}
pub(super) fn routes(w: Workspace) -> Router<Workspace> {
    let mut router = Router::new()
        .route("/", get(root))
        .route("/login", get(login_page).post(submit))
        .route("/setup", get(setup_page).post(submit))
        .route("/recover", get(recover_page).post(submit))
        .route("/auth/session", get(session))
        .route("/auth/style.css", get(stylesheet))
        .route("/auth/logout", post(logout))
        .layer(DefaultBodyLimit::max(8192));
    if let Some(root) = &w.browser.config.web_root {
        router = router
            .nest_service(
                "/app",
                tower_http::services::ServeDir::new(root)
                    .append_index_html_on_directories(true)
                    .precompressed_gzip(),
            )
            .layer(axum::middleware::from_fn_with_state(w, app_gate));
    }
    router
}
pub(super) async fn response_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"));
    response
}

/// Generate an enrollment secret without overwriting an existing file or printing it.
pub fn initialize_setup_code(path: &Path) -> Result<(), Error> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| Error::invalid("Cannot create setup code file (it must not already exist)"))?;
    file.write_all(random().as_bytes())
        .map_err(|_| Error::invalid("Cannot write setup code file"))?;
    file.sync_all()
        .map_err(|_| Error::invalid("Cannot persist setup code file"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
