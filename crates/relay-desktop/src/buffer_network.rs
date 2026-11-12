use crate::network::Config;
#[cfg(not(target_arch = "wasm32"))]
use futures_util::{SinkExt, StreamExt};
use mosaic::prelude::StateSender;
use relay_core::*;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};

#[derive(Clone)]
pub enum Request {
    Catalog {
        session: String,
        force: bool,
    },
    ProfileCatalog {
        project: String,
        harness: Harness,
    },
    Save {
        session: String,
        request: SaveDraft,
        generation: u64,
    },
    Upload {
        asset: Asset,
        bytes: Arc<Vec<u8>>,
    },
    Fetch(String),
    Forget(Vec<String>),
}

// Credits cover queued, active and completed-but-unconsumed requests together.
// In particular, moving work out of the channel cannot create more admission room.
pub const ADMISSION_CAPACITY: usize = 32;
const ADMISSION_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct RequestSender {
    data: mpsc::Sender<Admitted>,
    control: mpsc::Sender<Vec<String>>,
    slots: Arc<tokio::sync::Semaphore>,
    bytes: Arc<tokio::sync::Semaphore>,
}
#[derive(Debug)]
pub struct AdmissionError;
struct Credit {
    _slot: tokio::sync::OwnedSemaphorePermit,
    _bytes: tokio::sync::OwnedSemaphorePermit,
}
struct Admitted {
    request: Request,
    credit: Credit,
}
pub struct RequestReceiver {
    data: mpsc::Receiver<Admitted>,
    control: mpsc::Receiver<Vec<String>>,
}
pub fn request_channel() -> (RequestSender, RequestReceiver) {
    let (data, receiver) = mpsc::channel(ADMISSION_CAPACITY);
    let (control, controls) = mpsc::channel(1);
    (
        RequestSender {
            data,
            control,
            slots: Arc::new(tokio::sync::Semaphore::new(ADMISSION_CAPACITY)),
            bytes: Arc::new(tokio::sync::Semaphore::new(ADMISSION_BYTES)),
        },
        RequestReceiver {
            data: receiver,
            control: controls,
        },
    )
}
impl RequestSender {
    /// Nonblocking admission. The caller retains its local data on refusal and may retry.
    /// Cleanup has an independent bounded lane; a refused cleanup is reissued with the
    /// next published outcome, so saturated data cannot prevent credits being returned.
    pub fn send(&self, request: Request) -> Result<(), AdmissionError> {
        if let Request::Forget(ids) = request {
            let storage = ids.iter().fold(
                ids.capacity().saturating_mul(std::mem::size_of::<String>()),
                |size, id| size.saturating_add(id.capacity()),
            );
            if ids.len() > ADMISSION_CAPACITY || storage > ADMISSION_CAPACITY * 256 {
                return Err(AdmissionError);
            }
            return self.control.try_send(ids).map_err(|_| AdmissionError);
        }
        let weight = request.weight().ok_or(AdmissionError)?;
        let slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AdmissionError)?;
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(weight)
            .map_err(|_| AdmissionError)?;
        self.data
            .try_send(Admitted {
                request,
                credit: Credit {
                    _slot: slot,
                    _bytes: bytes,
                },
            })
            .map_err(|_| AdmissionError)
    }
}
#[cfg(test)]
impl RequestReceiver {
    pub fn try_recv(&mut self) -> Result<Request, mpsc::error::TryRecvError> {
        if let Ok(ids) = self.control.try_recv() {
            return Ok(Request::Forget(ids));
        }
        self.data.try_recv().map(|item| item.request)
    }
}
// Count encoded payload without allocating another large serialization buffer.
struct PayloadSize(usize);
impl std::io::Write for PayloadSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn payload_size(value: &impl serde::Serialize) -> Option<usize> {
    let mut size = PayloadSize(0);
    serde_json::to_writer(&mut size, value).ok()?;
    Some(size.0)
}
fn forget(state: &mut Update, credits: &mut BTreeMap<String, Credit>, ids: Vec<String>) {
    for id in ids {
        state.outcomes.remove(&id);
        credits.remove(&id);
    }
}
fn completed(
    state: &mut Update,
    credits: &mut BTreeMap<String, Credit>,
    delta: Update,
    credit: Credit,
) {
    // Each request has at most one terminal outcome. Catalogs have none and release
    // their credit here. Other credits last until the UI consumes their outcome.
    if let Some(id) = delta.outcomes.keys().next() {
        credits.insert(id.clone(), credit);
    }
    merge_io(state, delta);
}

