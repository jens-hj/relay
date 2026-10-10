//! Same-origin browser transport. Session cookies stay in the browser; no bearer token is stored.
use crate::network::Config;
use std::cell::RefCell;
use wasm_bindgen::{JsCast, closure::Closure};

#[derive(Clone, serde::Deserialize)]
struct Session {
    authenticated: bool,
    username: String,
    csrf_token: String,
}
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}
pub fn login() {
    SESSION.with(|s| *s.borrow_mut() = None);
    let key = RECOVERY.with(|r| std::mem::take(&mut *r.borrow_mut()).0);
    wasm_bindgen_futures::spawn_local(async move {
        if !key.is_empty() {
            let _ = wasm_bindgen_futures::JsFuture::from(recovery_delete(&key)).await;
        }
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_href("/login");
        }
    });
}
async fn session(config: &Config) -> Result<Session, String> {
    if let Some(session) = SESSION.with(|s| s.borrow().clone()) {
        return Ok(session);
    }
    let response = reqwest::Client::new()
        .get(config.url("auth/session"))
        .fetch_credentials_same_origin()
        .send()
        .await
        .map_err(|_| "Cannot check login session")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        login();
        return Err("Login required".into());
    }
    let session: Session = response
        .error_for_status()
        .map_err(|_| "Cannot check login session")?
        .json()
        .await
        .map_err(|_| "Invalid login session")?;
    if !session.authenticated || session.csrf_token.is_empty() {
        login();
        return Err("Login required".into());
    }
    SESSION.with(|s| *s.borrow_mut() = Some(session.clone()));
    Ok(session)
}
pub async fn send(
    config: &Config,
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    let session = session(config).await?;
    let request = request
        .fetch_credentials_same_origin()
        .header("x-relay-protocol", "2")
        .header("x-relay-csrf", &session.csrf_token);
    let response = tokio::select! {
        response = request.send() => response.map_err(|_| "Request could not be confirmed; exact retry preserves its ID")?,
        _ = sleep(30_000) => return Err("Request timed out; exact retry preserves its ID".into()),
    };
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        login();
        return Err("Login required".into());
    }
    Ok(response)
}
pub async fn sleep(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}
pub fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Browser unavailable")?
        .local_storage()
        .map_err(|_| "Browser storage is unavailable")?
        .ok_or_else(|| "Browser storage is disabled".into())
}
pub(crate) fn surface_color(color: mosaic::prelude::Color) {
    let [r, g, b, _] = color.to_srgb8();
    browser_surface(&format!("#{r:02x}{g:02x}{b:02x}"));
}
pub fn read(key: &str) -> Result<Option<String>, String> {
    storage()?
        .get_item(key)
        .map_err(|_| "Cannot read browser recovery storage".into())
}
pub fn write(key: &str, value: &str) -> Result<(), String> {
    storage()?
        .set_item(key, value)
        .map_err(|_| "Cannot save browser recovery; storage may be full".into())
}

// Event-owned clipboard data avoids browser permission prompts and supports image/file paste.
#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/browser_clipboard.js")]
extern "C" {
    fn installFilePaste(accepts: &js_sys::Function, receive: &js_sys::Function);
}

