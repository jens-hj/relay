use super::*;
use axum::body::Bytes;
use axum::extract::Path as RoutePath;
use rusqlite::Transaction;

pub(super) fn read_drafts(connection: &Connection) -> Result<Vec<Draft>, Error> {
    let mut query = connection
        .prepare("SELECT draft FROM drafts ORDER BY session_id")
        .map_err(Error::internal)?;
    query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(Error::internal)?
        .map(|row| serde_json::from_str(&row.map_err(Error::internal)?).map_err(Error::internal))
        .collect()
}

fn draft(connection: &Connection, session: &str) -> Result<Draft, Error> {
    let value: Option<String> = connection
        .query_row(
            "SELECT draft FROM drafts WHERE session_id=?1",
            [session],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::internal)?;
    value
        .map(|value| serde_json::from_str(&value).map_err(Error::internal))
        .unwrap_or_else(|| {
            Ok(Draft {
                session_id: session.into(),
                ..Default::default()
            })
        })
}

fn store_draft(connection: &Connection, value: &Draft) -> Result<(), Error> {
    connection.execute("INSERT INTO drafts(session_id,draft) VALUES(?1,?2) ON CONFLICT(session_id) DO UPDATE SET draft=excluded.draft", params![value.session_id, serde_json::to_string(value).map_err(Error::internal)?]).map_err(Error::internal)?;
    Ok(())
}