impl Request {
    fn weight(&self) -> Option<u32> {
        let size = match self {
            Self::Save {
                session, request, ..
            } => session
                .len()
                .saturating_add(payload_size(request)?.saturating_mul(2))
                .saturating_add(
                    request
                        .parts
                        .len()
                        .saturating_mul(std::mem::size_of::<Part>()),
                ),
            Self::Upload { asset, bytes } => bytes
                .capacity()
                .saturating_add(payload_size(asset)?.saturating_mul(2)),
            Self::Fetch(id) => id.len(),
            Self::Catalog { session, .. } => session.len(),
            Self::ProfileCatalog { project, .. } => project.len(),
            Self::Forget(_) => return None,
        }
        .saturating_add(std::mem::size_of::<Request>());
        (size <= ADMISSION_BYTES).then_some(size as u32)
    }

    fn catalog_route(&self) -> (String, String, bool) {
        let encode = |id: &str| {
            percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
        };
        match self {
            Self::Catalog { session, force } => (
                session.clone(),
                format!("v1/sessions/{}/catalog", encode(session)),
                *force,
            ),
            Self::ProfileCatalog { project, harness } => (
                format!("profile:{project}:{harness:?}"),
                format!(
                    "v1/projects/{}/catalog/{}",
                    encode(project),
                    if *harness == Harness::Codex {
                        "codex"
                    } else {
                        "claude_code"
                    }
                ),
                false,
            ),
            _ => unreachable!("not a catalogue request"),
        }
    }
}

/// Retry only ambiguous transport/server failures. Client refusals require user review.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFailure {
    Retryable,
    Conflict,
    Permanent,
}
fn save_failure(status: reqwest::StatusCode) -> SaveFailure {
    if status == reqwest::StatusCode::CONFLICT {
        SaveFailure::Conflict
    } else if status.is_server_error() || matches!(status.as_u16(), 408 | 429) {
        SaveFailure::Retryable
    } else {
        SaveFailure::Permanent
    }
}

#[derive(Clone)]
pub enum Outcome {
    Saved {
        session: String,
        request: SaveDraft,
        generation: u64,
        result: Result<Draft, (SaveFailure, String)>,
    },
    Fetched {
        id: String,
        result: Result<(), String>,
    },
    Uploaded {
        asset: Asset,
        result: Result<(), String>,
    },
}

#[derive(Clone, Default)]
pub struct Update {
    pub drafts: Vec<Draft>,
    pub catalogs: BTreeMap<String, Result<HarnessCatalog, String>>,
    pub blobs: BTreeMap<String, Arc<Vec<u8>>>,
    pub outcomes: BTreeMap<String, Outcome>,
    pub initialized: bool,
    pub connected: bool,
    pub serial: u64,
}

