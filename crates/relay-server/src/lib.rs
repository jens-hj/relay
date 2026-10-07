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
use relay_core::*;
mod github;
mod process;
mod runtime;
pub use runtime::{RemoteConfig, RuntimeConfig};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::HashMap;
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
    controls: HashMap<String, watch::Sender<bool>>,
    closing: bool,
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
        if version > 2 {
            return Err(Error::invalid(
                "Database schema is newer than this Relay server",
            ));
        }
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS workspace (id INTEGER PRIMARY KEY CHECK(id = 1), snapshot TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS receipts (request_id TEXT PRIMARY KEY, request TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS runs (session_id TEXT PRIMARY KEY, run_id TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS syncs (project_id TEXT PRIMARY KEY, request_id TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS processes (session_id TEXT PRIMARY KEY, run_id TEXT NOT NULL, pid INTEGER NOT NULL, identity TEXT NOT NULL);
             PRAGMA user_version = 2;
             COMMIT;"
        ).map_err(Error::internal)?;
        let seed = serde_json::to_string(&demo_snapshot(defaults)).map_err(Error::internal)?;
        connection
            .execute(
                "INSERT OR IGNORE INTO workspace(id, snapshot) VALUES (1, ?1)",
                [&seed],
            )
            .map_err(Error::internal)?;
        Ok(Self {
            connection,
            controls: HashMap::new(),
            closing: false,
        })
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
    fn save(&mut self, snapshot: &Snapshot) -> Result<(), Error> {
        self.connection
            .execute(
                "UPDATE workspace SET snapshot = ?1 WHERE id = 1",
                [serde_json::to_string(snapshot).map_err(Error::internal)?],
            )
            .map_err(Error::internal)?;
        Ok(())
    }
    fn run_id(&self, session: &str) -> Result<Option<String>, Error> {
        self.connection
            .query_row(
                "SELECT run_id FROM runs WHERE session_id=?1",
                [session],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::internal)
    }
    fn apply(
        &mut self,
        envelope: CommandEnvelope,
        config: &RuntimeConfig,
    ) -> Result<(Snapshot, Option<Action>), Error> {
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
            return Ok((snapshot, None));
        }
        if snapshot.revision != envelope.expected_revision {
            return Err(Error::new(
                StatusCode::CONFLICT,
                "conflict",
                "Workspace changed. Refresh and review your draft before saving again.",
            ));
        }
        let mut action = None;
        match envelope.command {
            Command::SyncProject { project_id } => {
                let project = snapshot.project(&project_id).map_err(Error::invalid)?;
                if project.github.is_none() {
                    return Err(Error::invalid("Project has no configured GitHub board"));
                }
                if !runtime::configured_project(project, config) {
                    return Err(Error::invalid(
                        "Project is not the currently configured repository and GitHub board",
                    ));
                }
                action = Some(Action::Sync(project_id));
            }
            Command::StartWorker {
                issue_id,
                director_id,
                prompt,
                approve_implementation,
            } => {
                validate_prompt(&prompt)?;
                runtime::authorize_turn(
                    &snapshot,
                    &issue_id,
                    &director_id,
                    approve_implementation,
                    config,
                )?;
                let issue = snapshot.issues.iter().find(|i| i.id == issue_id).unwrap();
                let session_id = format!("session-{}", envelope.request_id);
                snapshot.sessions.push(Session {
                    id: session_id.clone(),
                    project_id: issue.project_id.clone(),
                    issue_id: Some(issue_id),
                    director_id,
                    title: issue.title.clone(),
                    role: SessionRole::Worker,
                    fixture: false,
                    worker: Some(WorkerRun {
                        status: WorkerStatus::Queued,
                        thread_id: None,
                        worktree: None,
                        branch: None,
                        base_commit: None,
                        error: None,
                        usage: None,
                        changes: None,
                    }),
                });
                snapshot
                    .messages
                    .push(prompt_message(&session_id, &envelope.request_id, &prompt));
                action = Some(Action::Run { session_id, prompt });
            }
            Command::SendWorker {
                session_id,
                prompt,
                approve_implementation,
            } => {
                validate_prompt(&prompt)?;
                let session = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .ok_or_else(|| Error::invalid("Session not found"))?;
                let worker = session
                    .worker
                    .as_ref()
                    .ok_or_else(|| Error::invalid("Not a worker session"))?;
                if runtime::active(&worker.status) {
                    return Err(Error::invalid("Worker already has an active turn"));
                }
                if worker.thread_id.is_none() || worker.worktree.is_none() {
                    return Err(Error::invalid(
                        "Worker has no recorded thread/worktree to resume",
                    ));
                }
                runtime::authorize_turn(
                    &snapshot,
                    session
                        .issue_id
                        .as_deref()
                        .ok_or_else(|| Error::invalid("Session has no issue"))?,
                    &session.director_id,
                    approve_implementation,
                    config,
                )?;
                let worker = snapshot
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == session_id)
                    .unwrap()
                    .worker
                    .as_mut()
                    .unwrap();
                worker.status = WorkerStatus::Queued;
                worker.error = None;
                worker.usage = None;
                snapshot
                    .messages
                    .push(prompt_message(&session_id, &envelope.request_id, &prompt));
                action = Some(Action::Run { session_id, prompt });
            }
            Command::StopWorker { session_id } => {
                let worker = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .and_then(|s| s.worker.as_ref())
                    .ok_or_else(|| Error::invalid("Worker session not found"))?;
                if !runtime::active(&worker.status) {
                    return Err(Error::invalid("Worker has no active turn"));
                }
                if !self.controls.contains_key(&session_id) {
                    return Err(Error::invalid(
                        "Worker process is unavailable; restart to classify interrupted runs",
                    ));
                }
                action = Some(Action::Stop(session_id));
            }
            command => {
                snapshot
                    .apply(command, &envelope.request_id, now())
                    .map_err(Error::invalid)?;
            }
        }
        if action.is_some() {
            snapshot.revision = snapshot
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        }
        if let Some(Action::Run { session_id, .. }) = &action {
            transaction.execute("INSERT INTO runs(session_id,run_id) VALUES(?1,?2) ON CONFLICT(session_id) DO UPDATE SET run_id=excluded.run_id",params![session_id,envelope.request_id]).map_err(Error::internal)?;
        }
        if let Some(Action::Sync(project_id)) = &action {
            transaction.execute("INSERT INTO syncs(project_id,request_id) VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET request_id=excluded.request_id",params![project_id,envelope.request_id]).map_err(Error::internal)?;
        }
        transaction
            .execute(
                "UPDATE workspace SET snapshot = ?1 WHERE id = 1",
                [serde_json::to_string(&snapshot).map_err(Error::internal)?],
            )
            .map_err(Error::internal)?;
        transaction
            .execute(
                "INSERT INTO receipts(request_id, request) VALUES (?1, ?2)",
                params![envelope.request_id, request],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        Ok((snapshot, action))
    }
}

#[derive(Clone)]
struct Workspace {
    transport_shutdown: watch::Sender<bool>,
    transports: Arc<std::sync::atomic::AtomicUsize>,
    store: Arc<Mutex<Store>>,
    snapshots: watch::Sender<Snapshot>,
    token: Arc<str>,
    config: Arc<RuntimeConfig>,
}

enum Action {
    Sync(String),
    Run { session_id: String, prompt: String },
    Stop(String),
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn validate_prompt(prompt: &str) -> Result<(), Error> {
    if prompt.trim().is_empty() || prompt.len() > 32_000 {
        Err(Error::invalid(
            "Prompt must be nonempty and at most 32000 bytes",
        ))
    } else {
        Ok(())
    }
}
fn prompt_message(session: &str, request: &str, prompt: &str) -> Message {
    Message {
        id: format!("prompt-{request}"),
        session_id: session.into(),
        author: "You".into(),
        kind: "prompt".into(),
        body: prompt.into(),
    }
}
impl Workspace {
    fn interrupt_run(&self, session: &str, run: &str) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(Error::internal)?;
        if store.run_id(session)?.as_deref() != Some(run) {
            return Ok(());
        }
        let mut snapshot = store.snapshot()?;
        let Some(worker) = snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == session)
            .and_then(|s| s.worker.as_mut())
            .filter(|w| runtime::active(&w.status))
        else {
            return Ok(());
        };
        let process: Option<(u32, String)> = store
            .connection
            .query_row(
                "SELECT pid,identity FROM processes WHERE session_id=?1 AND run_id=?2",
                params![session, run],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(Error::internal)?;
        if let Some((pid, identity)) = process {
            runtime::reap_owned(pid, &identity);
        }
        worker.status = WorkerStatus::Interrupted;
        worker.error = Some(
            "Server execution task ended before a terminal outcome; continuation is explicit"
                .into(),
        );
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&snapshot)?;
        store.controls.remove(session);
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
    fn update_run(
        &self,
        session: &str,
        run: &str,
        update: impl FnOnce(&mut Snapshot) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(Error::internal)?;
        if store.run_id(session)?.as_deref() != Some(run) {
            return Err(Error::invalid("Run superseded"));
        }
        let mut snapshot = store.snapshot()?;
        if !snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| runtime::active(&w.status))
        {
            return Err(Error::invalid("Run is no longer active"));
        }
        let previous = snapshot.clone();
        update(&mut snapshot)?;
        if snapshot == previous {
            return Ok(());
        }
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&snapshot)?;
        if snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| !runtime::active(&w.status))
        {
            store.controls.remove(session);
        }
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
    fn synchronize(&self, project_id: &str, request_id: &str) {
        let result = github::sync(&self.config, project_id);
        let update = || -> Result<(), Error> {
            let mut store = self.store.lock().map_err(Error::internal)?;
            let latest: Option<String> = store
                .connection
                .query_row(
                    "SELECT request_id FROM syncs WHERE project_id=?1",
                    [project_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(Error::internal)?;
            if latest.as_deref() != Some(request_id) {
                return Ok(());
            }
            let mut snapshot = store.snapshot()?;
            let project = snapshot
                .projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| Error::invalid("Project missing"))?;
            match result {
                Ok(board) => {
                    project.name = board.title;
                    project.columns = board.columns;
                    project.fixture = false;
                    let g = project.github.as_mut().unwrap();
                    g.url = board.url;
                    g.last_synced_at = Some(now());
                    g.sync_error = None;
                    // Preserve local issue result metadata while replacing remote fields.
                    let mut issues = board.issues;
                    for issue in &mut issues {
                        if let Some(old) = snapshot.issues.iter().find(|i| {
                            i.project_id == project_id
                                && i.reference.provider == issue.reference.provider
                                && i.reference.repository == issue.reference.repository
                                && i.reference.number == issue.reference.number
                        }) {
                            issue.id = old.id.clone();
                            issue.result = old.result.clone();
                        }
                    }
                    // Keep referenced history, but no removed item belongs to a live board column.
                    let mut retained = Vec::new();
                    for old in snapshot.issues.iter().filter(|i| {
                        i.project_id == project_id && !issues.iter().any(|new| new.id == i.id)
                    }) {
                        let linked = snapshot.sessions.iter().any(|s| s.issue_id.as_deref() == Some(old.id.as_str()))
                            || snapshot.directors.iter().any(|d| snapshot.effective_profile(d).ok().is_some_and(|p| matches!(p.scope, DirectorScope::Issues { issue_ids } if issue_ids.contains(&old.id))));
                        if linked {
                            let mut historic = old.clone();
                            historic.column_id = "github-removed-from-board".into();
                            retained.push(historic);
                        }
                    }
                    issues.extend(retained);
                    snapshot.issues.retain(|i| i.project_id != project_id);
                    snapshot.issues.extend(issues);
                }
                Err(error) => {
                    project.github.as_mut().unwrap().sync_error = Some(error.to_string());
                }
            }
            snapshot.revision = snapshot
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::invalid("Revision exhausted"))?;
            store.save(&snapshot)?;
            self.snapshots.send_replace(snapshot);
            Ok(())
        };
        if let Err(error) = update() {
            eprintln!("Board state publication failed: {error}");
        }
    }
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
    router_with_config(path, token, defaults, RuntimeConfig::default())
}