pub(super) fn validate(
    connection: &Connection,
    snapshot: &Snapshot,
    session: &str,
    parts: &[Part],
) -> Result<(), Error> {
    if !snapshot.sessions.iter().any(|s| s.id == session) {
        return Err(Error::invalid("Session not found"));
    }
    validate_parts(snapshot, session, parts).map_err(Error::invalid)?;
    for asset in assets(parts) {
        let value: Option<String> = connection
            .query_row(
                "SELECT metadata FROM assets WHERE id=?1",
                [&asset.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::internal)?;
        let stored: Asset = serde_json::from_str(
            &value.ok_or_else(|| Error::invalid("File upload is not complete"))?,
        )
        .map_err(Error::internal)?;
        if stored != *asset {
            return Err(Error::invalid(
                "File metadata does not match its uploaded content",
            ));
        }
    }
    Ok(())
}

pub(super) async fn drafts(State(workspace): State<Workspace>) -> Result<Json<Vec<Draft>>, Error> {
    Ok(Json(workspace.drafts.borrow().clone()))
}

pub(super) async fn save_draft(
    State(workspace): State<Workspace>,
    RoutePath(session): RoutePath<String>,
    Json(request): Json<SaveDraft>,
) -> Result<Json<Draft>, Error> {
    uuid::Uuid::parse_str(&request.request_id)
        .map_err(|_| Error::invalid("Draft request ID must be a UUID"))?;
    let request_json = serde_json::to_string(&(&session, &request)).map_err(Error::internal)?;
    // Waiting for the store and loading the snapshot must not occupy an async worker.
    blocking(|| persist_draft(&workspace, session, request, request_json)).map(Json)
}

fn persist_draft(
    workspace: &Workspace,
    session: String,
    request: SaveDraft,
    request_json: String,
) -> Result<Draft, Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing {
        return Err(Error::invalid("Server is shutting down"));
    }
    let snapshot = store.snapshot()?;
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(Error::internal)?;
    let receipt: Option<(String, String)> = transaction
        .query_row(
            "SELECT request,response FROM draft_receipts WHERE request_id=?1",
            [&request.request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Error::internal)?;
    if let Some((original, response)) = receipt {
        if original != request_json {
            return Err(Error::invalid(
                "Draft request ID was reused with different content",
            ));
        }
        return serde_json::from_str(&response).map_err(Error::internal);
    }
    validate(&transaction, &snapshot, &session, &request.parts)?;
    let previous = draft(&transaction, &session)?;
    if previous.revision != request.expected_revision {
        return Err(Error::new(
            StatusCode::CONFLICT,
            "draft_conflict",
            "Shared draft changed on another client; your local draft is retained",
        ));
    }
    let next = Draft {
        session_id: session,
        revision: previous
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Draft revision exhausted"))?,
        parts: request.parts,
    };
    store_draft(&transaction, &next)?;
    transaction
        .execute(
            "INSERT INTO draft_receipts(request_id,request,response) VALUES(?1,?2,?3)",
            params![
                request.request_id,
                request_json,
                serde_json::to_string(&next).map_err(Error::internal)?
            ],
        )
        .map_err(Error::internal)?;
    transaction.commit().map_err(Error::internal)?;
    workspace
        .drafts
        .send_replace(read_drafts(&store.connection)?);
    Ok(next)
}

pub(super) async fn draft_events(
    State(workspace): State<Workspace>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, Error> {
    let store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing {
        return Err(Error::invalid("Server is shutting down"));
    }
    let transport = Transport(workspace.transports.clone());
    transport
        .0
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut drafts = workspace.drafts.subscribe();
    let mut shutdown = workspace.transport_shutdown.subscribe();
    drop(store);
    Ok(upgrade.on_upgrade(move |mut socket| async move {
        let _transport = transport;
        let streaming = async {
            loop {
                let value = drafts.borrow_and_update().clone();
                let Ok(json) = serde_json::to_string(&value) else { return; };
                if socket.send(WsMessage::Text(json.into())).await.is_err() { return; }
                loop { tokio::select! {
                    changed = drafts.changed() => { if changed.is_err() { return; } break; },
                    incoming = socket.recv() => match incoming {
                        Some(Ok(WsMessage::Ping(value))) => { if socket.send(WsMessage::Pong(value)).await.is_err() { return; } },
                        Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => return,
                        _ => {},
                    }
                } }
            }
        };
        tokio::select! { _ = workspace.session_ended(&headers) => {}, _ = streaming => {}, _ = async { while !*shutdown.borrow_and_update() { if shutdown.changed().await.is_err() { break; } } } => {} }
        let _ = tokio::time::timeout(std::time::Duration::from_millis(250), socket.send(WsMessage::Close(None))).await;
    }))
}

pub(super) async fn upload_asset(
    State(workspace): State<Workspace>,
    RoutePath(id): RoutePath<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<Asset>, Error> {
    uuid::Uuid::parse_str(&id).map_err(|_| Error::invalid("File ID must be a UUID"))?;
    let name = headers
        .get("x-relay-filename")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("file");
    let name = percent_encoding::percent_decode_str(name)
        .decode_utf8()
        .map_err(|_| Error::invalid("Invalid file name encoding"))?;
    if name.len() > 240
        || name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(Error::invalid("Invalid file name"));
    }
    let media_type = headers
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("application/octet-stream");
    if media_type.len() > 128 || !media_type.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::invalid("Invalid media type"));
    }
    if bytes.len() > ASSET_LIMIT {
        return Err(Error::invalid("File exceeds 20 MiB"));
    }
    let bytes = bytes.to_vec();
    let (name, media_type) = (name.into_owned(), media_type.to_owned());
    let value = tokio::task::spawn_blocking(move || {
        if media_type.starts_with("image/") {
            let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(Error::internal)?;
            let (width, height) = reader
                .into_dimensions()
                .map_err(|_| Error::invalid("Unsupported or invalid image"))?;
            if u64::from(width) * u64::from(height) > 32_000_000 {
                return Err(Error::invalid("Image exceeds 32 megapixels"));
            }
            image::load_from_memory(&bytes)
                .map_err(|_| Error::invalid("Unsupported or invalid image"))?;
        }
        let asset = Asset {
            id,
            name,
            media_type,
            size: bytes.len() as u64,
        };
        let store = workspace.store.lock().map_err(Error::internal)?;
        let existing: Option<(String, Vec<u8>)> = store
            .connection
            .query_row(
                "SELECT metadata,bytes FROM assets WHERE id=?1",
                [&asset.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Error::internal)?;
        let metadata = serde_json::to_string(&asset).map_err(Error::internal)?;
        if let Some((old_metadata, old_bytes)) = existing {
            if old_metadata != metadata || old_bytes != bytes {
                return Err(Error::invalid("File ID already has different content"));
            }
        } else {
            if store.closing {
                return Err(Error::invalid("Server is shutting down"));
            }
            store
                .connection
                .execute(
                    "INSERT INTO assets(id,metadata,bytes) VALUES(?1,?2,?3)",
                    params![asset.id, metadata, bytes],
                )
                .map_err(Error::internal)?;
        }
        Ok::<_, Error>(asset)
    })
    .await
    .map_err(Error::internal)??;
    Ok(Json(value))
}

pub(super) async fn get_asset(
    State(workspace): State<Workspace>,
    RoutePath(id): RoutePath<String>,
) -> Result<Response, Error> {
    let store = workspace.store.lock().map_err(Error::internal)?;
    let (metadata, bytes): (String, Vec<u8>) = store
        .connection
        .query_row(
            "SELECT metadata,bytes FROM assets WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Error::internal)?
        .ok_or_else(|| Error::invalid("File not found"))?;
    let metadata: Asset = serde_json::from_str(&metadata).map_err(Error::internal)?;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        "content-type",
        metadata
            .media_type
            .parse()
            .map_err(|_| Error::invalid("Invalid media type"))?,
    );
    response.headers_mut().insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    if !matches!(
        metadata.media_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "text/plain"
    ) {
        response.headers_mut().insert(
            "content-disposition",
            axum::http::HeaderValue::from_static("attachment"),
        );
        response.headers_mut().insert(
            "content-type",
            axum::http::HeaderValue::from_static("application/octet-stream"),
        );
    }
    Ok(response)
}

pub(super) fn pause(snapshot: &mut Snapshot, session: &str, reason: &str) {
    for submission in &mut snapshot.submissions {
        if submission.session_id == session && submission.state == SubmissionState::Queued {
            submission.state = SubmissionState::Paused;
            submission.error = Some(reason.into());
            submission.interrupts_run = None;
        }
    }
}

/// Policy for accepting a turn, and whether its shared workspaces are free to
/// launch it now. Contention never rejects a turn: the caller queues it and
/// waits for the workspace, as for any other sender, behind earlier waiters.
pub(super) fn authorize(
    connection: &Connection,
    snapshot: &Snapshot,
    session: &str,
    approved: bool,
    config: &RuntimeConfig,
) -> Result<bool, Error> {
    authorize_policy(snapshot, session, approved, config)?;
    runtime::launch_available(snapshot, session, &waiting_sessions(connection)?)
}

/// Sessions whose queues wait for a shared workspace.
pub(super) fn waiting_sessions(
    connection: &Connection,
) -> Result<std::collections::HashSet<String>, Error> {
    let mut query = connection
        .prepare("SELECT session_id FROM workspace_waits")
        .map_err(Error::internal)?;
    query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(Error::internal)?
        .collect::<Result<_, _>>()
        .map_err(Error::internal)
}

fn authorize_policy(
    snapshot: &Snapshot,
    session: &str,
    approved: bool,
    config: &RuntimeConfig,
) -> Result<(), Error> {
    let mut candidate = snapshot.clone();
    let index = candidate
        .sessions
        .iter()
        .position(|s| s.id == session)
        .ok_or_else(|| Error::invalid("Session not found"))?;
    let session = candidate.sessions.remove(index);
    if session.archived {
        return Err(Error::invalid("Unarchive this chat before continuing"));
    }
    if session.fixture || session.worker.is_none() {
        return Err(Error::invalid("This session cannot run agent turns"));
    }
    if !snapshot.can_continue(&session) {
        return Err(Error::invalid(
            "This worker has no resumable thread; start a new linked worker",
        ));
    }
    crate::agent_tools::authorize_session(snapshot, &session, approved, config)
}

pub(super) fn authorize_sender(
    snapshot: &Snapshot,
    session: &str,
    sender: &Sender,
) -> Result<(), Error> {
    if let Sender::Agent { session_id } = sender {
        crate::agent_tools::can_message(snapshot, session_id, session)?;
    }
    Ok(())
}

fn authorize_queued(
    snapshot: &Snapshot,
    session: &str,
    approved: bool,
    config: &RuntimeConfig,
    sender: &Sender,
    waiting: &std::collections::HashSet<String>,
) -> Result<bool, Error> {
    authorize_policy(snapshot, session, approved, config)?;
    authorize_sender(snapshot, session, sender)?;
    runtime::launch_available(snapshot, session, waiting)
}

pub(super) fn wait_for_workspace(
    connection: &Connection,
    snapshot: &mut Snapshot,
    session: &str,
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT OR IGNORE INTO workspace_waits(session_id) VALUES(?1)",
            [session],
        )
        .map_err(Error::internal)?;
    if let Some(next) = snapshot
        .submissions
        .iter_mut()
        .find(|s| s.session_id == session && s.state == SubmissionState::Queued)
    {
        next.error = Some("Waiting for a shared workspace".into());
    }
    Ok(())
}