enum Event {
    Drafts(Vec<Draft>),
    Connected(bool),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start(config: Config, sender: StateSender<Update>) -> RequestSender {
    let (
        requests,
        RequestReceiver {
            data: mut receiver,
            control: mut controls,
        },
    ) = request_channel();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build().unwrap();
            let (events, mut updates) = mpsc::channel(8);
            let watcher = tokio::spawn(watch(config.clone(), events.clone()));
            let mut state = Update::default();
        let mut credits = BTreeMap::new();
        let mut pending: std::collections::VecDeque<Admitted> = std::collections::VecDeque::new();
        let mut active = std::collections::BTreeSet::new();
        let mut jobs = futures_util::stream::FuturesUnordered::new();
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            loop {
                if let Ok(ids) = controls.try_recv() { forget(&mut state, &mut credits, ids); }
            tokio::select! {
                    _ = tick.tick() => {},
                    event = updates.recv() => match event {
                        Some(Event::Drafts(drafts)) => { state.drafts = drafts; state.initialized = true; },
                        Some(Event::Connected(connected)) => state.connected = connected,
                        None => break,
                    },
                Some((key, delta, credit)) = jobs.next(), if !jobs.is_empty() => {
                    active.remove(&key);
                    completed(&mut state, &mut credits, delta, credit);
                },
                    control = controls.recv() => { if let Some(ids) = control { forget(&mut state, &mut credits, ids); } },
                    request = receiver.recv() => match request {
                    Some(request) => pending.push_back(request),
                        None => break,
                    }
                }
            while let Some(index) = pending.iter().position(|r| can_dispatch(&r.request, &active)) {
                let Admitted {request, credit} = pending.remove(index).unwrap();
                let key = io_key(&request);
                active.insert(key.clone());
                let client = client.clone(); let config = config.clone();
                jobs.push(async move { (key, execute(client, config, request).await, credit) });
            }
                state.serial += 1;
                if sender.send(state.clone()).is_err() { break; }
            }
            watcher.abort();
        });
    });
    requests
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_bytes(
    client: &reqwest::Client,
    config: &Config,
    id: &str,
) -> Result<Vec<u8>, String> {
    let error = || "Cannot load inline file; click its preview to retry".to_owned();
    let mut response = client
        .get(config.url(&format!("v1/assets/{id}")))
        .bearer_auth(&config.token)
        .send()
        .await
        .map_err(|_| error())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > ASSET_LIMIT as u64)
    {
        return Err(error());
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| error())? {
        if bytes.len() + chunk.len() > ASSET_LIMIT {
            return Err(error());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
async fn watch(config: Config, events: mpsc::Sender<Event>) {
    loop {
        let result: Result<(), ()> = async {
            let mut url = config.url("v1/drafts/events");
            url.set_scheme(if config.endpoint.scheme() == "https" { "wss" } else { "ws" }).map_err(|_| ())?;
            let mut request = url.as_str().into_client_request().map_err(|_| ())?;
            request.headers_mut().insert("authorization", HeaderValue::from_str(&format!("Bearer {}",config.token)).map_err(|_| ())?);
            let (mut socket,_) = tokio::time::timeout(Duration::from_secs(10),tokio_tungstenite::connect_async(request)).await.map_err(|_| ())?.map_err(|_| ())?;
            events.send(Event::Connected(true)).await.map_err(|_| ())?;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
            let mut last = tokio::time::Instant::now();
            loop { tokio::select! {
                _ = heartbeat.tick() => { if last.elapsed() > Duration::from_secs(45) { return Err(()); } socket.send(Message::Ping(vec![].into())).await.map_err(|_| ())?; },
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Text(json))) => { last = tokio::time::Instant::now(); events.send(Event::Drafts(serde_json::from_str(&json).map_err(|_| ())?)).await.map_err(|_| ())?; },
                    Some(Ok(Message::Ping(data))) => { last = tokio::time::Instant::now(); socket.send(Message::Pong(data)).await.map_err(|_| ())?; },
                    Some(Ok(Message::Pong(_))) => last = tokio::time::Instant::now(),
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Err(()),
                    _ => {},
                }
            } }
        }.await;
        if result.is_err() && events.send(Event::Connected(false)).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(target_arch = "wasm32")]
