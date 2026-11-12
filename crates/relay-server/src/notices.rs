//! Relay notices to a parent agent when a child turn ends.
use super::*;

/// Children ending within this window share one parent turn.
const WINDOW: std::time::Duration = if cfg!(test) {
    std::time::Duration::from_millis(1000)
} else {
    std::time::Duration::from_secs(2)
};
/// Submission IDs with this prefix are Relay notices; their prompt is authored by Relay.
pub(super) const PREFIX: &str = "relay-notice-";

#[derive(Clone)]
pub(super) struct Notice {
    session: String,
    run: String,
    /// The notice waited for the child's own workers to end.
    held: bool,
}

/// Called inside the transaction publishing a terminal outcome. A tombstone
/// written by a stop command wins over terminal publication and all retries.
pub(super) fn record(
    db: &Connection,
    snapshot: &Snapshot,
    session: &str,
    run: &str,
) -> Result<(), Error> {
    let Some(child) = find(snapshot, session).filter(|s| !s.fixture) else {
        return Ok(());
    };
    let Some(parent) = child.parent_session_id.as_deref() else {
        return Ok(());
    };
    if child
        .worker
        .as_ref()
        .is_none_or(|w| runtime::active(&w.status))
    {
        return Ok(());
    }
    db.execute("INSERT INTO lifecycle_events(child_session,child_run,parent_session,disposition,held) VALUES(?1,?2,?3,'pending',?4) ON CONFLICT DO NOTHING", params![session,run,parent,busy(snapshot,session)]).map_err(Error::internal)?;
    Ok(())
}

pub(super) fn superseded(db: &Connection) -> Result<(), Error> {
    db.execute("UPDATE lifecycle_events SET disposition='suppressed',hold_reason='superseded_run' WHERE notice_id IS NULL AND disposition!='suppressed' AND EXISTS(SELECT 1 FROM runs WHERE session_id=child_session AND run_id!=child_run)", []).map_err(Error::internal)?;
    Ok(())
}

/// Suppress delivery to a stopped branch and terminal events of cascaded runs.
/// Insert run tombstones even before their terminal snapshot exists.
pub(super) fn suppress(
    db: &Connection,
    snapshot: &Snapshot,
    id: &str,
    own: bool,
    reason: &str,
) -> Result<(), Error> {
    let ids = snapshot.session_branch_ids(id);
    for target in &ids {
        db.execute("UPDATE lifecycle_events SET disposition='suppressed',hold_reason=?2 WHERE parent_session=?1 AND notice_id IS NULL", params![target,reason]).map_err(Error::internal)?;
        if own || target != id {
            if let Some(parent) =
                find(snapshot, target).and_then(|s| s.parent_session_id.as_deref())
            {
                db.execute("INSERT INTO lifecycle_events(child_session,child_run,parent_session,disposition,hold_reason) SELECT ?1,run_id,?2,'suppressed',?3 FROM runs WHERE session_id=?1 ON CONFLICT(child_session,child_run,parent_session) DO UPDATE SET disposition='suppressed',hold_reason=excluded.hold_reason WHERE lifecycle_events.notice_id IS NULL", params![target,parent,reason]).map_err(Error::internal)?;
            }
            db.execute("UPDATE lifecycle_events SET disposition='suppressed',hold_reason=?2 WHERE child_session=?1 AND notice_id IS NULL", params![target,reason]).map_err(Error::internal)?;
        }
    }
    Ok(())
}

pub(super) fn command(
    db: &Connection,
    snapshot: &Snapshot,
    command: &Command,
) -> Result<(), Error> {
    match command {
        Command::StopWorker {
            session_id,
            include_descendants,
        } => {
            db.execute("INSERT INTO lifecycle_holds(session_id,run_id,reason) SELECT session_id,run_id,'human_stop' FROM runs WHERE session_id=?1 ON CONFLICT(session_id) DO UPDATE SET run_id=excluded.run_id,reason=excluded.reason",[session_id]).map_err(Error::internal)?;
            if *include_descendants {
                suppress(db, snapshot, session_id, false, "ancestor_stop")?;
            }
        }
        Command::ResumeQueue { session_id, .. }
        | Command::SubmitTurn { session_id, .. }
        | Command::SendWorker { session_id, .. } => {
            db.execute(
                "DELETE FROM lifecycle_holds WHERE session_id=?1",
                [session_id],
            )
            .map_err(Error::internal)?;
        }
        Command::PauseQueue { session_id, .. } => {
            db.execute("INSERT INTO lifecycle_holds(session_id,run_id,reason) SELECT session_id,run_id,'human_pause' FROM runs WHERE session_id=?1 ON CONFLICT(session_id) DO UPDATE SET run_id=excluded.run_id,reason=excluded.reason",[session_id]).map_err(Error::internal)?;
        }
        Command::SetSessionArchived {
            session_id,
            archived: true,
        } => suppress(db, snapshot, session_id, true, "archived_lineage")?,
        Command::DeleteSessionBranch { session_id } => {
            suppress(db, snapshot, session_id, true, "deleted_lineage")?
        }
        Command::CancelTurn { submission_id } => {
            // Bound events retain their identity after explicit cancellation.
            db.execute(
                "UPDATE lifecycle_events SET hold_reason='human_cancel' WHERE notice_id=?1",
                [submission_id],
            )
            .map_err(Error::internal)?;
        }
        _ => {}
    }
    Ok(())
}

