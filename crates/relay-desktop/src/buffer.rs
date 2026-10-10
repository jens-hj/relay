use crate::platform::Instant;
use crate::{
    buffer_network::{Outcome, Request, Update},
    model::{Model, Saved},
};
use relay_core::*;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

#[derive(Clone)]
pub struct Document {
    pub remote: Draft,
    pub parts: Vec<Part>,
    pub recovery: Vec<Vec<Part>>,
    pub recovery_reviewed: bool,
    pub conflict: bool,
    pub error: String,
    pub saving: Option<SaveDraft>,
    pub finalize: Option<Vec<Part>>,
    pub submitting: bool,
    pub changed: Instant,
    pub editing_queue: Option<String>,
    pub force_after_submit: bool,
}
impl Document {
    fn new(draft: Draft) -> Self {
        Self {
            parts: draft.parts.clone(),
            remote: draft,
            recovery: vec![],
            recovery_reviewed: false,
            conflict: false,
            error: String::new(),
            saving: None,
            finalize: None,
            submitting: false,
            changed: Instant::now(),
            editing_queue: None,
            force_after_submit: false,
        }
    }
}

#[derive(Clone, Default)]
pub struct BufferState {
    pub documents: BTreeMap<String, Document>,
    pub blobs: BTreeMap<String, Arc<Vec<u8>>>,
    pub uploads: BTreeMap<String, Asset>,
    pub failed_uploads: BTreeSet<String>,
    pub fetching: BTreeSet<String>,
    pub fetch_errors: BTreeMap<String, String>,
    pub last_queued: BTreeMap<String, String>,
    pub initialized: bool,
    pub connected: bool,
    pub serial: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pub journal: Option<PathBuf>,
    pub approval_needed: bool,
    pub promote_after_ack: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct JournalDocument {
    remote: Draft,
    parts: Vec<Part>,
    recovery: Vec<Vec<Part>>,
    #[serde(default)]
    recovery_reviewed: bool,
    saving: Option<SaveDraft>,
    #[serde(default)]
    editing_queue: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_journal(model: Model, settings: &Path, config: &crate::network::Config) {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    config.endpoint.as_str().hash(&mut hash);
    config.token.hash(&mut hash);
    let path = settings.with_file_name(format!("buffer-drafts-{:016x}.json", hash.finish()));
    let mut state = model.buffer.get_untracked();
    state.journal = Some(path.clone());
    match std::fs::read(&path) {
        Ok(bytes) => {
            match serde_json::from_slice::<BTreeMap<String, JournalDocument>>(&bytes) {
                Ok(documents) => {
                    for (session, record) in documents {
                        let mut doc = Document::new(record.remote);
                        doc.parts = record.parts;
                        doc.recovery = record.recovery;
                        doc.recovery_reviewed = record.recovery_reviewed;
                        doc.saving = record.saving;
                        if doc.saving.is_some() {
                            doc.error="Recovered save awaiting confirmation; retry preserves its original ID".into();
                        }
                        doc.editing_queue = record.editing_queue;
                        state.documents.insert(session, doc);
                    }
                }
                Err(_) => model.notice.set(
                    "Cannot read recovered agent drafts; the recovery file was preserved".into(),
                ),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => model
            .notice
            .set("Cannot read the local draft recovery file".into()),
    }
    // Assets from recovered drafts are retried with their original immutable IDs.
    let referenced = state
        .documents
        .values()
        .flat_map(|doc| {
            assets(&doc.parts)
                .into_iter()
                .chain(assets(&doc.remote.parts))
                .chain(doc.recovery.iter().flat_map(|parts| assets(parts)))
                .cloned()
        })
        .collect::<Vec<_>>();
    for asset in referenced {
        if uuid::Uuid::parse_str(&asset.id).is_err() {
            continue;
        }
        if let Ok(file) = std::fs::File::open(path.with_extension("assets").join(&asset.id)) {
            use std::io::Read;
            let mut bytes = vec![];
            if file
                .take((ASSET_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .is_ok()
                && bytes.len() as u64 == asset.size
            {
                let bytes = Arc::new(bytes);
                state.blobs.insert(asset.id.clone(), bytes.clone());
                state.uploads.insert(asset.id.clone(), asset.clone());
                if let Some(requests) = model.buffer_requests.get_untracked() {
                    let _ = requests.send(Request::Upload { asset, bytes });
                }
            }
        }
    }
    model.buffer.set(state);
}

#[cfg(not(target_arch = "wasm32"))]
fn journal(state: &BufferState) -> Result<(), String> {
    let Some(path) = &state.journal else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| "Cannot create the draft recovery directory")?;
    }
    let cache = path.with_extension("assets");
    std::fs::create_dir_all(&cache)
        .map_err(|_| "Cannot create the inline file recovery directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect inline file recovery")?;
    }
    for (id, asset) in &state.uploads {
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        let Some(bytes) = state.blobs.get(id) else {
            continue;
        };
        if bytes.len() as u64 != asset.size {
            continue;
        }
        let destination = cache.join(id);
        if destination.exists() {
            continue;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let temporary = cache.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|_| "Cannot recover pasted file locally")?;
            std::io::Write::write_all(&mut file, bytes)
                .map_err(|_| "Cannot write pasted file recovery")?;
            file.sync_all()
                .map_err(|_| "Cannot flush pasted file recovery")?;
            std::fs::rename(&temporary, &destination)
                .map_err(|_| "Cannot replace pasted file recovery")
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
    }
    let records = state
        .documents
        .iter()
        .map(|(id, doc)| {
            let mut recovery = doc.recovery.clone();
            if let Some(parts) = &doc.finalize {
                recovery.push(parts.clone());
            }
            if doc.submitting && !doc.remote.parts.is_empty() {
                recovery.push(doc.remote.parts.clone());
            }
            (
                id.clone(),
                JournalDocument {
                    remote: doc.remote.clone(),
                    parts: doc.parts.clone(),
                    recovery,
                    recovery_reviewed: doc.recovery_reviewed
                        && doc.finalize.is_none()
                        && !doc.submitting,
                    saving: doc.saving.clone(),
                    editing_queue: doc.editing_queue.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let bytes =
        serde_json::to_vec(&records).map_err(|_| "Cannot encode the draft recovery file")?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Cannot write the draft recovery file")?;
        std::io::Write::write_all(&mut file, &bytes)
            .map_err(|_| "Cannot write the draft recovery file")?;
        file.sync_all()
            .map_err(|_| "Cannot flush the draft recovery file")?;
        std::fs::rename(&temporary, path).map_err(|_| "Cannot replace the draft recovery file")?;
        let referenced = state
            .documents
            .values()
            .flat_map(|doc| {
                assets(&doc.parts)
                    .into_iter()
                    .chain(assets(&doc.remote.parts))
                    .chain(doc.recovery.iter().flat_map(|parts| assets(parts)))
                    .chain(doc.finalize.iter().flat_map(|parts| assets(parts)))
            })
            .map(|asset| asset.id.clone())
            .chain(state.uploads.keys().cloned())
            .collect::<BTreeSet<_>>();
        if let Ok(entries) = std::fs::read_dir(&cache) {
            for entry in entries.flatten() {
                if let Some(id) = entry.file_name().to_str()
                    && uuid::Uuid::parse_str(id).is_ok()
                    && !referenced.contains(id)
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn commit(model: Model, state: BufferState) {
    #[cfg(target_arch = "wasm32")]
    persist_browser(model, &state);
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = journal(&state) {
        model.notice.set(error);
    }
    model.buffer.set(state);
}

pub fn parts(model: Model, session: &str) -> Vec<Part> {
    model
        .buffer
        .get()
        .documents
        .get(session)
        .map(|d| d.parts.clone())
        .unwrap_or_default()
}

pub fn edit(model: Model, session: &str, parts: Vec<Part>) {
    let mut state = model.buffer.get_untracked();
    let doc = state.documents.entry(session.into()).or_insert_with(|| {
        Document::new(Draft {
            session_id: session.into(),
            ..Default::default()
        })
    });
    doc.parts = parts;
    doc.changed = Instant::now();
    commit(model, state);
}

pub fn execution_problem(model: Model) -> Option<String> {
    let snapshot = model.snapshot.get();
    let id = model.session.get();
    let session = snapshot.sessions.iter().find(|s| s.id == id)?;
    let Some(worker) = session.worker.as_ref().filter(|_| !session.fixture) else {
        return Some("This transcript cannot execute turns".into());
    };
    let Some(issue) = snapshot
        .issues
        .iter()
        .find(|i| Some(&i.id) == session.issue_id.as_ref())
    else {
        return Some("Linked issue is unavailable".into());
    };
    let Some(_project) = snapshot.projects.iter().find(|p| p.id == issue.project_id) else {
        return Some("Project is unavailable".into());
    };
    if !snapshot.visible_task(&issue.id) {
        return Some("Issue is no longer on the board; restore it and sync".into());
    }
    let Some(director) = snapshot
        .directors
        .iter()
        .find(|d| d.id == session.director_id)
    else {
        return Some("Director is unavailable".into());
    };
    let Ok(profile) = snapshot.effective_profile(director) else {
        return Some("Director profile is invalid".into());
    };
    if matches!(&profile.scope,DirectorScope::Issues{issue_ids} if !issue_ids.contains(&issue.id)) {
        return Some("Issue is outside director scope".into());
    }
    let active = snapshot
        .sessions
        .iter()
        .filter(|s| {
            s.id != session.id
                && s.director_id == session.director_id
                && s.worker.as_ref().is_some_and(|w| {
                    matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)
                })
        })
        .count();
    if active >= usize::from(profile.max_workers) {
        return Some("No worker slots available".into());
    }
    if !matches!(worker.status, WorkerStatus::Queued | WorkerStatus::Running)
        && (worker.thread_id.is_none() || worker.worktree.is_none())
    {
        return Some("This worker has no resumable thread; open its linked issue".into());
    }
    None
}

pub fn promote(model: Model, id: &str) {
    let snapshot = model.snapshot.get_untracked();
    let Some(submission) = snapshot.submissions.iter().find(|s| {
        s.id == id && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
    }) else {
        return;
    };
    let session = &submission.session_id;
    let active = snapshot
        .sessions
        .iter()
        .find(|s| &s.id == session)
        .and_then(|s| s.worker.as_ref())
        .is_some_and(|w| matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running));
    let active_run_id = active
        .then(|| {
            snapshot
                .messages
                .iter()
                .rev()
                .find(|m| &m.session_id == session && m.kind == "prompt")
                .and_then(|m| m.id.strip_prefix("prompt-"))
                .map(str::to_owned)
        })
        .flatten();
    model.submit(
        Command::PromoteTurn {
            submission_id: id.into(),
            active_run_id,
        },
        snapshot.revision,
        Saved::Action,
    );
}

fn send_first(model: Model) {
    let session_id = model.session.get_untracked();
    let state = model.buffer.get_untracked();
    let parts = parts(model, &session_id);
    if !has_content(&parts) || model.busy.get_untracked() || model.can_retry() {
        return;
    }
    if !model.connected.get_untracked() {
        model
            .notice
            .set("Reconnect before sending. Your draft is retained.".into());
        return;
    }
    if assets(&parts)
        .iter()
        .any(|a| state.uploads.contains_key(&a.id) || state.failed_uploads.contains(&a.id))
    {
        model
            .notice
            .set("Wait for inline files to finish uploading before sending".into());
        return;
    }
    if let Err(error) = validate_parts(&model.snapshot.get_untracked(), &session_id, &parts) {
        model.notice.set(error);
        return;
    }
    model.submit(
        Command::StartDirector {
            director_id: model.worker_director.get_untracked(),
            prompt: plain_text(&parts),
            parts: parts.clone(),
            approve_implementation: false,
        },
        model.snapshot.get_untracked().revision,
        Saved::DirectorStart {
            draft: Draft {
                session_id,
                revision: 0,
                parts,
            },
        },
    );
}

pub fn acknowledge_first(model: Model, submitted: &Draft, session: &str) {
    let mut state = model.buffer.get_untracked();
    if let Some(mut doc) = state.documents.remove(&submitted.session_id) {
        if doc.parts == submitted.parts {
            doc.parts.clear();
        }
        doc.remote = Draft {
            session_id: session.into(),
            ..Default::default()
        };
        state.documents.insert(session.into(), doc);
    }
    commit(model, state);
}

pub fn send(model: Model) {
    if model.page.get_untracked() == crate::model::Page::DirectorStart {
        send_first(model);
        return;
    }
    let session_id = model.session.get_untracked();
    let mut state = model.buffer.get_untracked();
    let snapshot = model.snapshot.get_untracked();
    let Some(session) = snapshot.sessions.iter().find(|s| s.id == session_id) else {
        return;
    };
    let Some(_) = session.worker.as_ref().filter(|_| !session.fixture) else {
        model.notice.set("This transcript has no running agent. Drafts can be saved; execution requires a linked worker.".into());
        return;
    };
    if let Some(error) = execution_problem(model) {
        model.notice.set(error);
        return;
    }
    if !state.connected || !model.connected.get_untracked() {
        model
            .notice
            .set("Reconnect before sending. Your draft is retained.".into());
        return;
    }
    let doc = state
        .documents
        .entry(session_id.clone())
        .or_insert_with(|| {
            Document::new(Draft {
                session_id: session_id.clone(),
                ..Default::default()
            })
        });
    if doc.conflict || !doc.error.is_empty() {
        model
            .notice
            .set("Resolve the draft save issue before sending".into());
        return;
    }
    if doc.finalize.is_some() || doc.submitting {
        if !has_content(&doc.parts) {
            doc.force_after_submit = true;
            commit(model, state);
        }
        return;
    }
    if model.busy.get_untracked() || model.can_retry() {
        model
            .notice
            .set("Resolve the previous request before sending another message".into());
        return;
    }
    if !has_content(&doc.parts) {
        let Some(id) = state.last_queued.get(&session_id).cloned() else {
            return;
        };
        promote(model, &id);
        return;
    }
    let Some(director) = snapshot
        .directors
        .iter()
        .find(|d| d.id == session.director_id)
    else {
        return;
    };
    let Ok(profile) = snapshot.effective_profile(director) else {
        return;
    };
    match profile
        .permissions
        .get(&Task::Implement)
        .copied()
        .unwrap_or(Permission::Deny)
    {
        Permission::Deny => {
            model
                .notice
                .set("Implementation denied by the effective director profile".into());
            return;
        }
        Permission::Ask if !model.worker_approval.get_untracked() => {
            state.approval_needed = true;
            model.buffer.set(state);
            return;
        }
        _ => {}
    }
    if let Err(error) = validate_parts(&snapshot, &session_id, &doc.parts) {
        model.notice.set(error);
        return;
    }
    doc.finalize = Some(std::mem::take(&mut doc.parts));
    doc.changed = Instant::now() - Duration::from_secs(1);
    state.approval_needed = false;
    commit(model, state);
    flush(model);
}

pub fn acknowledge(model: Model, submitted: &Draft, id: &str) {
    let mut state = model.buffer.get_untracked();
    if let Some(doc) = state.documents.get_mut(&submitted.session_id) {
        doc.submitting = false;
        if !doc.conflict {
            if !doc.error.is_empty() && doc.parts == submitted.parts {
                doc.parts.clear();
            }
            doc.error.clear();
        }
        doc.editing_queue = None;
        if doc.remote.revision <= submitted.revision + 1 {
            doc.remote = Draft {
                session_id: submitted.session_id.clone(),
                revision: submitted.revision + 1,
                parts: vec![],
            };
        }
        doc.changed = Instant::now();
        if doc.force_after_submit {
            state
                .promote_after_ack
                .insert(submitted.session_id.clone(), id.into());
            doc.force_after_submit = false;
        }
    }
    state
        .last_queued
        .insert(submitted.session_id.clone(), id.into());
    model.worker_approval.set(false);
    commit(model, state);
}

pub fn receive(model: Model, update: Update) {
    let mut state = model.buffer.get_untracked();
    if update.serial <= state.serial {
        return;
    }
    state.serial = update.serial;
    state.initialized |= update.initialized;
    state.connected = update.connected;
    let mut changed = false;
    let mut forget = vec![];
    for (id, outcome) in &update.outcomes {
        forget.push(id.clone());
        match outcome {
            Outcome::Saved {
                session,
                request,
                result,
            } => {
                let Some(doc) = state.documents.get_mut(session).filter(|d| {
                    d.saving
                        .as_ref()
                        .is_some_and(|r| r.request_id == request.request_id)
                }) else {
                    continue;
                };
                match result {
                    Ok(draft) => {
                        if draft.revision >= doc.remote.revision {
                            doc.remote = draft.clone();
                        }
                        doc.saving = None;
                        doc.error.clear();
                    }
                    Err((conflict, error)) => {
                        doc.error = error.clone();
                        doc.conflict = *conflict;
                        if *conflict {
                            doc.saving = None;
                            if let Some(parts) = doc.finalize.take() {
                                doc.recovery.push(parts);
                                doc.recovery_reviewed = false;
                            }
                        }
                    }
                }
                changed = true;
            }
            Outcome::Fetched { id, result } => {
                state.fetching.remove(id);
                if let Err(error) = result {
                    state.fetch_errors.insert(id.clone(), error.clone());
                } else {
                    state.fetch_errors.remove(id);
                }
                changed = true;
            }
            Outcome::Uploaded { asset, result } => {
                if result.is_ok() {
                    state.uploads.remove(&asset.id);
                    state.failed_uploads.remove(&asset.id);
                } else {
                    state.failed_uploads.insert(asset.id.clone());
                    model.notice.set(result.as_ref().unwrap_err().clone());
                }
                changed = true;
            }
        }
    }
    for remote in &update.drafts {
        let doc = state
            .documents
            .entry(remote.session_id.clone())
            .or_insert_with(|| {
                changed = true;
                Document::new(remote.clone())
            });
        if remote.revision <= doc.remote.revision {
            continue;
        }
        let own_save = doc.saving.as_ref().is_some_and(|request| {
            request.expected_revision + 1 == remote.revision && request.parts == remote.parts
        });
        if own_save {
            doc.remote = remote.clone();
            doc.saving = None;
            doc.error.clear();
        } else if doc.parts == doc.remote.parts
            && doc.finalize.is_none()
            && !doc.submitting
            && doc.saving.is_none()
        {
            doc.parts = remote.parts.clone();
            doc.remote = remote.clone();
        } else if doc.submitting && remote.parts.is_empty() {
            doc.remote = remote.clone();
        } else {
            doc.remote = remote.clone();
            doc.conflict = true;
            doc.error = "Shared draft changed; local content is retained".into();
        }
        changed = true;
    }
    for (id, bytes) in update.blobs {
        if !state.blobs.contains_key(&id) {
            state.blobs.insert(id.clone(), bytes);
            state.fetching.remove(&id);
            changed = true;
        }
    }
    if let Some(requests) = model.buffer_requests.get_untracked()
        && !forget.is_empty()
    {
        let _ = requests.send(Request::Forget(forget));
    }
    let connection_changed = model.buffer.get_untracked().connected != state.connected
        || model.buffer.get_untracked().initialized != state.initialized;
    if changed {
        commit(model, state);
    } else if connection_changed {
        model.buffer.set(state);
    }
    flush(model);
}

pub fn flush(model: Model) {
    let Some(requests) = model.buffer_requests.get_untracked() else {
        return;
    };
    let mut state = model.buffer.get_untracked();
    if !state.initialized || !state.connected {
        return;
    }
    if !model.busy.get_untracked()
        && let Some((session, id)) = state
            .promote_after_ack
            .iter()
            .next()
            .map(|(s, id)| (s.clone(), id.clone()))
    {
        state.promote_after_ack.remove(&session);
        let snapshot = model.snapshot.get_untracked();
        let queued = snapshot.submissions.iter().any(|s| {
            s.id == id && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
        });
        model.buffer.set(state);
        if queued {
            let active = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session)
                .and_then(|s| s.worker.as_ref())
                .is_some_and(|w| matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued));
            let active_run_id = active
                .then(|| {
                    snapshot
                        .messages
                        .iter()
                        .rev()
                        .find(|m| m.session_id == session && m.kind == "prompt")
                        .and_then(|m| m.id.strip_prefix("prompt-"))
                        .map(str::to_owned)
                })
                .flatten();
            model.submit(
                Command::PromoteTurn {
                    submission_id: id,
                    active_run_id,
                },
                snapshot.revision,
                Saved::Action,
            );
        }
        return;
    }
    let mut changed = false;
    let mut submit = None;
    let snapshot = model.snapshot.get_untracked();
    for (session, doc) in &mut state.documents {
        if session.starts_with("director-draft-") {
            continue;
        }
        if doc.conflict || !doc.error.is_empty() || doc.submitting || doc.saving.is_some() {
            continue;
        }
        let target = doc.finalize.as_ref().unwrap_or(&doc.parts);
        if assets(target)
            .iter()
            .any(|a| state.uploads.contains_key(&a.id))
        {
            continue;
        }
        if doc.finalize.is_some()
            && *target == doc.remote.parts
            && !model.busy.get_untracked()
            && !model.can_retry()
        {
            let parts = target.clone();
            clear_stale_queue_edit(doc, &snapshot);
            submit = Some((
                Draft {
                    session_id: session.clone(),
                    revision: doc.remote.revision,
                    parts,
                },
                doc.editing_queue.clone(),
            ));
            doc.finalize = None;
            doc.submitting = true;
            changed = true;
            break;
        }
        if *target != doc.remote.parts && doc.changed.elapsed() >= Duration::from_millis(400) {
            let request = SaveDraft {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: doc.remote.revision,
                parts: target.clone(),
            };
            if requests
                .send(Request::Save {
                    session: session.clone(),
                    request: request.clone(),
                })
                .is_ok()
            {
                doc.saving = Some(request);
                changed = true;
            }
        }
    }
    if changed {
        commit(model, state);
    }
    if let Some((draft, editing)) = submit {
        let command = if let Some(submission_id) = editing {
            Command::EditQueuedTurn {
                submission_id,
                draft_revision: draft.revision,
                parts: draft.parts.clone(),
                approve_implementation: model.worker_approval.get_untracked(),
            }
        } else {
            Command::SubmitTurn {
                session_id: draft.session_id.clone(),
                draft_revision: draft.revision,
                parts: draft.parts.clone(),
                approve_implementation: model.worker_approval.get_untracked(),
            }
        };
        model.submit(
            command,
            model.snapshot.get_untracked().revision,
            Saved::Buffer(draft),
        );
    }
}

pub fn edit_queued(model: Model, id: &str) {
    let snapshot = model.snapshot.get_untracked();
    let Some(submission) = snapshot.submissions.iter().find(|s| {
        s.id == id && matches!(s.state, SubmissionState::Queued | SubmissionState::Paused)
    }) else {
        return;
    };
    let mut state = model.buffer.get_untracked();
    let doc = state
        .documents
        .entry(submission.session_id.clone())
        .or_insert_with(|| {
            Document::new(Draft {
                session_id: submission.session_id.clone(),
                ..Default::default()
            })
        });
    if has_content(&doc.parts) || doc.finalize.is_some() || doc.submitting {
        model
            .notice
            .set("Finish the current draft before editing a queued message".into());
        return;
    }
    doc.parts = submission.parts.clone();
    doc.editing_queue = Some(id.into());
    doc.changed = Instant::now();
    commit(model, state);
}

fn clear_stale_queue_edit(doc: &mut Document, snapshot: &Snapshot) {
    if doc.editing_queue.as_ref().is_some_and(|id| {
        !snapshot.submissions.iter().any(|submission| {
            &submission.id == id
                && matches!(
                    submission.state,
                    SubmissionState::Queued | SubmissionState::Paused
                )
        })
    }) {
        doc.editing_queue = None;
    }
}

pub fn rejected(model: Model, draft: &Draft, error: &str) {
    let snapshot = model.snapshot.get_untracked();
    let mut state = model.buffer.get_untracked();
    if let Some(doc) = state.documents.get_mut(&draft.session_id) {
        doc.submitting = false;
        doc.force_after_submit = false;
        if !has_content(&doc.parts) {
            doc.parts = draft.parts.clone();
        } else if doc.parts != draft.parts && !doc.recovery.contains(&draft.parts) {
            doc.recovery.push(draft.parts.clone());
            doc.recovery_reviewed = false;
        }
        clear_stale_queue_edit(doc, &snapshot);
        doc.error = error.into();
    }
    commit(model, state);
}

pub fn resolve(model: Model, use_local: bool) {
    let mut state = model.buffer.get_untracked();
    if let Some(doc) = state.documents.get_mut(&model.session.get_untracked()) {
        if doc.submitting {
            model
                .notice
                .set("Wait for the message acknowledgement before resolving the draft".into());
            return;
        }
        if let Some(parts) = doc.finalize.take()
            && has_content(&parts)
            && parts != doc.parts
            && !doc.recovery.contains(&parts)
        {
            doc.recovery.push(parts);
        }
        if use_local {
            if let Some(recovered) = doc.recovery.pop() {
                if has_content(&doc.parts)
                    && doc.parts != recovered
                    && !doc.recovery.contains(&doc.parts)
                {
                    doc.recovery.push(doc.parts.clone());
                }
                doc.parts = recovered;
            }
        } else {
            if doc.parts != doc.remote.parts && !doc.recovery.contains(&doc.parts) {
                doc.recovery.push(doc.parts.clone());
            }
            doc.parts = doc.remote.parts.clone();
        }
        doc.conflict = false;
        doc.recovery_reviewed = true;
        doc.force_after_submit = false;
        doc.error.clear();
        doc.saving = None;
        clear_stale_queue_edit(doc, &model.snapshot.get_untracked());
        doc.changed = Instant::now() - Duration::from_secs(1);
    }
    commit(model, state);
    model.notice.set(
        if use_local {
            "Local draft restored. Ready to send."
        } else {
            "Shared draft loaded."
        }
        .into(),
    );
    flush(model);
}

pub fn retry_save(model: Model) {
    let mut state = model.buffer.get_untracked();
    if state
        .documents
        .get(&model.session.get_untracked())
        .is_some_and(|doc| {
            !doc.conflict
                && doc.saving.is_none()
                && !has_content(&doc.parts)
                && !doc.recovery.is_empty()
        })
    {
        resolve(model, true);
        return;
    }
    let mut retrying = false;
    if let Some(doc) = state.documents.get_mut(&model.session.get_untracked()) {
        if doc.conflict {
            model
                .notice
                .set("Choose Load shared or Restore local to resolve the draft conflict".into());
            return;
        }
        if let Some(request) = &doc.saving
            && let Some(requests) = model.buffer_requests.get_untracked()
        {
            let _ = requests.send(Request::Save {
                session: doc.remote.session_id.clone(),
                request: request.clone(),
            });
            retrying = true;
        }
        doc.error.clear();
        doc.recovery_reviewed = true;
        clear_stale_queue_edit(doc, &model.snapshot.get_untracked());
    }
    for id in &state.failed_uploads {
        if let Some(asset) = state.uploads.get(id)
            && let Some(bytes) = state.blobs.get(id)
            && let Some(requests) = model.buffer_requests.get_untracked()
        {
            let _ = requests.send(Request::Upload {
                asset: asset.clone(),
                bytes: bytes.clone(),
            });
            retrying = true;
        }
    }
    commit(model, state);
    model.notice.set(
        if retrying {
            "Retrying draft save…"
        } else {
            "Draft ready to send."
        }
        .into(),
    );
    flush(model);
}

pub fn fetch(model: Model, asset: &Asset) {
    let mut state = model.buffer.get_untracked();
    if !state.blobs.contains_key(&asset.id) && state.fetching.insert(asset.id.clone()) {
        if let Some(requests) = model.buffer_requests.get_untracked() {
            let _ = requests.send(Request::Fetch(asset.id.clone()));
        }
        model.buffer.set(state);
    }
}

pub fn insert_asset(
    model: Model,
    name: String,
    media_type: String,
    bytes: Vec<u8>,
) -> Result<Part, String> {
    if bytes.len() > ASSET_LIMIT {
        return Err("File exceeds 20 MiB".into());
    }
    if media_type.starts_with("image/") {
        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|_| "Invalid image")?;
        let (width, height) = reader
            .into_dimensions()
            .map_err(|_| "Unsupported or invalid image")?;
        if u64::from(width) * u64::from(height) > 32_000_000 {
            return Err("Image exceeds 32 megapixels".into());
        }
    }
    let asset = Asset {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        media_type,
        size: bytes.len() as u64,
    };
    let bytes = Arc::new(bytes);
    let mut state = model.buffer.get_untracked();
    state.uploads.insert(asset.id.clone(), asset.clone());
    state.blobs.insert(asset.id.clone(), bytes.clone());
    if let Some(requests) = model.buffer_requests.get_untracked() {
        let _ = requests.send(Request::Upload {
            asset: asset.clone(),
            bytes,
        });
    }
    commit(model, state);
    Ok(Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Asset { asset },
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn paste(model: Model) -> Result<Vec<Part>, String> {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(_) => {
            return model
                .ui
                .get_untracked()
                .clipboard_text()
                .map(|text| vec![Part::text(text)])
                .ok_or_else(|| "Cannot read the system clipboard".into());
        }
    };
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        let mut prepared = vec![];
        let mut total = 0u64;
        for path in files {
            // arboard 3.6 retains CR from Linux text/uri-list CRLF delimiters.
            let path = if !path.exists() {
                path.to_str()
                    .and_then(|text| text.strip_suffix('\r'))
                    .map(PathBuf::from)
                    .unwrap_or(path)
            } else {
                path
            };
            let file = std::fs::File::open(&path).map_err(|_| "Cannot read the copied file")?;
            let metadata = file
                .metadata()
                .map_err(|_| "Cannot inspect the copied file")?;
            if !metadata.is_file() {
                return Err("Paste files individually; directories are not imported".into());
            }
            if metadata.len() > ASSET_LIMIT as u64 {
                return Err("File exceeds 20 MiB".into());
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("Copied file has an invalid name")?
                .to_owned();
            if name.len() > 240
                || name
                    .chars()
                    .any(|c| c.is_control() || c == '/' || c == '\\')
            {
                return Err("Copied file name is invalid or too long".into());
            }
            let mut bytes = vec![];
            use std::io::Read;
            file.take((ASSET_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read the copied file")?;
            if bytes.len() > ASSET_LIMIT {
                return Err("File exceeds 20 MiB".into());
            }
            total += bytes.len() as u64;
            if total > DRAFT_ASSET_LIMIT {
                return Err("Copied files exceed 64 MiB".into());
            }
            let media = match image::guess_format(&bytes) {
                Ok(image::ImageFormat::Png) => "image/png",
                Ok(image::ImageFormat::Jpeg) => "image/jpeg",
                Ok(image::ImageFormat::WebP) => "image/webp",
                _ => "application/octet-stream",
            };
            prepared.push((name, media.to_owned(), bytes));
        }
        return prepared
            .into_iter()
            .map(|(name, media, bytes)| insert_asset(model, name, media, bytes))
            .collect();
    }
    if let Ok(image) = clipboard.get_image() {
        if image.width as u64 * image.height as u64 > 32_000_000 {
            return Err("Image exceeds 32 megapixels".into());
        }
        let image = image::RgbaImage::from_raw(
            image.width as u32,
            image.height as u32,
            image.bytes.into_owned(),
        )
        .ok_or("Invalid clipboard image")?;
        let mut bytes = std::io::Cursor::new(vec![]);
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|_| "Cannot encode the pasted image")?;
        return Ok(vec![insert_asset(
            model,
            "Pasted image.png".into(),
            "image/png".into(),
            bytes.into_inner(),
        )?]);
    }
    Ok(vec![Part::text(
        clipboard
            .get_text()
            .ok()
            .or_else(|| model.ui.get_untracked().clipboard_text())
            .ok_or("Clipboard has no text, image, or files")?,
    )])
}

#[cfg(target_arch = "wasm32")]
#[derive(Serialize, Deserialize)]
struct BrowserJournal {
    documents: BTreeMap<String, JournalDocument>,
    blobs: BTreeMap<String, Vec<u8>>,
}
#[cfg(target_arch = "wasm32")]
pub fn load_journal(model: Model, _settings: &Path, _config: &crate::network::Config) {
    let source = crate::browser::recovered();
    if source.is_empty() {
        return;
    }
    match serde_json::from_str::<BrowserJournal>(&source) {
        Ok(journal) => {
            let mut state = model.buffer.get_untracked();
            for (session, record) in journal.documents {
                let mut doc = Document::new(record.remote);
                doc.parts = record.parts;
                doc.recovery = record.recovery;
                doc.recovery_reviewed = record.recovery_reviewed;
                doc.saving = record.saving;
                doc.editing_queue = record.editing_queue;
                if doc.saving.is_some() {
                    doc.error =
                        "Recovered save awaiting confirmation; retry preserves its original ID"
                            .into();
                }
                state.documents.insert(session, doc);
            }
            state.blobs = journal
                .blobs
                .into_iter()
                .filter(|(_, bytes)| bytes.len() <= ASSET_LIMIT)
                .map(|(id, bytes)| (id, Arc::new(bytes)))
                .collect();
            let referenced = state
                .documents
                .values()
                .flat_map(|doc| {
                    assets(&doc.parts)
                        .into_iter()
                        .chain(assets(&doc.remote.parts))
                        .chain(doc.recovery.iter().flat_map(|p| assets(p)))
                })
                .cloned()
                .collect::<Vec<_>>();
            for asset in referenced {
                if uuid::Uuid::parse_str(&asset.id).is_err() {
                    continue;
                }
                if let Some(bytes) = state.blobs.get(&asset.id)
                    && bytes.len() as u64 == asset.size
                {
                    state.uploads.insert(asset.id.clone(), asset.clone());
                    // Immutable uploads may be retried; execution commands are never replayed.
                    if let Some(requests) = model.buffer_requests.get_untracked() {
                        let _ = requests.send(Request::Upload {
                            asset,
                            bytes: bytes.clone(),
                        });
                    }
                }
            }
            model.buffer.set(state);
        }
        Err(_) => model
            .notice
            .set("Cannot read recovered drafts; the IndexedDB record was preserved".into()),
    }
}
#[cfg(target_arch = "wasm32")]
fn persist_browser(model: Model, state: &BufferState) {
    let documents = state
        .documents
        .iter()
        .map(|(id, doc)| {
            let mut recovery = doc.recovery.clone();
            if let Some(parts) = &doc.finalize {
                recovery.push(parts.clone());
            }
            if doc.submitting && !doc.remote.parts.is_empty() {
                recovery.push(doc.remote.parts.clone());
            }
            (
                id.clone(),
                JournalDocument {
                    remote: doc.remote.clone(),
                    parts: doc.parts.clone(),
                    recovery,
                    recovery_reviewed: doc.recovery_reviewed
                        && doc.finalize.is_none()
                        && !doc.submitting,
                    saving: doc.saving.clone(),
                    editing_queue: doc.editing_queue.clone(),
                },
            )
        })
        .collect();
    let referenced = state
        .documents
        .values()
        .flat_map(|doc| {
            assets(&doc.parts)
                .into_iter()
                .chain(assets(&doc.remote.parts))
                .chain(doc.recovery.iter().flat_map(|p| assets(p)))
                .chain(doc.finalize.iter().flat_map(|p| assets(p)))
        })
        .map(|asset| asset.id.clone())
        .chain(state.uploads.keys().cloned())
        .collect::<BTreeSet<_>>();
    let blobs = state
        .blobs
        .iter()
        .filter(|(id, _)| referenced.contains(*id))
        .map(|(id, bytes)| (id.clone(), bytes.as_ref().clone()))
        .collect();
    match serde_json::to_string(&BrowserJournal { documents, blobs }) {
        Ok(source) => crate::browser::persist(source, model),
        Err(_) => model
            .notice
            .set("Cannot encode browser draft recovery".into()),
    }
}
#[cfg(target_arch = "wasm32")]
pub fn paste(model: Model) -> Result<Vec<Part>, String> {
    model
        .ui
        .get_untracked()
        .clipboard_text()
        .map(|text| vec![Part::text(text)])
        .ok_or_else(|| "Use the browser Paste action or choose files".into())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn validate_browser_recovery(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Ok(());
    }
    serde_json::from_str::<BrowserJournal>(source).map(|_|()).map_err(|_| "Cannot read browser draft recovery. The IndexedDB record was preserved; restore or back it up before restarting Relay.".into())
}