#[path = "buffer_network_web.rs"]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(not(target_arch = "wasm32"))]
async fn execute_inner(client: reqwest::Client, config: Config, request: Request) -> Update {
    let mut state = Update::default();
    match request {
        request @ (Request::Catalog { .. } | Request::ProfileCatalog { .. }) => {
            let (key, path, force) = request.catalog_route();
            let request = if force {
                client.post(config.url(&path))
            } else {
                client.get(config.url(&path))
            };
            let result = match request
                .bearer_auth(&config.token)
                .header("X-Relay-Protocol", PROTOCOL_VERSION.to_string())
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response
                    .json::<HarnessCatalog>()
                    .await
                    .map_err(|_| "Invalid harness catalog".into()),
                _ => Err("Cannot discover harness commands".into()),
            };
            state.catalogs.insert(key, result);
        }
        Request::Save {
            session,
            request,
            generation,
        } => {
            let result = match client
                .post(config.url(&format!(
                    "v1/drafts/{}",
                    percent_encoding::utf8_percent_encode(
                        &session,
                        percent_encoding::NON_ALPHANUMERIC
                    )
                )))
                .bearer_auth(&config.token)
                .header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string())
                .json(&request)
                .send()
                .await
            {
                Ok(response) => {
                    let conflict = save_failure(response.status());
                    if response.status().is_success() {
                        response.json::<Draft>().await.map_err(|_| {
                            (
                                SaveFailure::Retryable,
                                "Draft save could not be confirmed; exact retry is retained".into(),
                            )
                        })
                    } else if conflict != SaveFailure::Retryable {
                        Err((
                            conflict,
                            if conflict == SaveFailure::Conflict {
                                "Shared draft changed; local content is retained".into()
                            } else {
                                "Draft save refused; check access and whether the chat still exists"
                                    .into()
                            },
                        ))
                    } else {
                        Err((
                            conflict,
                            response
                                .json::<ApiError>()
                                .await
                                .map(|e| e.message)
                                .unwrap_or_else(|_| "Draft save was rejected".into()),
                        ))
                    }
                }
                Err(_) => Err((
                    SaveFailure::Retryable,
                    "Cannot confirm draft save; local content is retained".into(),
                )),
            };
            if let Ok(draft) = &result {
                if let Some(old) = state.drafts.iter_mut().find(|d| d.session_id == session) {
                    if draft.revision >= old.revision {
                        *old = draft.clone();
                    }
                } else {
                    state.drafts.push(draft.clone());
                }
            }
            state.outcomes.insert(
                format!("{}:{generation}", request.request_id),
                Outcome::Saved {
                    session,
                    request,
                    generation,
                    result,
                },
            );
        }
        Request::Upload { asset, bytes } => {
            let result = match client
                .post(config.url(&format!("v1/assets/{}", asset.id)))
                .bearer_auth(&config.token)
                .header("X-Relay-Protocol", relay_core::PROTOCOL_VERSION.to_string())
                .header("content-type", &asset.media_type)
                .header(
                    "x-relay-filename",
                    percent_encoding::utf8_percent_encode(
                        &asset.name,
                        percent_encoding::NON_ALPHANUMERIC,
                    )
                    .to_string(),
                )
                .body(bytes.as_ref().clone())
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response
                    .json::<Asset>()
                    .await
                    .map_err(|_| "File upload acknowledgement was invalid".into())
                    .and_then(|saved| {
                        if saved == asset {
                            Ok(())
                        } else {
                            Err("Uploaded file metadata changed".into())
                        }
                    }),
                Ok(response) => Err(response
                    .json::<ApiError>()
                    .await
                    .map(|e| e.message)
                    .unwrap_or_else(|_| "File upload failed".into())),
                Err(_) => Err("File upload could not be confirmed; retry retains its ID".into()),
            };
            state.blobs.insert(asset.id.clone(), bytes);
            state
                .outcomes
                .insert(asset.id.clone(), Outcome::Uploaded { asset, result });
        }
        Request::Fetch(id) => {
            let result = match fetch_bytes(&client, &config, &id).await {
                Ok(bytes) => {
                    state.blobs.insert(id.clone(), Arc::new(bytes));
                    Ok(())
                }
                Err(error) => Err(error),
            };
            state
                .outcomes
                .insert(format!("fetch-{id}"), Outcome::Fetched { id, result });
        }
        _ => unreachable!("control request dispatched as I/O"),
    }
    state
}

