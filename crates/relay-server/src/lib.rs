//! Single-writer persistent workspace and authenticated HTTP/WebSocket transport.

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use relay_core::{ApiError, CommandEnvelope, DirectorProfile, Snapshot, demo_snapshot};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::watch;

#[derive(Debug)]
pub struct Error {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl Error {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("Workspace storage error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage",
            "Workspace storage failed",
        )
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", message)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                code: self.code.into(),
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for Error {}

struct Store {
    connection: Connection,
}

impl Store {
    fn open(path: &Path, defaults: DirectorProfile) -> Result<Self, Error> {
        defaults.validate().map_err(Error::invalid)?;
        let connection = Connection::open(path).map_err(Error::internal)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(Error::internal)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(Error::internal)?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(Error::internal)?;
        if version > 1 {
            return Err(Error::invalid(
                "Database schema is newer than this Relay server",
            ));
        }
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS workspace (id INTEGER PRIMARY KEY CHECK(id = 1), snapshot TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS receipts (request_id TEXT PRIMARY KEY, request TEXT NOT NULL);
             PRAGMA user_version = 1;
             COMMIT;"
        ).map_err(Error::internal)?;
        let seed = serde_json::to_string(&demo_snapshot(defaults)).map_err(Error::internal)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO workspace(id, snapshot) VALUES (1, ?1)",
                [&seed],
            )
            .map_err(Error::internal)?;
        Ok(Self { connection })
    }
    fn snapshot(&self) -> Result<Snapshot, Error> {
        let json: String = self
            .connection
            .query_row("SELECT snapshot FROM workspace WHERE id = 1", [], |r| {
                r.get(0)
            })
            .map_err(Error::internal)?;
        serde_json::from_str(&json).map_err(Error::internal)
    }
    fn apply(&mut self, envelope: CommandEnvelope) -> Result<Snapshot, Error> {
        uuid::Uuid::parse_str(&envelope.request_id)
            .map_err(|_| Error::invalid("request_id must be a UUID"))?;
        let request = serde_json::to_string(&envelope).map_err(Error::internal)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let json: String = transaction
            .query_row("SELECT snapshot FROM workspace WHERE id = 1", [], |r| {
                r.get(0)
            })
            .map_err(Error::internal)?;
        let mut snapshot: Snapshot = serde_json::from_str(&json).map_err(Error::internal)?;
        let receipt: Option<String> = transaction
            .query_row(
                "SELECT request FROM receipts WHERE request_id = ?1",
                [&envelope.request_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::internal)?;
        if let Some(receipt) = receipt {
            if receipt != request {
                return Err(Error::invalid(
                    "request_id was already used for a different command",
                ));
            }
            return Ok(snapshot);
        }
        if snapshot.revision != envelope.expected_revision {
            return Err(Error::new(
                StatusCode::CONFLICT,
                "conflict",
                "Workspace changed. Refresh and review your draft before saving again.",
            ));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(Error::internal)?
            .as_secs();
        snapshot
            .apply(envelope.command, &envelope.request_id, now)
            .map_err(Error::invalid)?;
        let json = serde_json::to_string(&snapshot).map_err(Error::internal)?;
        transaction
            .execute("UPDATE workspace SET snapshot = ?1 WHERE id = 1", [&json])
            .map_err(Error::internal)?;
        transaction
            .execute(
                "INSERT INTO receipts(request_id, request) VALUES (?1, ?2)",
                params![envelope.request_id, request],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        Ok(snapshot)
    }
}

#[derive(Clone)]
struct Workspace {
    store: Arc<Mutex<Store>>,
    snapshots: watch::Sender<Snapshot>,
    token: Arc<str>,
}

impl Workspace {
    fn authorize(&self, headers: &HeaderMap) -> Result<(), Error> {
        let supplied = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or("");
        if supplied.as_bytes().ct_eq(self.token.as_bytes()).into() {
            return Ok(());
        }
        Err(Error::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A valid Relay bearer token is required",
        ))
    }
}

pub fn router(
    path: impl AsRef<Path>,
    token: String,
    defaults: DirectorProfile,
) -> Result<Router, Error> {
    if token.len() < 16 || token.trim() != token || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::invalid(
            "RELAY_TOKEN must contain at least 16 printable ASCII characters with no spaces",
        ));
    }
    let store = Store::open(path.as_ref(), defaults)?;
    let (snapshots, _) = watch::channel(store.snapshot()?);
    let workspace = Workspace {
        store: Arc::new(Mutex::new(store)),
        snapshots,
        token: token.into(),
    };
    Ok(Router::new()
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/commands", post(command))
        .route("/v1/events", get(events))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn_with_state(
            workspace.clone(),
            authorize,
        ))
        .with_state(workspace))
}

async fn authorize(
    State(workspace): State<Workspace>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, Error> {
    workspace.authorize(request.headers())?;
    Ok(next.run(request).await)
}

async fn snapshot(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
) -> Result<Json<Snapshot>, Error> {
    workspace.authorize(&headers)?;
    Ok(Json(workspace.snapshots.borrow().clone()))
}

async fn command(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    body: Result<Json<CommandEnvelope>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Snapshot>, Error> {
    workspace.authorize(&headers)?;
    let Json(envelope) =
        body.map_err(|_| Error::invalid("Expected a valid JSON command envelope"))?;
    let result = tokio::task::spawn_blocking(move || {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        let snapshot = store.apply(envelope)?;
        // Commit and publication share the lock, preserving revision order across writers.
        workspace.snapshots.send_replace(snapshot.clone());
        Ok::<_, Error>(snapshot)
    })
    .await
    .map_err(Error::internal)??;
    Ok(Json(result))
}

async fn events(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, Error> {
    workspace.authorize(&headers)?;
    let receiver = workspace.snapshots.subscribe();
    Ok(upgrade
        .max_message_size(64 * 1024)
        .on_upgrade(move |socket| stream(socket, receiver)))
}

async fn stream(mut socket: WebSocket, mut receiver: watch::Receiver<Snapshot>) {
    loop {
        let snapshot = receiver.borrow_and_update().clone();
        let Ok(json) = serde_json::to_string(&snapshot) else {
            break;
        };
        if socket.send(WsMessage::Text(json.into())).await.is_err() {
            break;
        }
        loop {
            tokio::select! {
                changed = receiver.changed() => {
                    if changed.is_err() { return; }
                    break;
                }
                incoming = socket.recv() => match incoming {
                    Some(Ok(WsMessage::Ping(data))) => {
                        if socket.send(WsMessage::Pong(data)).await.is_err() { return; }
                    }
                    Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => return,
                    _ => {},
                },
            }
        }
    }
}