/// Mint the peer grants of an accepted message sent to `session`. Only a
/// human's message grants: every mentioned session must be live and differ
/// from the recipient, otherwise nothing is granted and the send is rejected.
/// The caller commits the grants with the submission and its receipt.
pub(super) fn grant_mentions(
    snapshot: &mut Snapshot,
    session: &str,
    request: &str,
    parts: &[Part],
    sender: &Sender,
) -> Result<(), Error> {
    let targets: Vec<String> = session_mentions(parts)
        .into_iter()
        .map(|m| m.session_id.clone())
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    if *sender != Sender::User {
        return Err(Error::invalid("Only people can mention sessions"));
    }
    // A queued turn of an archived chat can still be edited; a grant made
    // then would return on unarchive.
    if !snapshot.is_live(session) {
        return Err(Error::invalid("Unarchive this chat before continuing"));
    }
    for target in &targets {
        if target == session {
            return Err(Error::invalid("Remove the mention of this session"));
        }
        if !snapshot.is_live(target) {
            return Err(Error::invalid(
                "Remove the mention of an archived or missing session",
            ));
        }
    }
    let granted_at = now();
    for target in &targets {
        snapshot.grant_peer(session, target, request, granted_at);
    }
    Ok(())
}

pub(super) fn apply(
    transaction: &Transaction<'_>,
    snapshot: &mut Snapshot,
    request: &str,
    command: Command,
    config: &RuntimeConfig,
    controls: &HashMap<String, watch::Sender<bool>>,
    sender: Sender,
) -> Result<Option<Action>, Error> {
    match command {
        Command::SubmitTurn {
            session_id,
            draft_revision,
            parts,
            approve_implementation,
        } => {
            validate(transaction, snapshot, &session_id, &parts)?;
            if !has_content(&parts) {
                return Err(Error::invalid("Write a message first"));
            }
            // message_agent already rechecks an agent sender under this writer
            // lock. Keep mention validation's normal rejection below.
            let available = authorize(
                transaction,
                snapshot,
                &session_id,
                approve_implementation,
                config,
            )?;
            let previous = draft(transaction, &session_id)?;
            if previous.revision != draft_revision || previous.parts != parts {
                return Err(Error::new(
                    StatusCode::CONFLICT,
                    "draft_conflict",
                    "Save and review the shared draft before sending",
                ));
            }
            let worker = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .unwrap()
                .worker
                .as_ref()
                .unwrap();
            if !snapshot.can_continue(
                snapshot
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .unwrap(),
            ) {
                return Err(Error::invalid(
                    "This worker has no resumable thread; start a new linked worker",
                ));
            }
            let start = available
                && !runtime::active(&worker.status)
                && !snapshot.submissions.iter().any(|s| {
                    s.session_id == session_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                });
            grant_mentions(snapshot, &session_id, request, &parts, &sender)?;
            let submission = Submission {
                id: request.into(),
                session_id: session_id.clone(),
                parts: parts.clone(),
                state: if start {
                    SubmissionState::Launching
                } else {
                    SubmissionState::Queued
                },
                approve_implementation,
                error: None,
                interrupts_run: None,
                last_edit_request: None,
                sender,
            };
            snapshot.submissions.push(submission);
            if !available {
                wait_for_workspace(transaction, snapshot, &session_id)?;
            }
            store_draft(
                transaction,
                &Draft {
                    session_id: session_id.clone(),
                    revision: previous.revision + 1,
                    parts: vec![],
                },
            )?;
            if start {
                reserve(snapshot, &session_id, request, &parts);
                return Ok(Some(Action::Run {
                    session_id,
                    prompt: plain_text(&parts),
                }));
            }
        }
        Command::PromoteTurn {
            submission_id,
            active_run_id,
        } => {
            let index = snapshot
                .submissions
                .iter()
                .position(|s| {
                    s.id == submission_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                })
                .ok_or_else(|| Error::invalid("Message is no longer queued"))?;
            let session_id = snapshot.submissions[index].session_id.clone();
            let available = authorize(
                transaction,
                snapshot,
                &session_id,
                snapshot.submissions[index].approve_implementation,
                config,
            )?;
            // A later message cannot start the thread ahead of the session's
            // waiting first turn; it stays queued behind it.
            if snapshot
                .waiting_first_turn(
                    snapshot
                        .sessions
                        .iter()
                        .find(|s| s.id == session_id)
                        .unwrap(),
                )
                .is_some_and(|first| first.id != submission_id)
            {
                return Err(Error::invalid(
                    "This agent's first turn runs before other queued messages",
                ));
            }
            let worker = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .unwrap()
                .worker
                .as_ref()
                .unwrap();
            if runtime::active(&worker.status) {
                let current: Option<String> = transaction
                    .query_row(
                        "SELECT run_id FROM runs WHERE session_id=?1",
                        [&session_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(Error::internal)?;
                if current != active_run_id
                    || current.is_none()
                    || !controls.contains_key(&session_id)
                {
                    return Err(Error::invalid(
                        "Active turn changed; review before interrupting",
                    ));
                }
            }
            let mut submission = snapshot.submissions.remove(index);
            submission.state = SubmissionState::Queued;
            submission.error = None;
            submission.interrupts_run = active_run_id;
            let index = snapshot
                .submissions
                .iter()
                .position(|s| s.session_id == session_id && s.state == SubmissionState::Queued)
                .unwrap_or(snapshot.submissions.len());
            let id = submission.id.clone();
            let parts = submission.parts.clone();
            snapshot.submissions.insert(index, submission);
            if runtime::active(&worker.status) {
                return Ok(Some(Action::Promote(session_id)));
            }
            if !available {
                // First in the queue, it launches once the workspace is free.
                wait_for_workspace(transaction, snapshot, &session_id)?;
                snapshot.revision += 1;
                return Ok(None);
            }
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .state = SubmissionState::Launching;
            reserve(snapshot, &session_id, &id, &parts);
            // A promoted turn still uses its original durable submission identity.
            transaction
                .execute(
                    "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                    params![session_id, id],
                )
                .map_err(Error::internal)?;
            return Ok(Some(Action::Run {
                session_id,
                prompt: plain_text(&parts),
            }));
        }
        Command::CancelTurn { submission_id } => {
            let submission = snapshot
                .submissions
                .iter_mut()
                .find(|s| {
                    s.id == submission_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                })
                .ok_or_else(|| Error::invalid("Message already launching or missing"))?;
            submission.state = SubmissionState::Cancelled;
            submission.interrupts_run = None;
        }
        Command::EditQueuedTurn {
            submission_id,
            draft_revision,
            parts,
            approve_implementation,
        } => {
            let submission = snapshot
                .submissions
                .iter()
                .find(|s| {
                    s.id == submission_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                })
                .ok_or_else(|| Error::invalid("Message already launching or missing"))?;
            // A waiting fork launches from its receipt, not from editable text.
            if transaction
                .query_row(
                    "SELECT request FROM receipts WHERE request_id=?1",
                    [&submission_id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .map_err(Error::internal)?
                .and_then(|request| serde_json::from_str::<CommandEnvelope>(&request).ok())
                .is_some_and(|envelope| {
                    matches!(envelope.command, Command::ConversationCommand { name, .. } if name == "fork")
                })
            {
                return Err(Error::invalid(
                    "A waiting fork cannot be edited; cancel it instead",
                ));
            }
            validate(transaction, snapshot, &submission.session_id, &parts)?;
            let previous = draft(transaction, &submission.session_id)?;
            if previous.revision != draft_revision || previous.parts != parts {
                return Err(Error::new(
                    StatusCode::CONFLICT,
                    "draft_conflict",
                    "Shared draft changed before queue edit",
                ));
            }
            if !has_content(&parts) {
                return Err(Error::invalid("Queued message cannot be empty"));
            }
            let session_id = submission.session_id.clone();
            // An edit may add grants; removed mentions keep theirs.
            grant_mentions(snapshot, &session_id, request, &parts, &sender)?;
            store_draft(
                transaction,
                &Draft {
                    session_id,
                    revision: previous.revision + 1,
                    parts: vec![],
                },
            )?;
            let submission = snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == submission_id)
                .unwrap();
            submission.parts = parts;
            submission.approve_implementation = approve_implementation;
            submission.last_edit_request = Some(request.into());
        }
        Command::PauseQueue { session_id } => {
            if !snapshot
                .submissions
                .iter()
                .any(|s| s.session_id == session_id && s.state == SubmissionState::Queued)
            {
                return Err(Error::invalid("Nothing is queued to pause"));
            }
            pause(snapshot, &session_id, "Queue paused; resume to continue");
        }
        Command::ResumeQueue { session_id, .. } => {
            // While a turn runs, resuming only releases the held turns; they
            // follow the current turn as queued turns do.
            let running = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.worker.as_ref())
                .is_some_and(|w| runtime::active(&w.status));
            if running {
                let mut released = false;
                for item in snapshot
                    .submissions
                    .iter_mut()
                    .filter(|s| s.session_id == session_id && s.state == SubmissionState::Paused)
                {
                    item.state = SubmissionState::Queued;
                    item.error = None;
                    released = true;
                }
                if !released {
                    return Err(Error::invalid("Agent is already running"));
                }
                snapshot.revision += 1;
                return Ok(None);
            }
            let next = snapshot
                .submissions
                .iter()
                .find(|s| {
                    s.session_id == session_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                })
                .ok_or_else(|| Error::invalid("Queue is empty"))?
                .clone();
            let worker = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.worker.as_ref())
                .ok_or_else(|| Error::invalid("Worker missing"))?;
            if runtime::active(&worker.status) {
                return Err(Error::invalid("Agent is already running"));
            }
            let available = authorize(
                transaction,
                snapshot,
                &session_id,
                next.approve_implementation,
                config,
            )?;
            for item in snapshot
                .submissions
                .iter_mut()
                .filter(|s| s.session_id == session_id && s.state == SubmissionState::Paused)
            {
                item.state = SubmissionState::Queued;
                item.error = None;
            }
            if !available {
                wait_for_workspace(transaction, snapshot, &session_id)?;
                snapshot.revision += 1;
                return Ok(None);
            }
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == next.id)
                .unwrap()
                .state = SubmissionState::Launching;
            reserve(snapshot, &session_id, &next.id, &next.parts);
            transaction
                .execute(
                    "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                    params![session_id, next.id],
                )
                .map_err(Error::internal)?;
            return Ok(Some(Action::Run {
                session_id,
                prompt: plain_text(&next.parts),
            }));
        }
        _ => unreachable!(),
    }
    snapshot.revision += 1;
    Ok(None)
}