#[cfg(not(target_arch = "wasm32"))]
async fn execute(client: reqwest::Client, config: Config, request: Request) -> Update {
    execute_bounded(client, config, request, 30_000).await
}
#[cfg(not(target_arch = "wasm32"))]
async fn execute_bounded(
    client: reqwest::Client,
    config: Config,
    request: Request,
    deadline_ms: u32,
) -> Update {
    let backup = request.clone();
    let job = execute_inner(client, config, request);
    tokio::select! {
        result = job => result,
        _ = io_deadline(deadline_ms) => {
            let mut state = Update::default();
            let message = "Request timed out; exact retry preserves its ID".to_owned();
            let (id, outcome) = match backup {
                Request::Save { session, request, generation } => (format!("{}:{generation}", request.request_id), Outcome::Saved {session, request, generation, result: Err((SaveFailure::Retryable, message))}),
                Request::Upload {asset, ..} => (asset.id.clone(), Outcome::Uploaded {asset, result: Err(message)}),
                Request::Fetch(id) => (format!("fetch-{id}"), Outcome::Fetched {id, result: Err(message)}),
                request @ (Request::Catalog {..} | Request::ProfileCatalog {..}) => {
                    state.catalogs.insert(request.catalog_route().0, Err(message));
                    return state;
                },
                _ => unreachable!(),
            };
            state.outcomes.insert(id, outcome);
            state
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
async fn io_deadline(deadline_ms: u32) {
    tokio::time::sleep(Duration::from_millis(u64::from(deadline_ms))).await;
}

// Two saves and two media requests may run independently. A document has one lane,
// including after watcher acknowledgement, until the prior HTTP attempt terminates.
fn io_key(request: &Request) -> (u8, String) {
    match request {
        Request::Save { session, .. } => (0, session.clone()),
        Request::Upload { asset, .. } => (1, asset.id.clone()),
        Request::Fetch(id) => (1, id.clone()),
        request @ (Request::Catalog { .. } | Request::ProfileCatalog { .. }) => {
            (2, request.catalog_route().0)
        }
        _ => unreachable!(),
    }
}
fn can_dispatch(request: &Request, active: &std::collections::BTreeSet<(u8, String)>) -> bool {
    let key = io_key(request);
    !active.contains(&key) && active.iter().filter(|k| k.0 == key.0).count() < 2
}
fn merge_io(state: &mut Update, delta: Update) {
    for draft in delta.drafts {
        if let Some(old) = state
            .drafts
            .iter_mut()
            .find(|d| d.session_id == draft.session_id)
        {
            if draft.revision >= old.revision {
                *old = draft;
            }
        } else {
            state.drafts.push(draft);
        }
    }
    state.catalogs.extend(delta.catalogs);
    state.blobs.extend(delta.blobs);
    state.outcomes.extend(delta.outcomes);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn save_classification_and_scheduler_bounds() {
        use reqwest::StatusCode as S;
        for status in [
            S::UNAUTHORIZED,
            S::FORBIDDEN,
            S::NOT_FOUND,
            S::GONE,
            S::BAD_REQUEST,
            S::UNPROCESSABLE_ENTITY,
        ] {
            assert_eq!(save_failure(status), SaveFailure::Permanent);
        }
        assert_eq!(save_failure(S::CONFLICT), SaveFailure::Conflict);
        for status in [
            S::REQUEST_TIMEOUT,
            S::TOO_MANY_REQUESTS,
            S::INTERNAL_SERVER_ERROR,
            S::SERVICE_UNAVAILABLE,
        ] {
            assert_eq!(save_failure(status), SaveFailure::Retryable);
        }
        let request = |session: &str| Request::Save {
            session: session.into(),
            request: SaveDraft {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 0,
                parts: vec![],
            },
            generation: 1,
        };
        let mut active = std::collections::BTreeSet::new();
        active.insert(io_key(&request("a")));
        assert!(!can_dispatch(&request("a"), &active));
        assert!(can_dispatch(&request("b"), &active));
        active.insert(io_key(&request("b")));
        assert!(!can_dispatch(&request("c"), &active));
        assert!(can_dispatch(&Request::Fetch("media".into()), &active));
        active.clear();
        active.insert(io_key(&Request::Fetch("one".into())));
        active.insert(io_key(&Request::Fetch("two".into())));
        assert!(!can_dispatch(&Request::Fetch("three".into()), &active));
        assert!(can_dispatch(&request("a"), &active));
    }

    #[tokio::test]
    async fn stalled_body_has_complete_deadline_while_unrelated_io_progresses() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut bytes = vec![0; 8192];
                    let n = socket.read(&mut bytes).await.unwrap();
                    if bytes[..n].starts_with(b"GET")
                        && !bytes[..n].starts_with(b"GET /v1/assets/stalled")
                    {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await.unwrap();
                    } else {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                            .await
                            .unwrap();
                        // Header success must not escape the complete-response deadline.
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                });
            }
        });
        let config = Config {
            endpoint: format!("http://{address}/").parse().unwrap(),
            token: String::new(),
        };
        let request = SaveDraft {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: 0,
            parts: vec![Part::text("synthetic")],
        };
        let start = std::time::Instant::now();
        let stalled = execute_bounded(
            reqwest::Client::new(),
            config.clone(),
            Request::Save {
                session: "a".into(),
                request: request.clone(),
                generation: 7,
            },
            200,
        );
        let stalled_media = execute_bounded(
            reqwest::Client::new(),
            config.clone(),
            Request::Fetch("stalled".into()),
            200,
        );
        let independent = async {
            let result = execute_bounded(
                reqwest::Client::new(),
                config,
                Request::Fetch("file".into()),
                100,
            )
            .await;
            assert!(matches!(
                &result.outcomes["fetch-file"],
                Outcome::Fetched { result: Ok(()), .. }
            ));
            assert!(start.elapsed() < Duration::from_millis(200));
        };
        let (result, media, ()) = tokio::join!(stalled, stalled_media, independent);
        assert!(matches!(
            &media.outcomes["fetch-stalled"],
            Outcome::Fetched { result: Err(_), .. }
        ));
        assert!(matches!(
            &result.outcomes[&format!("{}:7", request.request_id)],
            Outcome::Saved {
                generation: 7,
                result: Err((SaveFailure::Retryable, _)),
                ..
            }
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        server.abort();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[tokio::test]
async fn native_dispatcher_publishes_watcher_and_other_documents_during_stalled_save() {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0; 8192];
                let n = socket.peek(&mut buf).await.unwrap();
                if buf[..n].starts_with(b"GET /v1/drafts/events") {
                    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                    ws.send(Message::Text("[]".into())).await.unwrap();
                    while ws.next().await.is_some() {}
                } else {
                    use tokio::io::AsyncReadExt;
                    let n = socket.read(&mut buf).await.unwrap();
                    if buf[..n].starts_with(b"POST /v1/drafts/a ") {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n")
                            .await
                            .unwrap();
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    } else {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\nConnection: close\r\n\r\n{\"session_id\":\"b\",\"revision\":1,\"parts\":[]}").await.unwrap();
                    }
                }
            });
        }
    });
    let (state, sender) = mosaic::prelude::state_channel(Update::default());
    let requests = start(
        Config {
            endpoint: format!("http://{addr}/").parse().unwrap(),
            token: String::new(),
        },
        sender,
    );
    for session in ["a", "b"] {
        requests
            .send(Request::Save {
                session: session.into(),
                generation: 1,
                request: SaveDraft {
                    request_id: session.into(),
                    expected_revision: 0,
                    parts: vec![],
                },
            })
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            mosaic::core::reactive::flush();
            let update = state.get_untracked();
            if update.connected && update.initialized && update.outcomes.contains_key("b:1") {
                assert!(!update.outcomes.contains_key("a:1"));
                assert!(matches!(
                    &update.outcomes["b:1"],
                    Outcome::Saved { result: Ok(_), .. }
                ));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(requests);
    server.abort();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod admission_tests {
    use super::*;

    #[tokio::test]
    async fn saturation_bounds_entire_backlog_and_cleanup_releases_admission() {
        let (sender, mut receiver) = request_channel();
        for index in 0..ADMISSION_CAPACITY {
            sender
                .send(Request::Fetch(format!("file-{index}")))
                .unwrap();
        }
        for _ in 0..1000 {
            assert!(sender.send(Request::Fetch("refused".into())).is_err());
        }
        assert_eq!(receiver.data.len(), ADMISSION_CAPACITY);
        assert_eq!(sender.slots.available_permits(), 0);
        let item = receiver.data.recv().await.unwrap();
        // Dequeuing, starting, and completing I/O do not return admission capacity.
        assert!(sender.send(Request::Fetch("still refused".into())).is_err());
        let mut state = Update::default();
        let mut credits = BTreeMap::new();
        let mut delta = Update::default();
        delta.outcomes.insert(
            "receipt".into(),
            Outcome::Fetched {
                id: "file-0".into(),
                result: Ok(()),
            },
        );
        completed(&mut state, &mut credits, delta, item.credit);
        assert!(
            sender
                .send(Request::Fetch("completed unconsumed".into()))
                .is_err()
        );
        // Control is admitted despite full data credits and is bounded independently.
        assert!(
            sender
                .send(Request::Forget(vec!["x".repeat(ADMISSION_CAPACITY * 256)]))
                .is_err()
        );
        sender
            .send(Request::Forget(vec!["receipt".into()]))
            .unwrap();
        assert!(
            sender
                .send(Request::Forget(vec!["receipt".into()]))
                .is_err()
        );
        forget(
            &mut state,
            &mut credits,
            receiver.control.recv().await.unwrap(),
        );
        assert!(state.outcomes.is_empty());
        sender.send(Request::Fetch("recovered".into())).unwrap();
        assert_eq!(receiver.data.len(), ADMISSION_CAPACITY);
        assert_eq!(sender.slots.available_permits(), 0);
        drop(receiver); // Closed producer must refuse without retaining request payload.
        assert!(sender.send(Request::Fetch("closed".into())).is_err());
        assert_eq!(sender.slots.available_permits(), ADMISSION_CAPACITY);
    }

    #[tokio::test]
    async fn byte_budget_refuses_large_payload_without_leaking_credits() {
        let (sender, mut receiver) = request_channel();
        let bytes = Arc::new(vec![0; 20 * 1024 * 1024]);
        let asset = Asset {
            id: "synthetic".into(),
            name: "file".into(),
            media_type: "application/octet-stream".into(),
            size: bytes.len() as u64,
        };
        sender
            .send(Request::Upload {
                asset: asset.clone(),
                bytes: bytes.clone(),
            })
            .unwrap();
        for _ in 0..1000 {
            assert!(
                sender
                    .send(Request::Upload {
                        asset: asset.clone(),
                        bytes: bytes.clone()
                    })
                    .is_err()
            );
        }
        assert_eq!(receiver.data.len(), 1);
        assert_eq!(Arc::strong_count(&bytes), 2);
        assert_eq!(sender.slots.available_permits(), ADMISSION_CAPACITY - 1);
        assert!(sender.bytes.available_permits() < 12 * 1024 * 1024);
        drop(receiver.data.recv().await.unwrap());
        assert_eq!(sender.bytes.available_permits(), ADMISSION_BYTES);
        sender.send(Request::Upload { asset, bytes }).unwrap();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[tokio::test]
async fn saturated_dispatcher_cleans_up_and_serializes_new_generation_after_watcher_ack() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let a_started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let active_a = Arc::new(AtomicUsize::new(0));
    let peak_a = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let a_started = a_started.clone();
        let release = release.clone();
        let active_a = active_a.clone();
        let peak_a = peak_a.clone();
        let calls = calls.clone();
        async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (a_started, release, active_a, peak_a, calls) = (
                    a_started.clone(),
                    release.clone(),
                    active_a.clone(),
                    peak_a.clone(),
                    calls.clone(),
                );
                tokio::spawn(async move {
                    let mut buf = [0; 8192];
                    let n = socket.peek(&mut buf).await.unwrap();
                    if buf[..n].starts_with(b"GET /v1/drafts/events") {
                        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
                        ws.send(Message::Text("[]".into())).await.unwrap();
                        a_started.notified().await;
                        ws.send(Message::Text(
                            "[{\"session_id\":\"a\",\"revision\":1,\"parts\":[]}]".into(),
                        ))
                        .await
                        .unwrap();
                        while ws.next().await.is_some() {}
                    } else {
                        let n = socket.read(&mut buf).await.unwrap();
                        let is_a = buf[..n].starts_with(b"POST /v1/drafts/a ");
                        let old = is_a && calls.fetch_add(1, Ordering::SeqCst) == 0;
                        if is_a {
                            let count = active_a.fetch_add(1, Ordering::SeqCst) + 1;
                            peak_a.fetch_max(count, Ordering::SeqCst);
                        }
                        let body = if old {
                            a_started.notify_one();
                            release.notified().await;
                            // Obsolete HTTP failure after a successful watcher acknowledgement.
                            "{\"code\":\"unavailable\",\"message\":\"synthetic\"}".to_owned()
                        } else if is_a {
                            "{\"session_id\":\"a\",\"revision\":2,\"parts\":[]}".to_owned()
                        } else if buf[..n].starts_with(b"POST") {
                            "{\"session_id\":\"b\",\"revision\":1,\"parts\":[]}".to_owned()
                        } else if buf[..n].starts_with(b"GET /v1/assets/slow") {
                            std::future::pending::<()>().await;
                            unreachable!()
                        } else {
                            "ok".to_owned()
                        };
                        if is_a {
                            active_a.fetch_sub(1, Ordering::SeqCst);
                        }
                        let status = if old {
                            "503 Service Unavailable"
                        } else {
                            "200 OK"
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                    }
                });
            }
        }
    });
    let (state, sender) = mosaic::prelude::state_channel(Update::default());
    let requests = start(
        Config {
            endpoint: format!("http://{addr}/").parse().unwrap(),
            token: String::new(),
        },
        sender,
    );
    let save = |session: &str, generation| Request::Save {
        session: session.into(),
        generation,
        request: SaveDraft {
            request_id: format!("{session}-{generation}"),
            expected_revision: generation - 1,
            parts: vec![],
        },
    };
    requests.send(save("a", 1)).unwrap();
    requests.send(Request::Fetch("fast".into())).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            mosaic::core::reactive::flush();
            let update = state.get_untracked();
            if update
                .drafts
                .iter()
                .any(|d| d.session_id == "a" && d.revision == 1)
                && update.outcomes.contains_key("fetch-fast")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    requests.send(save("a", 2)).unwrap();
    for index in 0..ADMISSION_CAPACITY - 3 {
        requests
            .send(Request::Fetch(format!("slow-{index}")))
            .unwrap();
    }
    assert_eq!(requests.slots.available_permits(), 0);
    assert!(requests.send(save("b", 1)).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    requests
        .send(Request::Forget(vec!["fetch-fast".into()]))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while requests.slots.available_permits() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    requests.send(save("b", 1)).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            mosaic::core::reactive::flush();
            let update = state.get_untracked();
            if update.outcomes.contains_key("b-1:1") {
                assert!(!update.outcomes.contains_key("fetch-fast"));
                assert!(!update.outcomes.contains_key("a-2:2"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            mosaic::core::reactive::flush();
            let update = state.get_untracked();
            if update.outcomes.contains_key("a-2:2") {
                assert!(matches!(
                    &update.outcomes["a-1:1"],
                    Outcome::Saved {
                        generation: 1,
                        result: Err(_),
                        ..
                    }
                ));
                assert!(matches!(
                    &update.outcomes["a-2:2"],
                    Outcome::Saved {
                        generation: 2,
                        result: Ok(_),
                        ..
                    }
                ));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(peak_a.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(requests);
    server.abort();
}