pub fn router_with_config(
    path: impl AsRef<Path>,
    token: String,
    defaults: DirectorProfile,
    config: RuntimeConfig,
) -> Result<Router, Error> {
    router_with_shutdown(path, token, defaults, config).map(|(router, _)| router)
}

/// Explicit server-lifetime control. Dropping clients does not stop workers.
#[derive(Clone)]
pub struct Shutdown {
    workspace: Workspace,
}
impl Shutdown {
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.workspace.transport_shutdown.send_replace(true);
        {
            let mut store = self.workspace.store.lock().map_err(Error::internal)?;
            store.closing = true;
            for control in store.controls.values() {
                control.send_replace(true);
            }
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let unfinished: Vec<(String, String)> = {
                let store = self.workspace.store.lock().map_err(Error::internal)?;
                store
                    .snapshot()?
                    .sessions
                    .iter()
                    .filter(|s| {
                        s.worker
                            .as_ref()
                            .is_some_and(|w| runtime::active(&w.status))
                    })
                    .map(|s| {
                        Ok((
                            s.id.clone(),
                            store
                                .run_id(&s.id)?
                                .ok_or_else(|| Error::invalid("Active run receipt missing"))?,
                        ))
                    })
                    .collect::<Result<_, Error>>()?
            };
            if unfinished.is_empty()
                && self
                    .workspace
                    .transports
                    .load(std::sync::atomic::Ordering::SeqCst)
                    == 0
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                for (session, run) in unfinished {
                    self.workspace.interrupt_run(&session, &run)?;
                }
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}
pub fn router_with_shutdown(
    path: impl AsRef<Path>,
    token: String,
    defaults: DirectorProfile,
    config: RuntimeConfig,
) -> Result<(Router, Shutdown), Error> {
    if token.len() < 16 || token.trim() != token || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::invalid(
            "RELAY_TOKEN must contain at least 16 printable ASCII characters with no spaces",
        ));
    }
    let mut store = Store::open(path.as_ref(), defaults.clone())?;
    let mut initial = store.snapshot()?;
    let mut changed = false;
    // Reap only a previously owned Linux process whose boot/start identity still matches.
    // A reused PID must never authorize signalling an unrelated process.
    {
        let mut statement = store
            .connection
            .prepare("SELECT session_id,pid,identity FROM processes")
            .map_err(Error::internal)?;
        let records = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::internal)?;
        for record in records {
            let (session, pid, identity) = record.map_err(Error::internal)?;
            if initial
                .sessions
                .iter()
                .find(|s| s.id == session)
                .and_then(|s| s.worker.as_ref())
                .is_some_and(|w| runtime::active(&w.status))
            {
                runtime::reap_owned(pid, &identity);
            }
        }
    }
    store
        .connection
        .execute("DELETE FROM processes", [])
        .map_err(Error::internal)?;
    for session in &mut initial.sessions {
        if let Some(w) = &mut session.worker
            && runtime::active(&w.status)
        {
            w.status = WorkerStatus::Interrupted;
            w.error =
                Some("Server restarted before the turn ended; continuation is explicit".into());
            changed = true;
        }
    }
    if let Some(remote) = &config.remote {
        let selected = if let Some(index) = initial.projects.iter().position(|project| {
            project.id != "demo" && runtime::configured_project(project, &config)
        }) {
            index
        } else {
            let id = format!(
                "github-project:{}:{}:{}",
                remote.owner, remote.number, remote.repository
            );
            if initial.projects.iter().any(|project| project.id == id) {
                return Err(Error::invalid(
                    "Configured live project ID collides with historical project metadata",
                ));
            }
            initial.projects.push(Project {
                id: id.clone(),
                name: format!("{} · GitHub project {}", remote.repository, remote.number),
                repository: remote.repository.clone(),
                fixture: false,
                columns: vec![BoardColumn {
                    id: "github-no-status".into(),
                    title: "No status".into(),
                }],
                defaults,
                github: Some(GitHubProject {
                    owner: remote.owner.clone(),
                    number: remote.number,
                    url: format!(
                        "https://github.com/users/{}/projects/{}",
                        remote.owner, remote.number
                    ),
                    last_synced_at: None,
                    sync_error: None,
                }),
            });
            initial.directors.push(Director {
                id: format!("director-{id}"),
                project_id: id,
                name: "Project director".into(),
                overrides: ProfileOverrides::default(),
            });
            changed = true;
            initial.projects.len() - 1
        };
        // The current configured live project comes first; all other identities remain intact.
        if selected != 0 {
            let project = initial.projects.remove(selected);
            initial.projects.insert(0, project);
            changed = true;
        }
    }
    if changed {
        initial.revision = initial
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&initial)?;
    }
    let (snapshots, _) = watch::channel(initial);
    let workspace = Workspace {
        transport_shutdown: watch::channel(false).0,
        transports: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        store: Arc::new(Mutex::new(store)),
        snapshots,
        token: token.into(),
        config: Arc::new(config),
    };
    let shutdown = Shutdown {
        workspace: workspace.clone(),
    };
    Ok((
        Router::new()
            .route("/v1/snapshot", get(snapshot))
            .route("/v1/commands", post(command))
            .route("/v1/events", get(events))
            .layer(DefaultBodyLimit::max(64 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                workspace.clone(),
                authorize,
            ))
            .with_state(workspace),
        shutdown,
    ))
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
    let handle = tokio::runtime::Handle::current();
    let result = tokio::task::spawn_blocking(move || {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if store.closing {
            return Err(Error::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "shutdown",
                "Server is shutting down",
            ));
        }
        let run_id = envelope.request_id.clone();
        let (snapshot, action) = store.apply(envelope, &workspace.config)?;
        workspace.snapshots.send_replace(snapshot.clone());
        match action {
            Some(Action::Run { session_id, prompt }) => {
                let (sender, receiver) = watch::channel(false);
                store.controls.insert(session_id.clone(), sender);
                handle.spawn(runtime::run(
                    workspace.clone(),
                    session_id,
                    run_id,
                    prompt,
                    receiver,
                ));
            }
            Some(Action::Stop(session_id)) => {
                store.controls.get(&session_id).unwrap().send_replace(true);
            }
            Some(Action::Sync(project_id)) => {
                let workspace = workspace.clone();
                handle.spawn_blocking(move || workspace.synchronize(&project_id, &run_id));
            }
            None => {}
        }
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
    let shutdown = workspace.transport_shutdown.subscribe();
    let store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing || *shutdown.borrow() {
        return Err(Error::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "shutdown",
            "Server is shutting down",
        ));
    }
    let transport = Transport(workspace.transports.clone());
    transport
        .0
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    drop(store);
    Ok(upgrade
        .max_message_size(64 * 1024)
        .on_upgrade(move |socket| async move {
            let _transport = transport;
            stream(socket, receiver, shutdown).await;
        }))
}

struct Transport(Arc<std::sync::atomic::AtomicUsize>);
impl Drop for Transport {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn stream(
    mut socket: WebSocket,
    mut receiver: watch::Receiver<Snapshot>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::select! {
        biased;
        _ = async {
            while !*shutdown.borrow_and_update() {
                if shutdown.changed().await.is_err() { break; }
            }
        } => {},
        _ = stream_snapshots(&mut socket, &mut receiver) => return,
    }
    // A stalled client must not hold shutdown open indefinitely.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        socket.send(WsMessage::Close(None)),
    )
    .await;
}

async fn stream_snapshots(socket: &mut WebSocket, receiver: &mut watch::Receiver<Snapshot>) {
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

#[cfg(test)]
mod tests;