pub(super) fn reserve(snapshot: &mut Snapshot, session: &str, id: &str, parts: &[Part]) {
    let agent = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session)
        .unwrap();
    if agent.awaiting_first_message {
        if agent.automatic_name {
            agent.title = crate::task_title(&plain_text(parts));
        }
        if let Some(spawn) = agent.spawn.as_mut() {
            spawn.task = plain_text(parts);
        }
        // Naming can happen at reservation, but first-message eligibility is
        // consumed only when the harness actually launches. A late guard may
        // still return this never-launched turn to the queue.
    }
    let worker = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session)
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
    let sender = snapshot
        .submissions
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.sender.clone())
        .unwrap_or_default();
    let prompt = Message {
        id: format!("prompt-{id}"),
        session_id: session.into(),
        author: if id.starts_with(crate::notices::PREFIX) {
            "Relay"
        } else {
            "You"
        }
        .into(),
        sender,
        kind: "prompt".into(),
        body: plain_text(parts),
        parts: parts.to_vec(),
        tool: None,
    };
    // Resuming a failed/interrupted reservation delivers the same logical
    // submission. Keep its history anchor and position, including after a
    // restart; do not append another row with the same message ID.
    if let Some(existing) = snapshot.messages.iter_mut().find(|m| m.id == prompt.id) {
        *existing = prompt;
    } else {
        snapshot.messages.push(prompt);
    }
}