/// Rebuild disposable hints from the ledger, never from historical terminal
/// sessions. All run/lineage/descendant checks occur under the writer lock.
fn refresh(store: &mut Store, eligible_at: Option<u64>) -> Result<(), Error> {
    superseded(&store.connection)?;
    let snapshot = store.snapshot()?;
    let mut rows = {
        let mut query = store.connection.prepare("SELECT child_session,child_run,parent_session,held FROM lifecycle_events WHERE notice_id IS NULL AND disposition!='suppressed' AND (?1 IS NULL OR (attempts<4 AND next_attempt<=?1)) ORDER BY rowid").map_err(Error::internal)?;
        query
            .query_map([eligible_at], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    rows.sort_by_key(|(session, _, _, _)| std::cmp::Reverse(snapshot.ancestors(session).len()));
    store.notices.clear();
    store.held.clear();
    for (session, run, parent, previous) in rows {
        let child = find(&snapshot, &session);
        let target = find(&snapshot, &parent);
        let reason = if child.is_none() || target.is_none() {
            Some("deleted_lineage")
        } else if child.is_some_and(|s| s.archived)
            || target.is_some_and(|s| s.archived || s.fixture || s.worker.is_none())
        {
            Some("archived_lineage")
        } else if child.is_some_and(|s| {
            s.worker
                .as_ref()
                .is_some_and(|w| !runtime::active(&w.status))
        }) && due(&store.connection, &snapshot, &session, &run).is_none()
        {
            Some("reported_non_director")
        } else {
            None
        };
        if let Some(reason) = reason {
            store.connection.execute("UPDATE lifecycle_events SET disposition='suppressed',hold_reason=?4 WHERE child_session=?1 AND child_run=?2 AND parent_session=?3", params![session,run,parent,reason]).map_err(Error::internal)?;
            continue;
        }
        let child_delivery: bool = store.connection.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_events WHERE parent_session=?1 AND notice_id IS NULL AND disposition!='suppressed')", [&session], |r| r.get(0)).map_err(Error::internal)?;
        let bound_delivery = snapshot.submissions.iter().any(|s| {
            s.session_id == session
                && s.id.starts_with(PREFIX)
                && matches!(
                    s.state,
                    SubmissionState::Queued
                        | SubmissionState::Paused
                        | SubmissionState::Launching
                        | SubmissionState::Running
                )
        });
        let held = busy(&snapshot, &session)
            || child_delivery
            || bound_delivery
            || child.is_some_and(|s| {
                s.worker
                    .as_ref()
                    .is_some_and(|w| runtime::active(&w.status))
            });
        let state = if held {
            "waiting_descendants"
        } else {
            "pending"
        };
        store.connection.execute("UPDATE lifecycle_events SET disposition=?4,hold_reason=CASE WHEN attempts>=4 THEN 'retry_exhausted' ELSE ?5 END,held=MAX(held,?6) WHERE child_session=?1 AND child_run=?2 AND parent_session=?3", params![session,run,parent,state,if held {Some("waiting_descendants")} else {None},held]).map_err(Error::internal)?;
        if held {
            store.held.insert(session, run);
        } else {
            store.notices.entry(parent).or_default().push(Notice {
                session,
                run,
                held: previous,
            });
        }
    }
    Ok(())
}

fn schedule(workspace: &Workspace, store: &mut Store) {
    if store.notice_dispatcher || store.closing {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    store.notice_dispatcher = true;
    let workspace = workspace.clone();
    handle.spawn(async move {
        let mut failures = 0;
        loop {
            tokio::time::sleep(WINDOW).await;
            let result = blocking(|| dispatch(&workspace));
            match result {
                Ok(true) => continue,
                Ok(false) => break,
                Err(_) => {
                    eprintln!("Lifecycle notice held: storage_retry");
                    failures += 1;
                    if failures < 4 {
                        tokio::time::sleep(std::time::Duration::from_secs(match failures {
                            1 => 2,
                            2 => 10,
                            _ => 60,
                        }))
                        .await;
                        continue;
                    }
                    if let Ok(mut store) = workspace.store.lock() {
                        store.notice_dispatcher = false;
                    }
                    break;
                }
            }
        }
    });
}

fn dispatch(workspace: &Workspace) -> Result<bool, Error> {
    let eligible_at = now();
    let parents = {
        let mut store = workspace.store.lock().map_err(Error::internal)?;
        if store.closing {
            store.notice_dispatcher = false;
            return Ok(false);
        }
        refresh(&mut store, Some(eligible_at))?;
        let mut query = store.connection.prepare("SELECT DISTINCT parent_session FROM lifecycle_events WHERE disposition='pending' AND notice_id IS NULL AND attempts<4 AND next_attempt<=?1 ORDER BY rowid LIMIT 32").map_err(Error::internal)?;
        query
            .query_map([eligible_at], |r| r.get::<_, String>(0))
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for parent in parents {
        flush_parent(workspace, &parent, true, true)?;
    }
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    // A delayed descendant hold is not refreshed until its delay expires.
    // Keep its timer alive even when its descendants ended during that delay.
    let more: bool = store.connection.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_events WHERE notice_id IS NULL AND attempts<4 AND (disposition='pending' OR (disposition='waiting_descendants' AND next_attempt>?1)))", [eligible_at], |r| r.get(0)).map_err(Error::internal)?;
    if !more {
        store.notice_dispatcher = false;
    }
    Ok(more)
}

/// Boot recovery binds unbound intent to review-only submissions. A binding is
/// permanent even when its native launch/consumption was never acknowledged.
pub(super) fn recover(workspace: &Workspace) -> Result<(), Error> {
    loop {
        let parents = {
            let mut store = workspace.store.lock().map_err(Error::internal)?;
            refresh(&mut store, None)?;
            store.notices.keys().take(32).cloned().collect::<Vec<_>>()
        };
        if parents.is_empty() {
            return Ok(());
        }
        for parent in parents {
            flush_with_launch(workspace, &parent, false)?;
        }
    }
}

/// Called once a run's task ends. Records a notice for the parent when the
/// child ended without `report_result`, or reported to a director parent,
/// unless it continues with another turn or was stopped by an ancestor. While
/// the child's own workers are still active its notice is held; their ends
/// either wake the child or release the held notice once its subtree is idle.
pub(super) fn finished(workspace: &Workspace, session: &str, run: &str) -> Result<(), Error> {
    blocking(|| finished_persisted(workspace, session, run))
}

fn finished_persisted(workspace: &Workspace, session: &str, run: &str) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.ancestor_stops.get(session).is_some_and(|r| r == run) {
        store.ancestor_stops.remove(session);
    }
    if store.closing {
        return Ok(());
    }
    schedule(workspace, &mut store);
    refresh(&mut store, Some(now()))?;
    Ok(())
}

/// The parent to notify about a child's ended run, and whether the run reported
/// a result. Results are announced only to directors, which do not block on
/// their workers; other parents await them with `wait_for_agents`.
fn due(db: &Connection, snapshot: &Snapshot, session: &str, run: &str) -> Option<(String, bool)> {
    let child = find(snapshot, session)?;
    let parent = find(snapshot, child.parent_session_id.as_deref()?)?;
    if child.archived
        || child
            .worker
            .as_ref()
            .is_none_or(|w| runtime::active(&w.status))
    {
        return None;
    }
    let reported = result(db, session, run).is_some();
    (!reported || director(snapshot, parent)).then(|| (parent.id.clone(), reported))
}

fn find<'a>(snapshot: &'a Snapshot, id: &str) -> Option<&'a Session> {
    snapshot.sessions.iter().find(|s| s.id == id)
}

/// Whether any descendant of a session is still queued or running.
fn busy(snapshot: &Snapshot, session: &str) -> bool {
    snapshot.descendants(session).iter().any(|s| {
        s.worker
            .as_ref()
            .is_some_and(|w| runtime::active(&w.status))
    })
}

fn director(snapshot: &Snapshot, session: &Session) -> bool {
    agent_tools::profile(snapshot, session).is_ok_and(|profile| {
        profile.permissions.get(&Task::Delegate) == Some(&Permission::Allow)
            && profile.responsibilities.contains(&Task::Delegate)
    })
}

/// The summary a child reported during `run`, if it reported.
fn result(db: &Connection, session: &str, run: &str) -> Option<String> {
    crate::messages::newest_matching(db, session, "agent_result", None, |m| {
        m.tool.as_ref().is_some_and(|t| t.run_id == run)
    })
    .ok()
    .flatten()
    .map(|m| match m.tool.and_then(|t| t.detail) {
        Some(ToolDetail::Result {
            summary: Some(summary),
            ..
        }) => summary,
        _ => String::new(),
    })
}

