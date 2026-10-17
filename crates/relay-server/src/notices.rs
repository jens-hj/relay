//! Relay notices to a parent agent when a child turn ends without `report_result`.
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
}

/// Called once a run's task ends. Records a notice for the parent unless the
/// child reported, continues with another turn, or was stopped by an ancestor.
pub(super) fn finished(workspace: &Workspace, session: &str, run: &str) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    let ancestor_stop = store.ancestor_stops.get(session).is_some_and(|r| r == run);
    if ancestor_stop {
        store.ancestor_stops.remove(session);
    }
    if store.closing || ancestor_stop || store.run_id(session)?.as_deref() != Some(run) {
        return Ok(());
    }
    let snapshot = workspace.snapshots.borrow().clone();
    let Some(child) = snapshot.sessions.iter().find(|s| s.id == session) else {
        return Ok(());
    };
    let Some(parent) = child.parent_session_id.clone() else {
        return Ok(());
    };
    if child.archived
        || child
            .worker
            .as_ref()
            .is_none_or(|w| runtime::active(&w.status))
        || reported(&snapshot, session, run)
    {
        return Ok(());
    }
    let batch = store.notices.entry(parent.clone()).or_default();
    let first = batch.is_empty();
    batch.push(Notice {
        session: session.into(),
        run: run.into(),
    });
    drop(store);
    if first && let Ok(handle) = tokio::runtime::Handle::try_current() {
        let workspace = workspace.clone();
        handle.spawn(async move {
            tokio::time::sleep(WINDOW).await;
            if let Err(error) = flush(&workspace, &parent) {
                eprintln!("Relay notice failed: {error}");
            }
        });
    }
    Ok(())
}

fn reported(snapshot: &Snapshot, session: &str, run: &str) -> bool {
    snapshot.messages.iter().any(|m| {
        m.session_id == session
            && m.kind == "agent_result"
            && m.tool.as_ref().is_some_and(|t| t.run_id == run)
    })
}

fn line(session: &Session) -> String {
    let worker = session.worker.as_ref().unwrap();
    let status = match worker.status {
        WorkerStatus::Completed => "completed",
        WorkerStatus::Failed => "failed",
        WorkerStatus::Stopped => "stopped",
        WorkerStatus::Interrupted => "interrupted",
        _ => "ended",
    };
    let mut text = format!(
        "- {} ({}): {status}, no result reported",
        session.title, session.id
    );
    if let Some(error) = &worker.error {
        let mut end = error.len().min(1000);
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&format!("\n  Error: {}", &error[..end]));
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
    for notice in &batch {
        // Skip children resumed, archived or otherwise moved on since ending.
        if store.run_id(&notice.session)?.as_deref() != Some(notice.run.as_str()) {
            continue;
        }
        if let Some(child) = snapshot.sessions.iter().find(|s| s.id == notice.session)
            && !child.archived
            && child
                .worker
                .as_ref()
                .is_some_and(|w| !runtime::active(&w.status))
            && !reported(&snapshot, &notice.session, &notice.run)
        {
            lines.push(line(child));
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    let heading = if lines.len() == 1 {
        "Relay: a worker ended without reporting a result.".to_owned()
    } else {
        format!(
            "Relay: {} workers ended without reporting a result.",
            lines.len()
        )
    };
    let text = format!(
        "{heading}\n\n{}\n\nInspect each worker, then resume it, send a follow-up, or accept and archive it.",
        lines.join("\n")
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