pub(super) fn advance(workspace: &Workspace, session: &str, run: &str) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.run_id(session)?.as_deref() != Some(run) {
        return Ok(());
    }
    let mut snapshot = store.snapshot()?;
    let status = snapshot
        .sessions
        .iter()
        .find(|s| s.id == session)
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .status
        .clone();
    if runtime::active(&status) {
        return Ok(());
    }
    if let Some(submission) = snapshot.submissions.iter_mut().find(|s| s.id == run) {
        submission.state = match status {
            WorkerStatus::Completed => SubmissionState::Completed,
            WorkerStatus::Stopped | WorkerStatus::Interrupted => SubmissionState::Interrupted,
            _ => SubmissionState::Failed,
        };
    }
    // A held queue launches nothing until it is resumed, except a turn
    // promoted to replace this run.
    let held = snapshot
        .submissions
        .iter()
        .any(|s| s.session_id == session && s.state == SubmissionState::Paused);
    let next = snapshot
        .submissions
        .iter()
        .find(|s| {
            s.session_id == session
                && s.state == SubmissionState::Queued
                && ((status == WorkerStatus::Completed && !held)
                    || s.interrupts_run.as_deref() == Some(run))
        })
        .cloned();
    if store.closing || next.is_none() {
        if status != WorkerStatus::Completed || store.closing {
            pause(
                &mut snapshot,
                session,
                "Queue paused; review and resume explicitly",
            );
        }
    } else if let Some(next) = next {
        let authorization = authorize_queued(
            &snapshot,
            session,
            next.approve_implementation,
            &workspace.config,
            &next.sender,
            &waiting_sessions(&store.connection)?,
        );
        if let Err(error) = authorization {
            pause(&mut snapshot, session, &error.to_string());
        } else if matches!(authorization, Ok(false)) {
            wait_for_workspace(&store.connection, &mut snapshot, session)?;
        } else {
            validate(&store.connection, &snapshot, session, &next.parts)?;
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == next.id)
                .unwrap()
                .state = SubmissionState::Launching;
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == next.id)
                .unwrap()
                .error = None;
            reserve(&mut snapshot, session, &next.id, &next.parts);
            let transaction = store
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(Error::internal)?;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                    params![session, next.id],
                )
                .map_err(Error::internal)?;
            snapshot.revision += 1;
            transaction
                .execute(
                    "UPDATE workspace SET snapshot=?1 WHERE id=1",
                    [serde_json::to_string(&snapshot).map_err(Error::internal)?],
                )
                .map_err(Error::internal)?;
            transaction.commit().map_err(Error::internal)?;
            workspace.snapshots.send_replace(snapshot);
            let (sender, receiver) = watch::channel(false);
            store.controls.insert(session.into(), sender);
            tokio::spawn(runtime::run(
                workspace.clone(),
                session.into(),
                next.id,
                plain_text(&next.parts),
                receiver,
            ));
            return Ok(());
        }
    }
    snapshot.revision += 1;
    store.save(&snapshot)?;
    workspace.snapshots.send_replace(snapshot);
    Ok(())
}

