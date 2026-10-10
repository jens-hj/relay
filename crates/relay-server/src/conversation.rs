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
        return Ok(Json(
            serde_json::from_str(&response).map_err(Error::internal)?,
        ));
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
    Ok(Json(next))
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

fn authorize(
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
    if session.fixture || session.worker.is_none() {
        return Err(Error::invalid("This session cannot run agent turns"));
    }
    let worker = session.worker.as_ref().unwrap();
    if !runtime::active(&worker.status) && (worker.thread_id.is_none() || worker.worktree.is_none())
    {
        return Err(Error::invalid(
            "This worker has no resumable thread; start a new linked worker",
        ));
    }
    runtime::authorize_session(
        &candidate,
        session
            .issue_id
            .as_deref()
            .ok_or_else(|| Error::invalid("Session has no linked issue"))?,
        &session.director_id,
        approved,
        &session.role,
        config,
    )
}

pub(super) fn apply(
    transaction: &Transaction<'_>,
    snapshot: &mut Snapshot,
    request: &str,
    command: Command,
    config: &RuntimeConfig,
    controls: &HashMap<String, watch::Sender<bool>>,
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
            authorize(snapshot, &session_id, approve_implementation, config)?;
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
            if !runtime::active(&worker.status)
                && (worker.thread_id.is_none() || worker.worktree.is_none())
            {
                return Err(Error::invalid(
                    "This worker has no resumable thread; start a new linked worker",
                ));
            }
            let start = !runtime::active(&worker.status)
                && !snapshot.submissions.iter().any(|s| {
                    s.session_id == session_id
                        && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
                });
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
            };
            snapshot.submissions.push(submission);
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
            authorize(
                snapshot,
                &session_id,
                snapshot.submissions[index].approve_implementation,
                config,
            )?;
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
            store_draft(
                transaction,
                &Draft {
                    session_id: submission.session_id.clone(),
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
        Command::ResumeQueue { session_id } => {
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
            authorize(snapshot, &session_id, next.approve_implementation, config)?;
            for item in snapshot
                .submissions
                .iter_mut()
                .filter(|s| s.session_id == session_id && s.state == SubmissionState::Paused)
            {
                item.state = SubmissionState::Queued;
                item.error = None;
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

fn reserve(snapshot: &mut Snapshot, session: &str, id: &str, parts: &[Part]) {
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
    worker.usage = None;
    snapshot.messages.push(Message {
        id: format!("prompt-{id}"),
        session_id: session.into(),
        author: "You".into(),
        kind: "prompt".into(),
        body: plain_text(parts),
        parts: parts.to_vec(),
    });
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
    let next = snapshot
        .submissions
        .iter()
        .find(|s| {
            s.session_id == session
                && s.state == SubmissionState::Queued
                && (status == WorkerStatus::Completed || s.interrupts_run.as_deref() == Some(run))
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
        if let Err(error) = authorize(
            &snapshot,
            session,
            next.approve_implementation,
            &workspace.config,
        ) {
            pause(&mut snapshot, session, &error.to_string());
        } else {
            validate(&store.connection, &snapshot, session, &next.parts)?;
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == next.id)
                .unwrap()
                .state = SubmissionState::Launching;
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
