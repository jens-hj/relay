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

pub(super) struct Notice {
    session: String,
    run: String,
    /// The notice waited for the child's own workers to end.
    held: bool,
}

/// Called once a run's task ends. Records a notice for the parent when the
/// child ended without `report_result`, or reported to a director parent,
/// unless it continues with another turn or was stopped by an ancestor. While
/// the child's own workers are still active its notice is held; their ends
/// either wake the child or release the held notice once its subtree is idle.
pub(super) fn finished(workspace: &Workspace, session: &str, run: &str) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let ancestor_stop = store.ancestor_stops.get(session).is_some_and(|r| r == run);
    if ancestor_stop {
        store.ancestor_stops.remove(session);
    }
    if store.closing {
        return Ok(());
    }
    let snapshot = workspace.snapshots.borrow().clone();
    let mut first = Vec::new();
    let mut queue = |store: &mut Store, parent: String, notice: Notice| {
        let batch = store.notices.entry(parent.clone()).or_default();
        if batch.is_empty() {
            first.push(parent);
        }
        batch.push(notice);
    };
    if !ancestor_stop
        && store.run_id(session)?.as_deref() == Some(run)
        && let Some((parent, _)) = due(&snapshot, session, run)
    {
        if busy(&snapshot, session) {
            store.held.insert(session.into(), run.into());
        } else {
            let notice = Notice {
                session: session.into(),
                run: run.into(),
                held: false,
            };
            queue(&mut store, parent, notice);
        }
    }
    // An ancestor's subtree only becomes idle after its descendants'. An
    // ancestor with a pending notice is about to be woken, so it keeps its own.
    for ancestor in snapshot.ancestors(session) {
        if busy(&snapshot, &ancestor.id) || store.notices.contains_key(&ancestor.id) {
            break;
        }
        let Some(held) = store.held.remove(&ancestor.id) else {
            continue;
        };
        if store.run_id(&ancestor.id)?.as_deref() == Some(held.as_str())
            && let Some((parent, _)) = due(&snapshot, &ancestor.id, &held)
        {
            let notice = Notice {
                session: ancestor.id.clone(),
                run: held,
                held: true,
            };
            queue(&mut store, parent, notice);
        }
    }
    drop(store);
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        for parent in first {
            let workspace = workspace.clone();
            handle.spawn(async move {
                tokio::time::sleep(WINDOW).await;
                if let Err(error) = flush(&workspace, &parent) {
                    eprintln!("Relay notice failed: {error}");
                }
            });
        }
    }
    Ok(())
}

/// The parent to notify about a child's ended run, and whether the run reported
/// a result. Results are announced only to directors, which do not block on
/// their workers; other parents await them with `wait_for_agents`.
fn due(snapshot: &Snapshot, session: &str, run: &str) -> Option<(String, bool)> {
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
    let reported = result(snapshot, session, run).is_some();
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
fn result<'a>(snapshot: &'a Snapshot, session: &str, run: &str) -> Option<&'a str> {
    snapshot
        .messages
        .iter()
        .rev()
        .find(|m| {
            m.session_id == session
                && m.kind == "agent_result"
                && m.tool.as_ref().is_some_and(|t| t.run_id == run)
        })
        .map(|m| match m.tool.as_ref().and_then(|t| t.detail.as_ref()) {
            Some(ToolDetail::Result {
                summary: Some(summary),
                ..
            }) => summary.as_str(),
            _ => "",
        })
}

fn bounded(text: &str) -> &str {
    let mut end = text.len().min(1000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn line(snapshot: &Snapshot, session: &Session, notice: &Notice) -> String {
    let worker = session.worker.as_ref().unwrap();
    let status = match worker.status {
        WorkerStatus::Completed => "completed",
        WorkerStatus::Failed => "failed",
        WorkerStatus::Stopped => "stopped",
        WorkerStatus::Interrupted => "interrupted",
        _ => "ended",
    };
    let summary = result(snapshot, &session.id, &notice.run);
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
        text.push_str(&format!("\n  Summary: {}", bounded(summary)));
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
fn flush(workspace: &Workspace, parent: &str) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let Some(batch) = store.notices.remove(parent) else {
        return Ok(());
    };
    if store.closing {
        return Ok(());
    }
    let mut snapshot = store.snapshot()?;
    let Some(target) = snapshot.sessions.iter().find(|s| s.id == parent) else {
        return Ok(());
    };
    if target.archived || target.fixture || target.worker.is_none() {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut reports = 0;
    for notice in &batch {
        // Skip children resumed, archived or otherwise moved on since ending.
        if store.run_id(&notice.session)?.as_deref() != Some(notice.run.as_str()) {
            continue;
        }
        if let Some((_, reported)) = due(&snapshot, &notice.session, &notice.run) {
            reports += usize::from(reported);
            let child = find(&snapshot, &notice.session).unwrap();
            lines.push(line(&snapshot, child, notice));
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
    let parts = vec![Part::text(&text)];
    let id = format!("{PREFIX}{}", uuid::Uuid::new_v4());
    let active = runtime::active(&target.worker.as_ref().unwrap().status);
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
    if held {
        submission.state = SubmissionState::Paused;
        submission.error = Some("Queue paused; resume to continue".into());
    } else if !active && !waiting {
        if let Err(error) = conversation::authorize(&snapshot, parent, false, &workspace.config) {
            submission.state = SubmissionState::Paused;
            submission.error = Some(error.to_string());
        } else {
            submission.state = SubmissionState::Launching;
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
    if launch {
        transaction
            .execute(
                "INSERT OR REPLACE INTO runs(session_id,run_id) VALUES(?1,?2)",
                params![parent, id],
            )
            .map_err(Error::internal)?;
    }
    transaction
        .execute(
            "UPDATE workspace SET snapshot=?1 WHERE id=1",
            [serde_json::to_string(&snapshot).map_err(Error::internal)?],
        )
        .map_err(Error::internal)?;
    transaction.commit().map_err(Error::internal)?;
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