/// Return a turn rejected by the final launch guard to its durable queue. A
/// reservation is not delivery, and reserve keeps the prompt identity stable
/// when this turn is eventually launched again. A turn started by a command
/// without a durable submission (a new or continued worker) gets one with the
/// command's request ID, prompt and approval, ahead of the session's queue.
pub(super) fn defer_launch(
    workspace: &Workspace,
    session: &str,
    run: &str,
    prompt: &str,
    stop: &watch::Receiver<bool>,
) -> Result<bool, Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    // Stop acceptance signals this channel under this same writer lock. A
    // pre-lock observation cannot authorize replacing its accepted stop with a
    // fresh channel in a subsequent queue drain.
    if *stop.borrow() || store.run_id(session)?.as_deref() != Some(run) || store.closing {
        return Ok(false);
    }
    let mut snapshot = store.snapshot()?;
    if let Some(submission) = snapshot.submissions.iter_mut().find(|s| s.id == run) {
        if submission.state != SubmissionState::Launching {
            return Ok(false);
        }
        submission.state = SubmissionState::Queued;
        submission.interrupts_run = None;
    } else {
        let request: Option<String> = store
            .connection
            .query_row(
                "SELECT request FROM receipts WHERE request_id=?1",
                [run],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::internal)?;
        let approved = match request
            .and_then(|request| serde_json::from_str::<CommandEnvelope>(&request).ok())
            .map(|envelope| envelope.command)
        {
            Some(
                Command::StartWorker {
                    approve_implementation,
                    ..
                }
                | Command::SendWorker {
                    approve_implementation,
                    ..
                }
                | Command::StartRootAgent {
                    approve_implementation,
                    ..
                }
                | Command::StartSession {
                    approve_implementation,
                    ..
                },
            ) => approve_implementation,
            // A fork waits as its "/fork" turn and launches from its receipt.
            Some(Command::ConversationCommand { name, .. }) if name == "fork" => true,
            _ => return Ok(false),
        };
        let position = snapshot
            .submissions
            .iter()
            .position(|s| {
                s.session_id == session
                    && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
            })
            .unwrap_or(snapshot.submissions.len());
        snapshot.submissions.insert(
            position,
            Submission {
                id: run.into(),
                session_id: session.into(),
                parts: vec![Part::text(prompt)],
                state: SubmissionState::Queued,
                approve_implementation: approved,
                error: None,
                interrupts_run: None,
                last_edit_request: None,
                sender: Sender::User,
            },
        );
    }
    let worker = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session)
        .and_then(|s| s.worker.as_mut())
        .ok_or_else(|| Error::invalid("Worker missing"))?;
    // The reservation never launched a harness and must no longer hold a lock.
    worker.status = WorkerStatus::Completed;
    worker.error = None;
    let transaction = store.connection.transaction().map_err(Error::internal)?;
    wait_for_workspace(&transaction, &mut snapshot, session)?;
    snapshot.revision += 1;
    transaction
        .execute(
            "UPDATE workspace SET snapshot=?1 WHERE id=1",
            [serde_json::to_string(&snapshot).map_err(Error::internal)?],
        )
        .map_err(Error::internal)?;
    transaction.commit().map_err(Error::internal)?;
    store.controls.remove(session);
    store.agent_capabilities.remove(session);
    workspace.snapshots.send_replace(snapshot);
    Ok(true)
}

