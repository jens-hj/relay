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
mod app_server;
mod browser;
pub use browser::{BrowserConfig, initialize_setup_code};
mod claude;
mod conversation;
mod github;
mod harness;
mod process;
mod projects;
mod providers;
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
    defaults: DirectorProfile,
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
        if version > 8 {
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
             CREATE TABLE IF NOT EXISTS drafts(session_id TEXT PRIMARY KEY, draft TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS draft_receipts(request_id TEXT PRIMARY KEY, request TEXT NOT NULL, response TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS assets(id TEXT PRIMARY KEY, metadata TEXT NOT NULL, bytes BLOB NOT NULL);
             CREATE TABLE IF NOT EXISTS browser_owner(id INTEGER PRIMARY KEY CHECK(id=1), username TEXT NOT NULL, password TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS browser_sessions(session_hash TEXT PRIMARY KEY, csrf TEXT NOT NULL, created INTEGER NOT NULL, seen INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS browser_recovery(code_hash TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS browser_setup(code_hash TEXT PRIMARY KEY, expires INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS browser_state(id INTEGER PRIMARY KEY CHECK(id=1), token_hash TEXT NOT NULL);
             PRAGMA user_version = 8;"
        ).map_err(Error::internal)?;
        let seed =
            serde_json::to_string(&demo_snapshot(defaults.clone())).map_err(Error::internal)?;
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO workspace(id, snapshot) VALUES (1, ?1)",
                [&seed],
            )
            .map_err(Error::internal)?;
        if version < 4 && inserted == 0 {
            let json: String = connection
                .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
                    r.get(0)
                })
                .map_err(Error::internal)?;
            let mut snapshot: Snapshot = serde_json::from_str(&json).map_err(Error::internal)?;
            let mut old = DirectorProfile::default();
            old.permissions.insert(Task::Implement, Permission::Ask);
            for project in &mut snapshot.projects {
                if project.defaults == old {
                    project
                        .defaults
                        .permissions
                        .insert(Task::Implement, Permission::Allow);
                }
            }
            connection
                .execute(
                    "UPDATE workspace SET snapshot=?1 WHERE id=1",
                    [serde_json::to_string(&snapshot).map_err(Error::internal)?],
                )
                .map_err(Error::internal)?;
        }
        if version < 5 {
            let json: String = connection
                .query_row("SELECT snapshot FROM workspace WHERE id=1", [], |r| {
                    r.get(0)
                })
                .map_err(Error::internal)?;
            let mut snapshot: Snapshot = serde_json::from_str(&json).map_err(Error::internal)?;
            snapshot.migrate_projects();
            connection
                .execute(
                    "UPDATE workspace SET snapshot=?1 WHERE id=1",
                    [serde_json::to_string(&snapshot).map_err(Error::internal)?],
                )
                .map_err(Error::internal)?;
        }
        connection
            .execute_batch("COMMIT;")
            .map_err(Error::internal)?;
        Ok(Self {
            defaults,
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
        if snapshot.revision != envelope.expected_revision
            && !matches!(envelope.command, Command::SubmitTurn { .. })
        {
            return Err(Error::new(
                StatusCode::CONFLICT,
                "conflict",
                "Workspace changed. Refresh and review your draft before saving again.",
            ));
        }
        let mut action = None;
        match envelope.command {
            command @ Command::StartDirector { .. } => {
                let Command::StartDirector {
                    ref parts,
                    ref prompt,
                    approve_implementation,
                    ..
                } = command
                else {
                    unreachable!()
                };
                let parts = parts.clone();
                if !parts.is_empty() && (!has_content(&parts) || plain_text(&parts) != *prompt) {
                    return Err(Error::invalid(
                        "First message content does not match its prompt",
                    ));
                }
                action = projects::apply(
                    &mut snapshot,
                    command,
                    &envelope.request_id,
                    self.defaults.clone(),
                    config,
                )?;
                if !parts.is_empty() {
                    let session_id = format!("session-{}", envelope.request_id);
                    conversation::validate(&transaction, &snapshot, &session_id, &parts)?;
                    snapshot
                        .messages
                        .iter_mut()
                        .find(|m| m.id == format!("prompt-{}", envelope.request_id))
                        .unwrap()
                        .parts = parts.clone();
                    snapshot.submissions.push(Submission {
                        id: envelope.request_id.clone(),
                        session_id,
                        parts,
                        state: SubmissionState::Launching,
                        approve_implementation,
                        error: None,
                        interrupts_run: None,
                        last_edit_request: None,
                    });
                }
            }
            command if projects::handles(&command) => {
                action = projects::apply(
                    &mut snapshot,
                    command,
                    &envelope.request_id,
                    self.defaults.clone(),
                    config,
                )?;
            }
            Command::ConfigureProject { binding } => {
                let id = harness::add_project(&mut snapshot, binding, self.defaults.clone())?;
                snapshot.migrate_projects();
                action = Some(Action::Sync(id));
            }
            Command::ConfigureHarness {
                harness,
                executable,
            } => {
                if executable.trim().is_empty()
                    || executable.len() > 4096
                    || executable.contains(['\0', '\n', '\r'])
                {
                    return Err(Error::invalid("Enter an executable path or command name"));
                }
                snapshot.installations.retain(|i| i.harness != harness);
                snapshot.installations.push(HarnessInstallation {
                    harness,
                    executable,
                });
                snapshot.revision += 1;
            }
            Command::SetWorkerExecution {
                session_id,
                execution,
            } => {
                let worker = snapshot
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == session_id && !s.fixture)
                    .and_then(|s| s.worker.as_mut())
                    .ok_or_else(|| Error::invalid("Live worker not found"))?;
                worker.execution = execution;
                snapshot.revision += 1;
            }
            Command::RespondPermission {
                permission_id,
                run_id,
                allow,
            } => {
                let permission = snapshot
                    .tool_permissions
                    .iter_mut()
                    .find(|p| {
                        p.id == permission_id
                            && p.run_id == run_id
                            && p.decision.is_none()
                            && !p.expired
                    })
                    .ok_or_else(|| Error::invalid("Permission request is no longer pending"))?;
                let active = snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == permission.session_id)
                    .and_then(|s| s.worker.as_ref())
                    .is_some_and(|w| runtime::active(&w.status));
                let current: Option<String> = transaction
                    .query_row(
                        "SELECT run_id FROM runs WHERE session_id=?1",
                        [&permission.session_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(Error::internal)?;
                if !active || current.as_deref() != Some(run_id.as_str()) {
                    return Err(Error::invalid("Permission run is no longer active"));
                }
                permission.decision = Some(allow);
                snapshot.revision += 1;
            }
            command @ (Command::SubmitTurn { .. }
            | Command::PromoteTurn { .. }
            | Command::CancelTurn { .. }
            | Command::EditQueuedTurn { .. }
            | Command::ResumeQueue { .. }) => {
                action = conversation::apply(
                    &transaction,
                    &mut snapshot,
                    &envelope.request_id,
                    command,
                    config,
                    &self.controls,
                )?;
            }
            Command::SyncProject { project_id } => {
                let project = snapshot.project(&project_id).map_err(Error::invalid)?;
                if project.github.is_none() {
                    return Err(Error::invalid("Project has no configured GitHub board"));
                }
                if !runtime::configured_project(
                    project,
                    &harness::configuration(&snapshot, &project_id, config),
                ) {
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
                let chosen_harness = snapshot
                    .effective_profile(
                        snapshot
                            .directors
                            .iter()
                            .find(|d| d.id == director_id)
                            .unwrap(),
                    )
                    .map_err(Error::invalid)?
                    .harness;
                let session_id = format!("session-{}", envelope.request_id);
                snapshot.sessions.push(Session {
                    workspaces: vec![],
                    connection_ids: snapshot
                        .connections
                        .iter()
                        .filter(|c| {
                            c.project_id == issue.project_id
                                && c.enabled
                                && c.state == ConnectionState::Ready
                                && !matches!(c.kind, ConnectionKind::Board { .. })
                        })
                        .map(|c| c.id.clone())
                        .collect(),
                    id: session_id.clone(),
                    project_id: issue.project_id.clone(),
                    issue_id: Some(issue_id),
                    director_id,
                    title: issue.title.clone(),
                    role: SessionRole::Worker,
                    fixture: false,
                    worker: Some(WorkerRun {
                        model: None,
                        context_tokens: None,
                        context_window: None,
                        last_usage: None,
                        harness: chosen_harness,
                        execution: None,
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
                runtime::authorize_session(
                    &snapshot,
                    session
                        .issue_id
                        .as_deref()
                        .ok_or_else(|| Error::invalid("Session has no issue"))?,
                    &session.director_id,
                    approve_implementation,
                    &session.role,
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
                if worker.usage.is_some() {
                    worker.last_usage = worker.usage.clone();
                }
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
                conversation::pause(
                    &mut snapshot,
                    &session_id,
                    "Queue paused after stopping the agent",
                );
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
            let identity = snapshot
                .submissions
                .iter()
                .find(|s| &s.session_id == session_id && s.state == SubmissionState::Launching)
                .map(|s| &s.id)
                .unwrap_or(&envelope.request_id);
            transaction.execute("INSERT INTO runs(session_id,run_id) VALUES(?1,?2) ON CONFLICT(session_id) DO UPDATE SET run_id=excluded.run_id",params![session_id,identity]).map_err(Error::internal)?;
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
    browser: Arc<browser::BrowserAuth>,
    harness_status: Arc<Mutex<Vec<HarnessStatus>>>,
    harness_probe: Arc<tokio::sync::Mutex<()>>,
    drafts: watch::Sender<Vec<Draft>>,
    transport_shutdown: watch::Sender<bool>,
    transports: Arc<std::sync::atomic::AtomicUsize>,
    project_jobs: Arc<std::sync::atomic::AtomicUsize>,
    store: Arc<Mutex<Store>>,
    snapshots: watch::Sender<Snapshot>,
    token: Arc<str>,
    config: Arc<RuntimeConfig>,
}

struct ProjectJob {
    count: Arc<std::sync::atomic::AtomicUsize>,
}
impl ProjectJob {
    fn new(workspace: &Workspace) -> Self {
        workspace
            .project_jobs
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self {
            count: workspace.project_jobs.clone(),
        }
    }
}
impl Drop for ProjectJob {
    fn drop(&mut self) {
        self.count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
enum Action {
    Sync(String),
    Operations(Vec<String>),
    Run { session_id: String, prompt: String },
    Stop(String),
    Promote(String),
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
        parts: vec![],
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
        if let Some(submission) = snapshot.submissions.iter_mut().find(|s| s.id == run) {
            submission.state = SubmissionState::Interrupted;
            submission.error = Some("Execution task interrupted; this turn is not replayed".into());
        }
        conversation::pause(
            &mut snapshot,
            session,
            "Execution interrupted; review and resume explicitly",
        );
        harness::expire(&mut snapshot, session, run);
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
        if snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| !runtime::active(&w.status))
        {
            store.controls.remove(session);
            harness::expire(&mut snapshot, session, run);
        }
        store.save(&snapshot)?;
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
    fn synchronize(&self, project_id: &str, request_id: &str) {
        let config = harness::configuration(&self.snapshots.borrow(), project_id, &self.config);
        let result = github::sync(&config, project_id);
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
                        if let Some(old) = snapshot
                            .issues
                            .iter()
                            .find(|i| i.project_id == project_id && i.reference == issue.reference)
                        {
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
            if let Some(project) = snapshot.projects.iter().find(|p| p.id == project_id)
                && let Some(board) = snapshot
                    .boards
                    .iter_mut()
                    .find(|b| b.id == format!("board-{project_id}"))
            {
                board.columns = project.columns.clone();
                if let Some(g) = &project.github {
                    board.name = project.name.clone();
                    board.last_synced_at = g.last_synced_at;
                    board.error = g.sync_error.clone();
                    board.source = BoardSource::Github {
                        owner: g.owner.clone(),
                        number: g.number,
                        url: g.url.clone(),
                    };
                }
                if board.error.is_none() {
                    let board_id = board.id.clone();
                    snapshot.memberships.retain(|m| m.board_id != board_id);
                    for issue in snapshot.issues.iter().filter(|i| {
                        i.project_id == project_id
                            && board.columns.iter().any(|c| c.id == i.column_id)
                    }) {
                        snapshot.memberships.push(BoardMembership {
                            board_id: board_id.clone(),
                            issue_id: issue.id.clone(),
                            column_ids: vec![issue.column_id.clone()],
                            remote_item_id: None,
                        });
                    }
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
    /// Commit complete operation state and publish it under the single writer.
    fn update_project(
        &self,
        update: impl FnOnce(&mut Snapshot) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(Error::internal)?;
        if store.closing {
            return Err(Error::invalid("Server is shutting down"));
        }
        let mut snapshot = store.snapshot()?;
        update(&mut snapshot)?;
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&snapshot)?;
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
    fn authorize(&self, headers: &HeaderMap) -> Result<(), Error> {
        if !headers.contains_key("authorization") {
            return self.browser_session(headers, false).map(|_| ());
        }
        if headers.get_all("authorization").iter().count() != 1 {
            return Err(Error::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication required",
            ));
        }
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
            let mut snapshot = store.snapshot()?;
            let changed = projects::interrupt_operations(
                &mut snapshot,
                "Server stopped the operation; inspect results before continuing",
            );
            if changed {
                snapshot.revision += 1;
                store.save(&snapshot)?;
                self.workspace.snapshots.send_replace(snapshot);
            }
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
                    .project_jobs
                    .load(std::sync::atomic::Ordering::SeqCst)
                    == 0
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
    router_with_browser(path, token, defaults, config, BrowserConfig::default())
}

pub fn router_with_browser(
    path: impl AsRef<Path>,
    token: String,
    defaults: DirectorProfile,
    config: RuntimeConfig,
    browser_config: BrowserConfig,
) -> Result<(Router, Shutdown), Error> {
    if token.len() < 16 || token.trim() != token || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::invalid(
            "RELAY_TOKEN must contain at least 16 printable ASCII characters with no spaces",
        ));
    }
    let mut store = Store::open(path.as_ref(), defaults.clone())?;
    let browser = browser::BrowserAuth::initialize(&mut store, &token, browser_config)?;
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
    for submission in &mut initial.submissions {
        if matches!(
            submission.state,
            SubmissionState::Queued | SubmissionState::Launching | SubmissionState::Running
        ) {
            submission.state = if submission.state == SubmissionState::Queued {
                SubmissionState::Paused
            } else {
                SubmissionState::Interrupted
            };
            submission.error = Some("Server restarted; delivered turns are never replayed, queued turns require explicit resume".into());
            submission.interrupts_run = None;
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
                root: None,
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
    if let (Some(remote), Some(checkout)) = (&config.remote, &config.repository) {
        let binding = ProjectBinding {
            repository: remote.repository.clone(),
            owner: remote.owner.clone(),
            number: remote.number,
            checkout: checkout.display().to_string(),
        };
        if !initial.bindings.iter().any(|b| b == &binding) {
            initial
                .bindings
                .retain(|b| b.project_id() != binding.project_id());
            initial.bindings.push(binding);
            changed = true;
        }
    }
    for permission in &mut initial.tool_permissions {
        if permission.decision.is_none() && !permission.expired {
            permission.expired = true;
            changed = true;
        }
    }
    let before_projects = initial.clone();
    initial.migrate_projects();
    // A prior schema migration may have created boards before environment bindings exist.
    for project in &mut initial.projects {
        if project.fixture {
            continue;
        }
        if let Some(binding) = initial.bindings.iter().find(|b| {
            b.repository == project.repository
                && project
                    .github
                    .as_ref()
                    .is_some_and(|g| g.owner == b.owner && g.number == b.number)
        }) && !initial.connections.iter().any(|c| {
            c.project_id == project.id && matches!(c.kind, ConnectionKind::Repository { .. })
        }) {
            project.root = Path::new(&binding.checkout)
                .parent()
                .map(|p| p.display().to_string());
            initial.connections.push(ProjectConnection {
                id: format!("repository-{}", project.id),
                project_id: project.id.clone(),
                name: binding.repository.clone(),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Repository {
                    remote: format!("git@github.com:{}.git", binding.repository),
                    checkout: Some(binding.checkout.clone()),
                    owned: false,
                },
            });
        }
    }
    projects::interrupt_operations(
        &mut initial,
        "Server restarted; inspect results before continuing",
    );
    changed |= initial != before_projects;
    if changed {
        initial.revision = initial
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&initial)?;
    }
    let (snapshots, _) = watch::channel(initial);
    let workspace = Workspace {
        browser,
        harness_status: Arc::new(Mutex::new(vec![])),
        harness_probe: Arc::new(tokio::sync::Mutex::new(())),
        drafts: watch::channel(conversation::read_drafts(&store.connection)?).0,
        transport_shutdown: watch::channel(false).0,
        transports: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        project_jobs: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
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
            .route("/v1/boards/discover", post(discover_board))
            .route("/v1/operations/reconcile", post(reconcile_lookup))
            .route("/v1/harnesses", get(harness::statuses))
            .route("/v1/harnesses/refresh", post(harness::refresh))
            .route("/v1/snapshot", get(snapshot))
            .route("/v1/commands", post(command))
            .route(
                "/v1/conversation/commands",
                post(command).layer(DefaultBodyLimit::max(512 * 1024)),
            )
            .route("/v1/events", get(events))
            .route("/v1/drafts", get(conversation::drafts))
            .route("/v1/drafts/events", get(conversation::draft_events))
            .route(
                "/v1/drafts/{session}",
                post(conversation::save_draft).layer(DefaultBodyLimit::max(512 * 1024)),
            )
            .route(
                "/v1/assets/{id}",
                get(conversation::get_asset)
                    .post(conversation::upload_asset)
                    .layer(DefaultBodyLimit::max(ASSET_LIMIT)),
            )
            .layer(axum::middleware::from_fn_with_state(
                workspace.clone(),
                protocol,
            ))
            .layer(DefaultBodyLimit::max(64 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                workspace.clone(),
                authorize,
            ))
            .merge(browser::routes(workspace.clone()))
            .layer(axum::middleware::from_fn(browser::response_headers))
            .with_state(workspace),
        shutdown,
    ))
}

async fn authorize(
    State(workspace): State<Workspace>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, Error> {
    workspace.browser_request(
        request.headers(),
        request.method(),
        matches!(request.uri().path(), "/v1/events" | "/v1/drafts/events"),
    )?;
    Ok(next.run(request).await)
}

async fn snapshot(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
) -> Result<Json<Snapshot>, Error> {
    workspace.authorize(&headers)?;
    Ok(Json(workspace.snapshots.borrow().clone()))
}

async fn protocol(
    State(workspace): State<Workspace>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) {
        if let Err(error) = workspace.authorize(request.headers()) {
            return error.into_response();
        }
        if request
            .headers()
            .get("x-relay-protocol")
            .and_then(|v| v.to_str().ok())
            != Some("2")
        {
            return Error::new(
                StatusCode::CONFLICT,
                "protocol_mismatch",
                "This server requires Relay protocol 2; update and restart the desktop client",
            )
            .into_response();
        }
    }
    next.run(request).await
}

async fn discover_board(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    Json(source): Json<BoardSource>,
) -> Result<Json<BoardDiscovery>, Error> {
    workspace.authorize(&headers)?;
    let job = ProjectJob::new(&workspace);
    tokio::task::spawn_blocking(move || {
        let _job = job;
        process::with_cancellation(workspace.transport_shutdown.subscribe(), || {
            providers::discover(&workspace.config, &source)
        })
    })
    .await
    .map_err(Error::internal)?
    .map(|b| {
        Json(BoardDiscovery {
            source: b.source,
            name: b.name,
            columns: b.columns,
        })
    })
}

async fn reconcile_lookup(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    Json(input): Json<ReconciliationInput>,
) -> Result<Json<ReconciliationResult>, Error> {
    workspace.authorize(&headers)?;
    if input.url.len() > 4096 || input.operation_id.len() > 256 {
        return Err(Error::invalid("Provider result URL is too long"));
    }
    let job = ProjectJob::new(&workspace);
    tokio::task::spawn_blocking(move || {
        let _job = job;
        let snapshot = workspace.snapshots.borrow().clone();
        process::with_cancellation(workspace.transport_shutdown.subscribe(), || {
            providers::lookup_reconciliation(
                &workspace.config,
                &snapshot,
                &input.operation_id,
                &input.url,
            )
        })
    })
    .await
    .map_err(Error::internal)?
    .map(|(key, result, description)| {
        Json(ReconciliationResult {
            key,
            result,
            description,
        })
    })
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
        let reset_status = matches!(envelope.command, Command::ConfigureHarness { .. });
        let (snapshot, action) = store.apply(envelope, &workspace.config)?;
        if reset_status {
            workspace
                .harness_status
                .lock()
                .map_err(Error::internal)?
                .clear();
        }
        workspace
            .drafts
            .send_replace(conversation::read_drafts(&store.connection)?);
        workspace.snapshots.send_replace(snapshot.clone());
        match action {
            Some(Action::Run { session_id, prompt }) => {
                let run_id = store
                    .run_id(&session_id)?
                    .ok_or_else(|| Error::invalid("Run identity missing"))?;
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
            Some(Action::Promote(session_id)) => {
                if let Some(control) = store.controls.get(&session_id) {
                    control.send_replace(true);
                }
            }
            Some(Action::Operations(ids)) => {
                for id in ids {
                    let workspace = workspace.clone();
                    let job = ProjectJob::new(&workspace);
                    handle.spawn_blocking(move || {
                        let _job = job;
                        projects::run(workspace, id)
                    });
                }
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
            stream(socket, receiver, shutdown, workspace, headers).await;
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
    workspace: Workspace,
    headers: HeaderMap,
) {
    tokio::select! {
        biased;
        _ = async {
            while !*shutdown.borrow_and_update() {
                if shutdown.changed().await.is_err() { break; }
            }
        } => {},
        _ = workspace.session_ended(&headers) => {},
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