fn bounded(text: &str) -> &str {
    let mut end = text.len().min(1000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn line(db: &Connection, session: &Session, notice: &Notice) -> String {
    let worker = session.worker.as_ref().unwrap();
    let status = match worker.status {
        WorkerStatus::Completed => "completed",
        WorkerStatus::Failed => "failed",
        WorkerStatus::Stopped => "stopped",
        WorkerStatus::Interrupted => "interrupted",
        _ => "ended",
    };
    let summary = result(db, &session.id, &notice.run);
    let outcome = if summary.is_some() {
        "reported a result"
    } else {
        "no result reported"
    };
    let mut text = format!("- {} ({}): {status}, {outcome}", session.title, session.id);
    if notice.held {
        text.push_str("; its workers have since ended");
    }
    if let Some(summary) = summary.filter(|s| !s.is_empty()) {
        text.push_str(&format!("\n  Summary: {}", bounded(&summary)));
    }
    if let Some(error) = &worker.error {
        text.push_str(&format!("\n  Error: {}", bounded(error)));
    }
    if let Some(path) = &worker.worktree {
        text.push_str(&format!("\n  Worktree: {path}"));
    }
    if let Some(changes) = &worker.changes {
        text.push_str(&format!("\n  Changed files: {}", changes.files.len()));
    }
    text
}

/// Queue one notice turn for a parent. It starts only when the parent is idle
/// with an empty queue; a running parent reads it next, and a paused queue keeps
/// it held until a human resumes it.
#[cfg(test)]
fn flush(workspace: &Workspace, parent: &str) -> Result<(), Error> {
    flush_with_launch(workspace, parent, true)
}

fn flush_with_launch(workspace: &Workspace, parent: &str, allow_launch: bool) -> Result<(), Error> {
    flush_parent(workspace, parent, allow_launch, false)
}

fn flush_parent(
    workspace: &Workspace,
    parent: &str,
    allow_launch: bool,
    account_failure: bool,
) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    // One cutoff and exact key set own selection, binding, and failed-attempt
    // accounting. Only boot review recovery deliberately ignores retry holds.
    let eligible_at = allow_launch.then(now);
    refresh(&mut store, eligible_at)?;
    let Some(batch) = store.notices.get(parent).cloned() else {
        return Ok(());
    };
    let result = flush_batch(
        workspace,
        &mut store,
        parent,
        allow_launch,
        &batch,
        eligible_at,
    );
    if account_failure && result.is_err() {
        let transaction = store.connection.transaction().map_err(Error::internal)?;
        for event in &batch {
            // Never charge a sibling admitted after selection or a binding
            // whose commit acknowledgement was ambiguous.
            transaction.execute("UPDATE lifecycle_events SET attempts=attempts+1,next_attempt=?4+CASE attempts WHEN 0 THEN 2 WHEN 1 THEN 10 ELSE 60 END,hold_reason=CASE WHEN attempts>=3 THEN 'retry_exhausted' ELSE 'storage_retry' END WHERE child_session=?1 AND child_run=?2 AND parent_session=?3 AND notice_id IS NULL AND disposition='pending' AND attempts<4 AND next_attempt<=?5",params![event.session,event.run,parent,now(),eligible_at]).map_err(Error::internal)?;
        }
        transaction.commit().map_err(Error::internal)?;
        eprintln!("Lifecycle notice held: storage_retry");
        return Ok(());
    }
    result
}

fn flush_batch(
    workspace: &Workspace,
    store: &mut Store,
    parent: &str,
    allow_launch: bool,
    batch: &[Notice],
    eligible_at: Option<u64>,
) -> Result<(), Error> {
    if store.closing {
        return Ok(());
    }
    let snapshot = store.snapshot()?;
    let Some(target) = snapshot.sessions.iter().find(|s| s.id == parent) else {
        return Ok(());
    };
    if target.archived || target.fixture || target.worker.is_none() {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut reports = 0;
    for notice in batch {
        // Skip children resumed, archived or otherwise moved on since ending.
        if store.run_id(&notice.session)?.as_deref() != Some(notice.run.as_str()) {
            continue;
        }
        if let Some((_, reported)) = due(&store.connection, &snapshot, &notice.session, &notice.run)
        {
            reports += usize::from(reported);
            let child = find(&snapshot, &notice.session).unwrap();
            lines.push(line(&store.connection, child, notice));
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    let heading = match (lines.len(), reports) {
        (1, 0) => "Relay: a worker ended without reporting a result.".to_owned(),
        (1, _) => "Relay: a worker reported a result.".to_owned(),
        (count, 0) => format!("Relay: {count} workers ended without reporting a result."),
        (count, reports) if reports == count => {
            format!("Relay: {count} workers reported results.")
        }
        (count, _) => format!("Relay: {count} workers ended their turns."),
    };
    let mut guidance = Vec::new();
    if reports > 0 {
        guidance.push(
            "Review each reported result, then accept and archive the worker or send a follow-up.",
        );
    }
    if reports < lines.len() {
        guidance.push(if reports == 0 {
            "Inspect each worker, then resume it, send a follow-up, or accept and archive it."
        } else {
            "Inspect each worker without a result, then resume it, send a follow-up, or accept and archive it."
        });
    }
    guidance.push("Complete cleanup of successful, failed, accidental and superseded inactive workers once no active or follow-up work remains. Archive useful history and evidence by default; archiving a failed attempt does not accept its fix. Delete a disposable archived branch only after retaining needed handoff and evidence elsewhere; deletion irreversibly removes contents and owned isolated workspaces.");
    if target.parent_session_id.is_some() {
        guidance.push(
            "When your delegated work is accepted, call report_result to report it to your parent.",
        );
    }
    let text = format!(
        "{heading}\n\n{}\n\n{}",
        lines.join("\n"),
        guidance.join(" ")
    );
    let id = format!("{PREFIX}{}", uuid::Uuid::new_v4());
    enqueue(
        workspace,
        store,
        snapshot,
        parent,
        Delivery {
            text,
            id,
            attempt: None,
            allow_launch,
            lifecycle: batch,
            eligible_at,
        },
    )
}

pub(super) struct Delivery<'a> {
    pub text: String,
    pub id: String,
    pub attempt: Option<&'a str>,
    pub allow_launch: bool,
    pub lifecycle: &'a [Notice],
    pub eligible_at: Option<u64>,
}

/// Persist an outcome notice and its outbox receipt atomically. Recovery may
/// hold it for explicit continuation rather than starting a harness at boot.
pub(super) fn enqueue(
    workspace: &Workspace,
    store: &mut Store,
    mut snapshot: Snapshot,
    parent: &str,
    delivery: Delivery<'_>,
) -> Result<(), Error> {
    let Delivery {
        text,
        id,
        attempt,
        allow_launch,
        lifecycle,
        eligible_at,
    } = delivery;
    if allow_launch && !lifecycle.is_empty() && eligible_at.is_none() {
        return Err(Error::invalid("Lifecycle delivery requires a retry cutoff"));
    }
    let Some(target) = find(&snapshot, parent) else {
        return Ok(());
    };
    if target.archived || target.fixture || target.worker.is_none() {
        return Ok(());
    }
    let parts = vec![Part::text(&text)];
    let active = runtime::active(&target.worker.as_ref().unwrap().status);
    let mut stop_hold: Option<String> = store
        .connection
        .query_row(
            "SELECT reason FROM lifecycle_holds WHERE session_id=?1",
            [parent],
            |r| r.get(0),
        )
        .optional()
        .map_err(Error::internal)?;
    // Schema29 had no durable empty-queue stop hold. Conservatively retain a
    // stopped parent's notice for review rather than infer permission to wake.
    if stop_hold.is_none() && target.worker.as_ref().unwrap().status == WorkerStatus::Stopped {
        stop_hold = Some("stopped_parent".into());
    }
    let held = snapshot
        .submissions
        .iter()
        .any(|s| s.session_id == parent && s.state == SubmissionState::Paused);
    let waiting = snapshot
        .submissions
        .iter()
        .any(|s| s.session_id == parent && s.state == SubmissionState::Queued);
    let mut submission = Submission {
        id: id.clone(),
        session_id: parent.into(),
        parts: parts.clone(),
        state: SubmissionState::Queued,
        approve_implementation: false,
        error: None,
        interrupts_run: None,
        last_edit_request: None,
        sender: Sender::User,
    };
    let mut contended = false;
    if held || stop_hold.is_some() || !allow_launch {
        submission.state = SubmissionState::Paused;
        submission.error = Some("Queue paused; resume to continue".into());
    } else if !active && !waiting {
        match conversation::authorize(
            &store.connection,
            &snapshot,
            parent,
            false,
            &workspace.config,
        ) {
            Err(error) => {
                submission.state = SubmissionState::Paused;
                submission.error = Some(error.to_string());
            }
            Ok(true) => submission.state = SubmissionState::Launching,
            // Stays queued until the shared workspace is released.
            Ok(false) => contended = true,
        }
    }
    let launch = submission.state == SubmissionState::Launching;
    snapshot.submissions.push(submission);
    if launch {
        conversation::reserve(&mut snapshot, parent, &id, &parts);
    }
    snapshot.revision = snapshot
        .revision
        .checked_add(1)
        .ok_or_else(|| Error::invalid("Revision exhausted"))?;
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(Error::internal)?;
    if let Some(attempt) = attempt {
        transaction
            .execute(
                "UPDATE spawn_attempts SET notice_id=?1 WHERE id=?2 AND notice_id IS NULL",
                params![id, attempt],
            )
            .map_err(Error::internal)?;
    }
    for event in lifecycle {
        let changed = transaction.execute("UPDATE lifecycle_events SET disposition='bound',notice_id=?4,hold_reason=?5 WHERE child_session=?1 AND child_run=?2 AND parent_session=?3 AND notice_id IS NULL AND disposition='pending' AND (?6 IS NULL OR (attempts<4 AND next_attempt<=?6))", params![event.session,event.run,parent,id,if !allow_launch {Some("restart_review")} else if stop_hold.is_some() {stop_hold.as_deref()} else if held {Some("human_pause")} else {None},eligible_at]).map_err(Error::internal)?;
        if changed != 1 {
            return Err(Error::invalid("Lifecycle event binding changed"));
        }
    }
    if contended {
        conversation::wait_for_workspace(&transaction, &mut snapshot, parent)?;
    }
    if launch {
        transaction
            .execute(
                "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                params![parent, id],
            )
            .map_err(Error::internal)?;
    }
    superseded(&transaction)?;
    let snapshot = crate::persist(
        &store.snapshot_cache,
        &store.transcripts,
        &transaction,
        snapshot,
    )?;
    transaction.commit().map_err(Error::internal)?;
    store.notices.remove(parent);
    workspace.snapshots.send_replace(snapshot);
    if launch {
        let (sender, receiver) = watch::channel(false);
        store.controls.insert(parent.into(), sender);
        tokio::spawn(runtime::run(
            workspace.clone(),
            parent.into(),
            id,
            text,
            receiver,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod outbox_follow_on_regressions {
    use super::*;

    // Synthetic stores only. Reopening a distinct Store discards every memory
    // notice/held map and models the named server-loss cut without a live kill.
    fn fixture(path: &std::path::Path) -> Workspace {
        let mut snapshot = crate::tests::live();
        let mut parent = snapshot.sessions[0].clone();
        parent.id = "outbox-parent".into();
        parent.parent_session_id = None;
        parent.fixture = false;
        parent.archived = false;
        parent.workspaces.clear();
        parent.worker = Some(WorkerRun {
            status: WorkerStatus::Running,
            harness: Harness::Codex,
            thread_id: Some("synthetic-thread".into()),
            worktree: None,
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
            model: None,
            context_tokens: None,
            context_window: None,
            last_usage: None,
            execution: None,
            launched_network_policy: None,
        });
        let mut child = parent.clone();
        child.id = "outbox-child".into();
        child.parent_session_id = Some(parent.id.clone());
        snapshot.sessions = vec![parent, child];
        snapshot.messages.clear();
        snapshot.submissions.clear();
        snapshot.tool_permissions.clear();
        let workspace = crate::tests::workspace(path, &snapshot, RuntimeConfig::default());
        workspace.store.lock().unwrap().connection.execute(
            "INSERT INTO runs(session_id,run_id) VALUES('outbox-parent','parent-run'),('outbox-child','child-run')", []
        ).unwrap();
        workspace
            .update_run("outbox-child", "child-run", |s| {
                s.sessions
                    .iter_mut()
                    .find(|s| s.id == "outbox-child")
                    .unwrap()
                    .worker
                    .as_mut()
                    .unwrap()
                    .status = WorkerStatus::Completed;
                Ok(())
            })
            .unwrap();
        workspace
    }

    fn notice_turns(snapshot: &Snapshot) -> Vec<&Submission> {
        snapshot
            .submissions
            .iter()
            .filter(|s| s.session_id == "outbox-parent" && s.id.starts_with(PREFIX))
            .collect()
    }

    async fn reopen_at_cut(after_finished: bool) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.sqlite");
        let old = fixture(&path);
        if after_finished {
            finished(&old, "outbox-child", "child-run").unwrap();
            assert_eq!(old.store.lock().unwrap().notices["outbox-parent"].len(), 1);
        }
        // Do not yield to the old batch timer. A separately reopened server
        // must use committed storage, never the old process's memory maps.
        let (_, first) = router_with_shutdown(
            &path,
            "synthetic-reopen-token".into(),
            AgentProfile::default(),
            RuntimeConfig::default(),
        )
        .unwrap();
        let snapshot = first.workspace.snapshots.borrow().clone();
        assert_eq!(
            notice_turns(&snapshot).len(),
            1,
            "Committed terminal intent vanished across server reopen"
        );
        let id = notice_turns(&snapshot)[0].id.clone();
        assert_eq!(
            notice_turns(&snapshot)[0].state,
            SubmissionState::Paused,
            "Restart recovery must retain intent without automatically replaying work"
        );
        first.shutdown().await.unwrap();
        let (_, second) = router_with_shutdown(
            &path,
            "synthetic-reopen-token".into(),
            AgentProfile::default(),
            RuntimeConfig::default(),
        )
        .unwrap();
        let snapshot = second.workspace.snapshots.borrow().clone();
        assert_eq!(notice_turns(&snapshot).len(), 1);
        assert_eq!(
            notice_turns(&snapshot)[0].id,
            id,
            "Repeated reopen duplicated notice identity"
        );
        assert_eq!(notice_turns(&snapshot)[0].state, SubmissionState::Paused);
        second.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn terminal_commit_before_finished_survives_reopen_once() {
        reopen_at_cut(false).await;
    }

    #[tokio::test]
    async fn finished_before_batch_timer_survives_reopen_once() {
        reopen_at_cut(true).await;
    }

    #[tokio::test]
    async fn failed_flush_retains_intent_and_retry_binds_once() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("synthetic.sqlite"));
        finished(&workspace, "outbox-child", "child-run").unwrap();
        workspace.store.lock().unwrap().connection.execute_batch(
            "CREATE TRIGGER outbox_injected_failure BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic outbox failure'); END;"
        ).unwrap();
        assert!(flush(&workspace, "outbox-parent").is_err());
        workspace
            .store
            .lock()
            .unwrap()
            .connection
            .execute_batch("DROP TRIGGER outbox_injected_failure;")
            .unwrap();
        flush(&workspace, "outbox-parent").unwrap();
        let snapshot = workspace.snapshots.borrow().clone();
        assert_eq!(
            notice_turns(&snapshot).len(),
            1,
            "Failed flush discarded its only notice intent; retry delivered nothing"
        );
        let id = notice_turns(&snapshot)[0].id.clone();
        assert_eq!(notice_turns(&snapshot)[0].state, SubmissionState::Queued);
        flush(&workspace, "outbox-parent").unwrap();
        let snapshot = workspace.snapshots.borrow().clone();
        assert_eq!(notice_turns(&snapshot).len(), 1);
        assert_eq!(notice_turns(&snapshot)[0].id, id);
    }

    #[tokio::test]
    async fn repeated_terminal_notification_keeps_one_binding() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("synthetic.sqlite"));
        finished(&workspace, "outbox-child", "child-run").unwrap();
        flush(&workspace, "outbox-parent").unwrap();
        let snapshot = workspace.snapshots.borrow().clone();
        assert_eq!(notice_turns(&snapshot).len(), 1);
        let id = notice_turns(&snapshot)[0].id.clone();
        // Reconciliation may revisit the same terminal event after a delivery
        // acknowledgement is lost. That must not mean a second parent turn.
        finished(&workspace, "outbox-child", "child-run").unwrap();
        flush(&workspace, "outbox-parent").unwrap();
        let snapshot = workspace.snapshots.borrow().clone();
        assert_eq!(
            notice_turns(&snapshot).len(),
            1,
            "Revisiting one child run created a second parent notice turn"
        );
        assert_eq!(notice_turns(&snapshot)[0].id, id);
    }

    #[tokio::test]
    async fn terminal_snapshot_failure_rolls_back_event_and_revocation() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("db"));
        let (sender, _receiver) = watch::channel(false);
        {
            let mut store = workspace.store.lock().unwrap();
            let mut snapshot = store.full();
            snapshot.sessions[1].worker.as_mut().unwrap().status = WorkerStatus::Running;
            store.save(&snapshot).unwrap();
            workspace.snapshots.send_replace((snapshot).into());
            store.controls.insert("outbox-child".into(), sender);
            store.connection.execute_batch("DELETE FROM lifecycle_events; CREATE TRIGGER reject_terminal BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic terminal failure'); END;").unwrap();
        }
        let end = |workspace: &Workspace| {
            workspace.update_run("outbox-child", "child-run", |s| {
                s.sessions[1].worker.as_mut().unwrap().status = WorkerStatus::Completed;
                Ok(())
            })
        };
        assert!(end(&workspace).is_err());
        {
            let store = workspace.store.lock().unwrap();
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT COUNT(*) FROM lifecycle_events", [], |r| r
                        .get::<_, u32>(0))
                    .unwrap(),
                0
            );
            assert!(store.controls.contains_key("outbox-child"));
            assert_eq!(
                store.full().sessions[1].worker.as_ref().unwrap().status,
                WorkerStatus::Running
            );
            store
                .connection
                .execute_batch("DROP TRIGGER reject_terminal")
                .unwrap();
        }
        end(&workspace).unwrap();
        assert!(
            !workspace
                .store
                .lock()
                .unwrap()
                .controls
                .contains_key("outbox-child")
        );
        flush(&workspace, "outbox-parent").unwrap();
        assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_repeat_and_cancelled_binding_never_requeue() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("db"));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let workspace = workspace.clone();
            tasks.push(tokio::spawn(async move {
                finished(&workspace, "outbox-child", "child-run").unwrap();
                flush(&workspace, "outbox-parent").unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let id = notice_turns(&workspace.snapshots.borrow())[0].id.clone();
        assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 1);
        {
            let mut store = workspace.store.lock().unwrap();
            let snapshot = store.full();
            let (snapshot, _) = store
                .apply_owned(
                    crate::tests::env(
                        snapshot.revision,
                        Command::CancelTurn {
                            submission_id: id.clone(),
                        },
                    ),
                    &RuntimeConfig::default(),
                )
                .unwrap();
            workspace.snapshots.send_replace((snapshot).into());
        }
        recover(&workspace).unwrap();
        let snapshot = workspace.snapshots.borrow().clone();
        assert_eq!(notice_turns(&snapshot).len(), 1);
        assert_eq!(notice_turns(&snapshot)[0].id, id);
        assert_eq!(notice_turns(&snapshot)[0].state, SubmissionState::Cancelled);
    }

    #[tokio::test]
    async fn run_bound_stop_suppression_preserves_validation_receipts_and_cached_transactions() {
        for cascade in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let workspace = fixture(&directory.path().join("db"));
            let mut store = workspace.store.lock().unwrap();
            let before = store.full(); // Prime the autocommit cache.
            let (sender, _receiver) = watch::channel(false);
            store.controls.insert("outbox-parent".into(), sender);
            let command = Command::StopWorkerRuns {
                session_id: "outbox-parent".into(),
                include_descendants: cascade,
                runs: std::collections::BTreeMap::from([(
                    "outbox-parent".into(),
                    "parent-run".into(),
                )]),
                branch: if cascade {
                    vec!["outbox-child".into(), "outbox-parent".into()]
                } else {
                    vec!["outbox-parent".into()]
                },
            };
            let mut request = crate::tests::env(before.revision, command);
            let mut stale = request.clone();
            if let Command::StopWorkerRuns { runs, .. } = &mut stale.command {
                runs.insert("outbox-parent".into(), "replaced-run".into());
            }
            assert!(store.apply_owned(stale, &RuntimeConfig::default()).is_err());
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT COUNT(*) FROM lifecycle_holds", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "invalid run identity must fail before suppression"
            );
            store.connection.execute_batch("CREATE TRIGGER reject_stop BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic stop failure'); END;").unwrap();
            assert!(
                store
                    .apply_owned(request.clone(), &RuntimeConfig::default())
                    .is_err()
            );
            assert_eq!(
                store.full(),
                before,
                "rolled-back raw writes must not poison the snapshot cache"
            );
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT COUNT(*) FROM lifecycle_holds", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "hold and snapshot must roll back together"
            );
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT disposition FROM lifecycle_events", [], |r| r
                        .get::<_, String>(0))
                    .unwrap(),
                "pending"
            );
            store
                .connection
                .execute_batch("DROP TRIGGER reject_stop")
                .unwrap();
            let serialized_original = serde_json::to_string(&request).unwrap();
            let (after, _) = store
                .apply_owned(request.clone(), &RuntimeConfig::default())
                .unwrap();
            assert_eq!(after.protocol_version, 18);
            assert_ne!(after, before);
            assert_eq!(
                store.full(),
                after,
                "committed raw lifecycle writes must invalidate the cache"
            );
            assert_eq!(store.connection.query_row(
                "SELECT run_id,reason FROM lifecycle_holds WHERE session_id='outbox-parent'", [],
                |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?))
            ).unwrap(), ("parent-run".into(), "human_stop".into()));
            if cascade {
                assert_eq!(
                    store
                        .connection
                        .query_row("SELECT disposition FROM lifecycle_events", [], |r| r
                            .get::<_, String>(0))
                        .unwrap(),
                    "suppressed"
                );
            }
            assert_eq!(
                store
                    .connection
                    .query_row(
                        "SELECT request FROM receipts WHERE request_id=?1",
                        [&request.request_id],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                serialized_original,
                "receipts retain the original run-bound envelope"
            );
            let changes = store.connection.total_changes();
            assert_eq!(
                store
                    .apply_owned(request.clone(), &RuntimeConfig::default())
                    .unwrap()
                    .0,
                after
            );
            assert_eq!(
                store.connection.total_changes(),
                changes,
                "exact retry must not repeat lifecycle effects"
            );
            request.command = Command::StopWorker {
                session_id: "outbox-parent".into(),
                include_descendants: cascade,
            };
            assert!(
                store
                    .apply_owned(request, &RuntimeConfig::default())
                    .is_err(),
                "normalized command must not replace original receipt identity"
            );
        }
    }

    #[tokio::test]
    async fn command_suppression_is_atomic_and_survives_unarchive_and_delete() {
        for mode in ["stop", "cascade", "archive", "delete"] {
            let directory = tempfile::tempdir().unwrap();
            let workspace = fixture(&directory.path().join("db"));
            let mut store = workspace.store.lock().unwrap();
            let mut snapshot = store.full();
            if mode == "stop" || mode == "cascade" {
                let (sender, _receiver) = watch::channel(false);
                store.controls.insert("outbox-parent".into(), sender);
            } else {
                snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
                store.save(&snapshot).unwrap();
            }
            let cmd = if mode == "stop" || mode == "cascade" {
                Command::StopWorker {
                    session_id: "outbox-parent".into(),
                    include_descendants: mode == "cascade",
                }
            } else {
                Command::SetSessionArchived {
                    session_id: "outbox-parent".into(),
                    archived: true,
                }
            };
            store.connection.execute_batch("CREATE TRIGGER reject_command BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic command failure'); END;").unwrap();
            assert!(
                store
                    .apply_owned(
                        crate::tests::env(snapshot.revision, cmd.clone()),
                        &RuntimeConfig::default()
                    )
                    .is_err()
            );
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT disposition FROM lifecycle_events", [], |r| r
                        .get::<_, String>(0))
                    .unwrap(),
                "pending"
            );
            store
                .connection
                .execute_batch("DROP TRIGGER reject_command")
                .unwrap();
            let (mut snapshot, _) = store
                .apply_owned(
                    crate::tests::env(snapshot.revision, cmd),
                    &RuntimeConfig::default(),
                )
                .unwrap();
            if mode == "archive" || mode == "delete" {
                let cmd = if mode == "delete" {
                    Command::DeleteSessionBranch {
                        session_id: "outbox-parent".into(),
                    }
                } else {
                    Command::SetSessionArchived {
                        session_id: "outbox-parent".into(),
                        archived: false,
                    }
                };
                (snapshot, _) = store
                    .apply_owned(
                        crate::tests::env(snapshot.revision, cmd),
                        &RuntimeConfig::default(),
                    )
                    .unwrap();
            }
            workspace.snapshots.send_replace((snapshot).into());
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT disposition FROM lifecycle_events", [], |r| r
                        .get::<_, String>(0))
                    .unwrap(),
                if mode == "stop" {
                    "pending"
                } else {
                    "suppressed"
                }
            );
            drop(store);
            finished(&workspace, "outbox-child", "child-run").unwrap();
            recover(&workspace).unwrap();
            if mode == "stop" {
                assert_eq!(
                    notice_turns(&workspace.snapshots.borrow())[0].state,
                    SubmissionState::Paused
                );
            } else {
                assert!(notice_turns(&workspace.snapshots.borrow()).is_empty());
            }
        }
    }

    #[tokio::test]
    async fn current_policy_human_pause_and_supersession_hold_delivery() {
        for mode in ["pause", "deny", "ask", "supersede"] {
            let directory = tempfile::tempdir().unwrap();
            let mut workspace = fixture(&directory.path().join("db"));
            workspace.config = Arc::new(crate::tests::config());
            {
                let mut store = workspace.store.lock().unwrap();
                let mut snapshot = store.full();
                snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
                snapshot.sessions[0].worker.as_mut().unwrap().worktree =
                    Some(directory.path().display().to_string());
                if mode == "pause" {
                    snapshot.submissions.push(Submission {
                        id: "human-hold".into(),
                        session_id: "outbox-parent".into(),
                        parts: vec![Part::text("held")],
                        state: SubmissionState::Paused,
                        approve_implementation: false,
                        error: None,
                        interrupts_run: None,
                        last_edit_request: None,
                        sender: Sender::User,
                    });
                }
                let mut profile = snapshot.session_profile(&snapshot.sessions[0]).unwrap();
                profile.permissions.insert(
                    Task::Implement,
                    if mode == "ask" {
                        Permission::Ask
                    } else {
                        Permission::Deny
                    },
                );
                snapshot.sessions[0].profile = Some(profile);
                store.save(&snapshot).unwrap();
                workspace.snapshots.send_replace((snapshot).into());
                if mode == "supersede" {
                    let tx = store.connection.transaction().unwrap();
                    tx.execute(
                        "UPDATE runs SET run_id='new-run' WHERE session_id='outbox-child'",
                        [],
                    )
                    .unwrap();
                    superseded(&tx).unwrap();
                    tx.commit().unwrap();
                }
            }
            flush(&workspace, "outbox-parent").unwrap();
            let snapshot = workspace.snapshots.borrow().clone();
            if mode == "supersede" {
                assert!(notice_turns(&snapshot).is_empty());
            } else {
                assert_eq!(notice_turns(&snapshot)[0].state, SubmissionState::Paused);
                assert!(!notice_turns(&snapshot)[0].approve_implementation);
                if mode == "deny" {
                    assert_eq!(
                        notice_turns(&snapshot)[0].error.as_deref(),
                        Some("Current profile denies implementation")
                    );
                }
                if mode == "ask" {
                    assert_eq!(
                        notice_turns(&snapshot)[0].error.as_deref(),
                        Some("Implementation approval is required for this turn")
                    );
                }
                assert_eq!(
                    workspace
                        .store
                        .lock()
                        .unwrap()
                        .run_id("outbox-parent")
                        .unwrap()
                        .as_deref(),
                    Some("parent-run")
                );
            }
        }
    }

    #[test]
    fn migration_from_twenty_nine_preserves_data_without_historical_scan() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let workspace = fixture(&path);
        let snapshot = workspace.store.lock().unwrap().full();
        {
            let store = workspace.store.lock().unwrap();
            store.connection.execute_batch("DROP TABLE lifecycle_events; DROP TABLE lifecycle_holds; PRAGMA user_version=29; INSERT INTO drafts(session_id,draft) VALUES('synthetic-owner','{\"revision\":7,\"parts\":[]}');").unwrap();
        }
        drop(workspace);
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        assert_eq!(store.full(), snapshot);
        assert_eq!(
            store
                .connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            33
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM lifecycle_events", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            store.run_id("outbox-child").unwrap().as_deref(),
            Some("child-run")
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT draft FROM drafts WHERE session_id='synthetic-owner'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "{\"revision\":7,\"parts\":[]}"
        );
        store
            .connection
            .pragma_update(None, "user_version", 34)
            .unwrap();
        drop(store);
        assert!(Store::open(&path, AgentProfile::default()).is_err());
    }

    #[tokio::test]
    async fn committed_binding_before_task_spawn_stays_interrupted_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("db");
        let workspace = fixture(&path);
        flush(&workspace, "outbox-parent").unwrap();
        let id = notice_turns(&workspace.snapshots.borrow())[0].id.clone();
        // Deterministic cut: persist a queue reservation with no owned process,
        // representing loss after commit and before native task spawn.
        {
            let mut store = workspace.store.lock().unwrap();
            let mut snapshot = store.full();
            snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Queued;
            snapshot
                .submissions
                .iter_mut()
                .find(|s| s.id == id)
                .unwrap()
                .state = SubmissionState::Launching;
            let tx = store.connection.transaction().unwrap();
            tx.execute(
                "UPDATE runs SET run_id=?1 WHERE session_id='outbox-parent'",
                [&id],
            )
            .unwrap();
            tx.execute(
                "UPDATE workspace SET snapshot=?1",
                [serde_json::to_string(&snapshot).unwrap()],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        drop(workspace);
        for _ in 0..2 {
            let (_, shutdown) = router_with_shutdown(
                &path,
                "synthetic-reopen-token".into(),
                AgentProfile::default(),
                RuntimeConfig::default(),
            )
            .unwrap();
            let snapshot = shutdown.workspace.snapshots.borrow().clone();
            assert_eq!(notice_turns(&snapshot).len(), 1);
            assert_eq!(notice_turns(&snapshot)[0].id, id);
            assert_eq!(
                notice_turns(&snapshot)[0].state,
                SubmissionState::Interrupted
            );
            assert!(shutdown.workspace.store.lock().unwrap().controls.is_empty());
            shutdown.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn dispatcher_limits_parent_batches_and_exhausts_only_unbound_retry() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("db"));
        {
            let mut store = workspace.store.lock().unwrap();
            let mut snapshot = store.full();
            for index in 1..33 {
                let mut parent = snapshot.sessions[0].clone();
                parent.id = format!("extra-parent-{index}");
                let mut child = snapshot.sessions[1].clone();
                child.id = format!("extra-child-{index}");
                child.parent_session_id = Some(parent.id.clone());
                store
                    .connection
                    .execute(
                        "INSERT INTO runs(session_id,run_id) VALUES(?1,'child-run')",
                        [&child.id],
                    )
                    .unwrap();
                snapshot.sessions.push(parent);
                snapshot.sessions.push(child.clone());
                record(&store.connection, &snapshot, &child.id, "child-run").unwrap();
            }
            store.save(&snapshot).unwrap();
            workspace.snapshots.send_replace((snapshot).into());
        }
        assert!(dispatch(&workspace).unwrap());
        assert_eq!(workspace.snapshots.borrow().submissions.len(), 32);
        assert!(!dispatch(&workspace).unwrap());
        assert_eq!(workspace.snapshots.borrow().submissions.len(), 33);

        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("retry-db"));
        workspace.store.lock().unwrap().connection.execute_batch("CREATE TRIGGER reject_binding BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic binding failure'); END;").unwrap();
        for attempt in 1..=4 {
            assert_eq!(dispatch(&workspace).unwrap(), attempt < 4);
            let store = workspace.store.lock().unwrap();
            let (count, after): (u32, u64) = store
                .connection
                .query_row(
                    "SELECT attempts,next_attempt FROM lifecycle_events",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(count, attempt);
            assert!(after > now());
            store
                .connection
                .execute("UPDATE lifecycle_events SET next_attempt=0", [])
                .unwrap();
        }
        assert!(workspace.snapshots.borrow().submissions.is_empty());
        workspace
            .store
            .lock()
            .unwrap()
            .connection
            .execute_batch("DROP TRIGGER reject_binding")
            .unwrap();
        assert!(!dispatch(&workspace).unwrap());
        assert!(workspace.snapshots.borrow().submissions.is_empty());
        // Review-only boot recovery retains exhausted intent without retrying
        // native execution or silently releasing a human hold.
        recover(&workspace).unwrap();
        assert_eq!(
            notice_turns(&workspace.snapshots.borrow())[0].state,
            SubmissionState::Paused
        );
    }

    #[tokio::test]
    async fn failed_finished_refresh_still_schedules_bounded_delivery() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = fixture(&directory.path().join("db"));
        workspace.store.lock().unwrap().connection.execute_batch("CREATE TRIGGER reject_refresh BEFORE UPDATE ON lifecycle_events BEGIN SELECT RAISE(ABORT,'synthetic refresh failure'); END;").unwrap();
        assert!(finished(&workspace, "outbox-child", "child-run").is_err());
        assert!(workspace.store.lock().unwrap().notice_dispatcher);
        workspace
            .store
            .lock()
            .unwrap()
            .connection
            .execute_batch("DROP TRIGGER reject_refresh")
            .unwrap();
        let mut events = workspace.snapshots.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !notice_turns(&events.borrow()).is_empty() {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 1);
    }

    fn mixed_siblings(path: &std::path::Path, exhausted: bool) -> Workspace {
        let workspace = fixture(path);
        let mut store = workspace.store.lock().unwrap();
        let mut snapshot = store.full();
        let mut child = snapshot.sessions[1].clone();
        child.id = "due-sibling".into();
        snapshot.sessions.push(child.clone());
        store
            .connection
            .execute(
                "INSERT INTO runs(session_id,run_id) VALUES('due-sibling','sibling-run')",
                [],
            )
            .unwrap();
        record(&store.connection, &snapshot, &child.id, "sibling-run").unwrap();
        store.connection.execute("UPDATE lifecycle_events SET attempts=?1,next_attempt=?2,hold_reason=?3 WHERE child_session='outbox-child'", params![if exhausted {4} else {1}, if exhausted {0} else {now()+3600},if exhausted {"retry_exhausted"} else {"storage_retry"}]).unwrap();
        store.save(&snapshot).unwrap();
        workspace.snapshots.send_replace((snapshot).into());
        drop(store);
        workspace
    }

    fn held_sibling(
        workspace: &Workspace,
    ) -> (String, Option<String>, Option<String>, u32, u64, bool) {
        workspace.store.lock().unwrap().connection.query_row("SELECT disposition,notice_id,hold_reason,attempts,next_attempt,held FROM lifecycle_events WHERE child_session='outbox-child'", [],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap()
    }

    #[tokio::test]
    async fn mixed_sibling_binding_respects_exhaustion_and_future_delay() {
        for exhausted in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let workspace = mixed_siblings(&directory.path().join("db"), exhausted);
            let before = held_sibling(&workspace);
            dispatch(&workspace).unwrap();
            assert_eq!(
                held_sibling(&workspace),
                before,
                "A sibling must not release another event's retry hold"
            );
            let snapshot = workspace.snapshots.borrow().clone();
            assert_eq!(notice_turns(&snapshot).len(), 1);
            let due = notice_turns(&snapshot)[0].id.clone();
            assert_eq!(notice_turns(&snapshot)[0].state, SubmissionState::Queued);
            assert!(!plain_text(&notice_turns(&snapshot)[0].parts).contains("outbox-child"));
            // Manual same-boot flush also cannot bypass the per-event gate.
            flush(&workspace, "outbox-parent").unwrap();
            assert_eq!(held_sibling(&workspace), before);
            recover(&workspace).unwrap();
            let snapshot = workspace.snapshots.borrow().clone();
            assert_eq!(notice_turns(&snapshot).len(), 2);
            assert_eq!(
                snapshot
                    .submissions
                    .iter()
                    .find(|s| s.id == due)
                    .unwrap()
                    .state,
                SubmissionState::Queued
            );
            let recovered = snapshot.submissions.iter().find(|s| s.id != due).unwrap();
            assert_eq!(recovered.state, SubmissionState::Paused);
            assert_eq!(
                held_sibling(&workspace).1.as_deref(),
                Some(recovered.id.as_str())
            );
            recover(&workspace).unwrap();
            assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 2);
        }
    }

    #[tokio::test]
    async fn mixed_sibling_failure_charges_only_the_selected_due_event() {
        for exhausted in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let workspace = mixed_siblings(&directory.path().join("db"), exhausted);
            let before = held_sibling(&workspace);
            workspace.store.lock().unwrap().connection.execute_batch("CREATE TRIGGER mixed_failure BEFORE UPDATE ON workspace BEGIN SELECT RAISE(ABORT,'synthetic mixed failure'); END;").unwrap();
            dispatch(&workspace).unwrap();
            assert_eq!(
                held_sibling(&workspace),
                before,
                "Failed B must not charge or modify held A"
            );
            assert!(workspace.snapshots.borrow().submissions.is_empty());
            {
                let store = workspace.store.lock().unwrap();
                assert_eq!(store.connection.query_row("SELECT attempts FROM lifecycle_events WHERE child_session='due-sibling'",[],|r|r.get::<_,u32>(0)).unwrap(),1);
                store.connection.execute_batch("DROP TRIGGER mixed_failure; UPDATE lifecycle_events SET next_attempt=0 WHERE child_session='due-sibling';").unwrap();
            }
            dispatch(&workspace).unwrap();
            assert_eq!(held_sibling(&workspace), before);
            assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 1);
            recover(&workspace).unwrap();
            assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 2);
            assert_eq!(
                workspace
                    .snapshots
                    .borrow()
                    .submissions
                    .iter()
                    .filter(|s| s.state == SubmissionState::Paused)
                    .count(),
                1
            );
        }
    }

    #[tokio::test]
    async fn binding_transaction_rejects_a_stale_retry_batch() {
        for exhausted in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let workspace = mixed_siblings(&directory.path().join("db"), exhausted);
            let before = held_sibling(&workspace);
            let batch = [
                Notice {
                    session: "due-sibling".into(),
                    run: "sibling-run".into(),
                    held: false,
                },
                Notice {
                    session: "outbox-child".into(),
                    run: "child-run".into(),
                    held: false,
                },
            ];
            let mut store = workspace.store.lock().unwrap();
            let snapshot = store.full();
            assert!(
                enqueue(
                    &workspace,
                    &mut store,
                    snapshot,
                    "outbox-parent",
                    Delivery {
                        text: "Synthetic stale batch".into(),
                        id: "relay-notice-stale".into(),
                        attempt: None,
                        allow_launch: true,
                        lifecycle: &batch,
                        eligible_at: Some(now()),
                    }
                )
                .is_err()
            );
            assert_eq!(
                store
                    .connection
                    .query_row(
                        "SELECT COUNT(*) FROM lifecycle_events WHERE notice_id IS NOT NULL",
                        [],
                        |r| r.get::<_, u32>(0)
                    )
                    .unwrap(),
                0
            );
            assert!(store.full().submissions.is_empty());
            drop(store);
            assert_eq!(held_sibling(&workspace), before);
        }
    }

    #[tokio::test]
    async fn delayed_descendant_hold_keeps_timer_until_eligible() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = mixed_siblings(&directory.path().join("db"), false);
        workspace.store.lock().unwrap().connection.execute("UPDATE lifecycle_events SET disposition='waiting_descendants' WHERE child_session='outbox-child'",[]).unwrap();
        let before = held_sibling(&workspace);
        assert!(dispatch(&workspace).unwrap());
        assert_eq!(held_sibling(&workspace), before);
        // Advance the fixture's eligibility boundary without a fresh terminal
        // callback. The existing timer must still be able to deliver it.
        workspace
            .store
            .lock()
            .unwrap()
            .connection
            .execute(
                "UPDATE lifecycle_events SET next_attempt=0 WHERE child_session='outbox-child'",
                [],
            )
            .unwrap();
        assert!(!dispatch(&workspace).unwrap());
        assert_eq!(notice_turns(&workspace.snapshots.borrow()).len(), 2);
    }
}