/// One event-driven pass, under the same writer lock as acceptance and launch.
/// Only queues explicitly deferred for contention are eligible. Paused queues
/// (including restart recovery) always require a human resume. Reservations in
/// this pass immediately participate in the guard for the following candidate.
pub(super) fn drain_workspace_waits(workspace: &Workspace) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing {
        return Ok(());
    }
    let mut waiting = waiting_sessions(&store.connection)?;
    if waiting.is_empty() {
        return Ok(());
    }
    // Every waiting queue, launched or not in this pass, keeps its place
    // ahead of later turns of other sessions.
    let waiters = waiting.clone();
    let mut snapshot = store.snapshot()?;
    let next: Vec<_> = snapshot
        .submissions
        .iter()
        .filter(|s| s.state == SubmissionState::Queued && waiting.remove(&s.session_id))
        .cloned()
        .collect();
    let transaction = store.connection.transaction().map_err(Error::internal)?;
    // Deleted sessions and queues with no queued turns have nothing to wake.
    for id in waiting {
        transaction
            .execute("DELETE FROM workspace_waits WHERE session_id=?1", [id])
            .map_err(Error::internal)?;
    }
    let mut launches = Vec::new();
    let mut changed = false;
    for next in next {
        let session = &next.session_id;
        let held = snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == *session && s.state == SubmissionState::Paused);
        if held || !snapshot.sessions.iter().any(|s| s.id == *session) {
            transaction
                .execute("DELETE FROM workspace_waits WHERE session_id=?1", [session])
                .map_err(Error::internal)?;
            continue;
        }
        if snapshot
            .sessions
            .iter()
            .find(|s| s.id == *session)
            .and_then(|s| s.worker.as_ref())
            .is_some_and(|w| runtime::active(&w.status))
        {
            continue;
        }
        let authorization = authorize_queued(
            &snapshot,
            session,
            next.approve_implementation,
            &workspace.config,
            &next.sender,
            &waiters,
        )
        .and_then(|available| {
            validate(&transaction, &snapshot, session, &next.parts)?;
            Ok(available)
        });
        match authorization {
            Ok(false) => continue,
            Err(error) => pause(&mut snapshot, session, &error.to_string()),
            Ok(true) => {
                let submission = snapshot
                    .submissions
                    .iter_mut()
                    .find(|s| s.id == next.id)
                    .unwrap();
                submission.state = SubmissionState::Launching;
                submission.error = None;
                reserve(&mut snapshot, session, &next.id, &next.parts);
                transaction
                    .execute(
                        "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                        params![session, next.id],
                    )
                    .map_err(Error::internal)?;
                launches.push(next.clone());
            }
        }
        changed = true;
        transaction
            .execute(
                "DELETE FROM workspace_waits WHERE session_id=?1",
                [&next.session_id],
            )
            .map_err(Error::internal)?;
    }
    if changed {
        snapshot.revision += 1;
        transaction
            .execute(
                "UPDATE workspace SET snapshot=?1 WHERE id=1",
                [serde_json::to_string(&snapshot).map_err(Error::internal)?],
            )
            .map_err(Error::internal)?;
    }
    transaction.commit().map_err(Error::internal)?;
    if changed {
        workspace.snapshots.send_replace(snapshot);
    }
    for next in launches {
        let (sender, receiver) = watch::channel(false);
        store.controls.insert(next.session_id.clone(), sender);
        tokio::spawn(runtime::run(
            workspace.clone(),
            next.session_id,
            next.id,
            plain_text(&next.parts),
            receiver,
        ));
    }
    Ok(())
}