fn receive_files(model: crate::model::Model, files: Vec<web_sys::File>) {
    let session = model.session.get_untracked();
    if session.is_empty() {
        model
            .notice
            .set("Open a conversation before adding files".into());
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        for file in files {
            if file.size() > relay_core::ASSET_LIMIT as f64 {
                model.notice.set("File exceeds 20 MiB".into());
                continue;
            }
            if let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                let media = if file.type_().is_empty() {
                    "application/octet-stream".into()
                } else {
                    file.type_()
                };
                let mut parts = crate::buffer::parts(model, &session);
                let existing: u64 = parts
                    .iter()
                    .filter_map(|p| {
                        if let relay_core::PartKind::Asset { asset } = &p.kind {
                            Some(asset.size)
                        } else {
                            None
                        }
                    })
                    .sum();
                if existing + bytes.len() as u64 > relay_core::DRAFT_ASSET_LIMIT {
                    model.notice.set("Draft files exceed 64 MiB".into());
                    continue;
                }
                match crate::buffer::insert_asset(model, file.name(), media, bytes) {
                    Ok(part) => {
                        parts.push(part);
                        crate::buffer::edit(model, &session, parts.clone());
                    }
                    Err(error) => model.notice.set(error),
                }
            } else {
                model.notice.set("Cannot read the selected file".into());
            }
        }
    });
}
pub fn install_input(model: crate::model::Model) {
    crate::browser_text::install(model.ui.get_untracked(), model.browser_viewport);
    let accepts = Closure::<dyn FnMut() -> bool>::new(move || {
        let ui = model.ui.get_untracked();
        let Some(field) = ui.focused() else {
            return false;
        };
        ui.inspection_snapshot()
            .node(field.id())
            .and_then(|n| n.label.as_ref())
            .is_some_and(|label| label.starts_with("Draft text ") || label.starts_with("Message "))
    });
    let receive = Closure::<dyn FnMut(js_sys::Array)>::new(move |files: js_sys::Array| {
        receive_files(
            model,
            files
                .iter()
                .filter_map(|file| file.dyn_into::<web_sys::File>().ok())
                .collect(),
        );
    });
    installFilePaste(
        accepts.as_ref().unchecked_ref(),
        receive.as_ref().unchecked_ref(),
    );
    accepts.forget();
    receive.forget();
}
pub fn pick_files(model: crate::model::Model) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(element) = document.create_element("input") else {
        return;
    };
    let Ok(input) = element.dyn_into::<web_sys::HtmlInputElement>() else {
        return;
    };
    input.set_type("file");
    input.set_multiple(true);
    let selected = input.clone();
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        if let Some(files) = selected.files() {
            receive_files(
                model,
                (0..files.length()).filter_map(|i| files.item(i)).collect(),
            );
        }
    });
    input.set_onchange(Some(callback.as_ref().unchecked_ref()));
    input.click();
    callback.forget();
}

// IndexedDB transactions are serialized so older writes can never replace a newer journal.
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function browser_surface(color) {
 document.documentElement.style.backgroundColor = color;
 document.body.style.backgroundColor = color;
 let meta = document.querySelector('meta[name="theme-color"]');
 if (!meta) { meta = document.createElement('meta'); meta.name = 'theme-color'; document.head.appendChild(meta); }
 meta.content = color;
}
export function touch_device() { return window.matchMedia('(pointer: coarse)').matches; }
export function installed_touch_app() {
 return touch_device() && (window.matchMedia('(display-mode: standalone)').matches || navigator.standalone === true);
}
let database;
let writes = Promise.resolve();
async function db() {
  if (!database) database = new Promise((resolve, reject) => {
    const request = indexedDB.open('relay-recovery-v1', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('journals');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return database;
}
export async function relayRecoveryKey(username, csrf, tab) {
  // CSRF is session-bound; only its noncredential fingerprint enters storage.
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(csrf));
  const fingerprint = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
  return JSON.stringify(['relay-drafts-v2', username, fingerprint, tab]);
}
export async function relayRecoveryRead(key) {
  const database = await db();
  return new Promise((resolve, reject) => {
    const request = database.transaction('journals').objectStore('journals').get(key);
    request.onsuccess = () => resolve(request.result || '');
    request.onerror = () => reject(request.error);
  });
}
export function relayRecoveryDelete(key) {
  const clear = writes.catch(() => {}).then(async () => {
    const database = await db();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction('journals', 'readwrite');
      transaction.objectStore('journals').delete(key);
      transaction.oncomplete = () => resolve();
      transaction.onabort = () => reject(transaction.error);
    });
  });
  writes = clear;
  return clear;
}
export function relayRecoveryWrite(key, source) {
  const write = writes.catch(() => {}).then(async () => {
    const database = await db();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction('journals', 'readwrite');
      transaction.objectStore('journals').put(source, key);
      transaction.oncomplete = () => resolve();
      transaction.onabort = () => reject(transaction.error);
      transaction.onerror = () => reject(transaction.error);
    });
  });
  writes = write;
  return write;
}
"#)]
extern "C" {
    pub(crate) fn touch_device() -> bool;
    pub(crate) fn installed_touch_app() -> bool;
    fn browser_surface(color: &str);
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = relayRecoveryKey)]
    fn recovery_key(username: &str, csrf: &str, tab: &str) -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = relayRecoveryRead)]
    fn recovery_read(key: &str) -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = relayRecoveryWrite)]
    fn recovery_write(key: &str, source: &str) -> js_sys::Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = relayRecoveryDelete)]
    fn recovery_delete(key: &str) -> js_sys::Promise;
}
thread_local! {
    static RECOVERY: RefCell<(String,String)> = const { RefCell::new((String::new(), String::new())) };
}
pub async fn prepare(config: &Config) -> Result<(), String> {
    let authenticated = session(config).await?;
    // IndexedDB itself is origin-scoped; the account identifies this server's workspace access.
    let tab = web_sys::window()
        .ok_or("Browser unavailable")?
        .session_storage()
        .map_err(|_| "Browser session storage unavailable")?
        .ok_or("Browser session storage disabled")?;
    let tab_id = match tab
        .get_item("relay-device-session")
        .map_err(|_| "Cannot read device session")?
    {
        Some(id) => id,
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            tab.set_item("relay-device-session", &id)
                .map_err(|_| "Cannot save device session")?;
            id
        }
    };
    // Never migrate username-only journals into a new authenticated session.
    let key = wasm_bindgen_futures::JsFuture::from(recovery_key(
        &authenticated.username,
        &authenticated.csrf_token,
        &tab_id,
    ))
    .await
    .map_err(|_| "Cannot identify the authenticated recovery session")?
    .as_string()
    .ok_or("Invalid recovery session identity")?;
    let source = wasm_bindgen_futures::JsFuture::from(recovery_read(&key))
        .await
        .map_err(
            |_| "Cannot open durable browser draft recovery. Check browser storage permissions.",
        )?
        .as_string()
        .unwrap_or_default();
    crate::buffer::validate_browser_recovery(&source)?;
    RECOVERY.with(|r| *r.borrow_mut() = (key, source));
    Ok(())
}
pub fn recovered() -> String {
    RECOVERY.with(|r| r.borrow().1.clone())
}
pub fn persist(source: String, model: crate::model::Model) {
    let key = RECOVERY.with(|r| {
        let mut record = r.borrow_mut();
        if record.0.is_empty() || record.1 == source {
            return None;
        }
        record.1 = source.clone();
        Some(record.0.clone())
    });
    let Some(key) = key else {
        return;
    };
    // Start/enqueue immediately; transaction completion, rather than enqueue, confirms durability.
    let promise = recovery_write(&key, &source);
    wasm_bindgen_futures::spawn_local(async move {
        if wasm_bindgen_futures::JsFuture::from(promise).await.is_err() {
            RECOVERY.with(|r| r.borrow_mut().1.clear());
            model.notice.set("Cannot save local draft recovery. Browser storage is full or unavailable; keep this tab open.".into());
        }
    });
}
pub fn startup_error(error: &str) {
    if let Some(document) = web_sys::window().and_then(|w| w.document())
        && let Some(body) = document.body()
    {
        body.set_text_content(Some(error));
    }
}

pub fn logout(model: crate::model::Model) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = async {
            let config = Config::from_env()?;
            let response = send(
                &config,
                reqwest::Client::new().post(config.url("auth/logout")),
            )
            .await?;
            if !response.status().is_success() {
                return Err("Sign out failed; try again".to_owned());
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => login(),
            Err(error) => model.notice.set(error),
        }
    });
}

pub async fn connect(
    config: &Config,
    path: &str,
) -> Result<gloo_net::websocket::futures::WebSocket, String> {
    use gloo_net::websocket::{State, futures::WebSocket};
    let mut url = config.url(path);
    url.set_scheme(if config.endpoint.scheme() == "https" {
        "wss"
    } else {
        "ws"
    })
    .map_err(|_| "Invalid event URL")?;
    let socket = WebSocket::open(url.as_str()).map_err(|_| "Cannot open event connection")?;
    let started = crate::platform::Instant::now();
    loop {
        match socket.state() {
            State::Open => return Ok(socket),
            State::Connecting if started.elapsed() < std::time::Duration::from_secs(10) => {
                sleep(50).await
            }
            _ => return Err("Event connection failed or timed out".into()),
        }
    }
}
