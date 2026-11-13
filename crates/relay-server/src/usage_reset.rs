//! Opt-in reset notices. Arm only future observed windows; persist delivery with
//! the queued turn, and hold overdue notices at startup for explicit review.
use super::*;

pub(super) const PREFIX: &str = "relay-usage-reset-";

fn eligible(snapshot: &Snapshot, session: &Session, harness: &str, window: &str) -> bool {
    if session.fixture
        || session.archived
        || session.worker.is_none()
        || snapshot.ancestors(&session.id).iter().any(|s| s.archived)
    {
        return false;
    }
    agent_tools::profile(snapshot, session).is_ok_and(|p| {
        let subscriptions = p.usage_reset_notifications();
        match (harness, window) {
            ("Codex", "five_hour") => subscriptions.codex_session,
            ("Claude Code", "five_hour") => subscriptions.claude_session,
            ("Claude Code", "weekly") => subscriptions.claude_weekly,
            _ => false,
        }
    })
}

impl Shutdown {
    pub fn start_usage_reset_notifications(&self) {
        let workspace = self.workspace.clone();
        tokio::spawn(async move {
            let mut changes = workspace.snapshots.subscribe();
            let mut shutdown = workspace.transport_shutdown.subscribe();
            let mut startup = true;
            loop {
                if *shutdown.borrow() {
                    break;
                }
                let result = tokio::task::block_in_place(|| tick(&workspace, now(), !startup));
                if result.is_err() {
                    eprintln!("Usage reset notification check failed");
                } else {
                    startup = false;
                }
                tokio::select! {
                    _ = shutdown.changed() => {},
                    _ = changes.changed() => {},
                    _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {},
                }
            }
        });
    }
}

pub(super) fn tick(workspace: &Workspace, at: u64, allow_launch: bool) -> Result<(), Error> {
    let mut store = workspace.store.lock().map_err(Error::internal)?;
    if store.closing {
        return Ok(());
    }
    let snapshot = store.snapshot()?;
    for limit in &snapshot.usage_limits {
        let Some(reset) = limit.resets_at.filter(|reset| *reset > at) else {
            continue;
        };
        for session in &snapshot.sessions {
            if eligible(
                &snapshot,
                session,
                limit.harness.label(),
                limit.window.key(),
            ) {
                let id = format!("{PREFIX}{}-{}-{}", session.id, limit.id, reset);
                store.connection.execute(
                    "INSERT OR IGNORE INTO usage_reset_notices(id,session_id,harness,window,resets_at) VALUES(?1,?2,?3,?4,?5)",
                    params![id,session.id,limit.harness.label(),limit.window.key(),reset],
                ).map_err(Error::internal)?;
            }
        }
    }
    let pending = {
        let mut query = store.connection.prepare("SELECT id,session_id,harness,window,resets_at FROM usage_reset_notices WHERE delivered=0").map_err(Error::internal)?;
        query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, u64>(4)?,
                ))
            })
            .map_err(Error::internal)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::internal)?
    };
    for (id, session, harness, window, reset) in pending {
        let snapshot = store.snapshot()?;
        let enabled = snapshot
            .sessions
            .iter()
            .find(|s| s.id == session)
            .is_some_and(|s| eligible(&snapshot, s, &harness, &window));
        if !enabled {
            store
                .connection
                .execute(
                    "UPDATE usage_reset_notices SET delivered=1 WHERE id=?1",
                    [&id],
                )
                .map_err(Error::internal)?;
            continue;
        }
        if reset > at {
            continue;
        }
        let label = if window == "weekly" {
            "weekly"
        } else {
            "session (five-hour)"
        };
        notices::enqueue(
            workspace,
            &mut store,
            snapshot,
            &session,
            notices::Delivery {
                text: format!(
                    "Relay: the reported {harness} {label} usage reset time has been reached. Reassess available usage and continue the existing task if appropriate. This notice does not grant additional permissions."
                ),
                id,
                attempt: None,
                allow_launch,
                lifecycle: &[],
                eligible_at: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &Path, harness: Harness) -> Workspace {
        let mut snapshot = crate::tests::live();
        let mut session = snapshot.sessions[0].clone();
        session.id = "reset-agent".into();
        session.parent_session_id = None;
        session.fixture = false;
        session.archived = false;
        session.worker = Some(WorkerRun {
            status: WorkerStatus::Running,
            harness,
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
        session.profile = Some(AgentProfile {
            harness,
            notify_session_usage_reset: true,
            notify_weekly_usage_reset: true,
            ..AgentProfile::default()
        });
        snapshot.sessions = vec![session];
        snapshot.submissions.clear();
        snapshot.usage_limits.clear();
        for window in [UsageWindow::FiveHour, UsageWindow::Weekly] {
            snapshot.record_usage_limit(UsageLimit::new(
                harness,
                window,
                95.0,
                300,
                Some(200),
                100,
            ));
        }
        crate::tests::workspace(path, &snapshot, RuntimeConfig::default())
    }

    #[test]
    fn reset_authorization_requires_delivery_and_recipient_and_submission() {
        let w = fixture(Path::new(":memory:"), Harness::Codex);
        tick(&w, 100, true).unwrap();
        tick(&w, 200, true).unwrap();
        let store = w.store.lock().unwrap();
        let mut s = store.snapshot().unwrap();
        let id = s.submissions[0].id.clone();
        assert!(notices::authorizes_run(&store.connection, &s, "reset-agent", &id).unwrap());
        assert!(!notices::authorizes_run(&store.connection, &s, "other-agent", &id).unwrap());
        store
            .connection
            .execute("UPDATE usage_reset_notices SET delivered=0", [])
            .unwrap();
        assert!(!notices::authorizes_run(&store.connection, &s, "reset-agent", &id).unwrap());
        store
            .connection
            .execute("UPDATE usage_reset_notices SET delivered=1", [])
            .unwrap();
        s.submissions.clear();
        assert!(!notices::authorizes_run(&store.connection, &s, "reset-agent", &id).unwrap());
    }

    #[test]
    fn startup_repairs_existing_reset_attribution_without_replaying_or_reordering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repair.sqlite");
        let w = fixture(&path, Harness::Codex);
        tick(&w, 100, true).unwrap();
        tick(&w, 200, true).unwrap();
        let (id, before) = {
            let mut store = w.store.lock().unwrap();
            let mut s = store.snapshot().unwrap();
            let id = s.submissions[0].id.clone();
            let parts = s.submissions[0].parts.clone();
            conversation::reserve(&mut s, "reset-agent", &id, &parts);
            s.messages[0].author = "You".into();
            store.save(&s).unwrap();
            (id, store.full())
        };
        drop(w);
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        let after = store.full();
        assert_eq!(after.submissions, before.submissions);
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.messages.len(), before.messages.len());
        let message = after
            .messages
            .iter()
            .find(|m| m.id == format!("prompt-{id}"))
            .unwrap();
        assert_eq!(message.author, "Relay");
        assert!(after.revision > before.revision);
        drop(store);
        let store = Store::open(&path, AgentProfile::default()).unwrap();
        assert_eq!(store.full(), after);
    }

    #[test]
    fn delegators_subscribe_to_other_harnesses_independently_and_persist_choices() {
        for harness in [Harness::Codex, Harness::ClaudeCode] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("cross-harness.sqlite");
            let w = fixture(&path, harness);
            let snapshot = {
                let mut store = w.store.lock().unwrap();
                let mut snapshot = store.snapshot().unwrap();
                snapshot.sessions[0]
                    .profile
                    .as_mut()
                    .unwrap()
                    .usage_reset_notifications = Some(UsageResetNotifications {
                    codex_session: true,
                    claude_session: true,
                    claude_weekly: false,
                });
                for source in [Harness::Codex, Harness::ClaudeCode] {
                    for window in [UsageWindow::FiveHour, UsageWindow::Weekly] {
                        snapshot.record_usage_limit(UsageLimit::new(
                            source,
                            window,
                            90.0,
                            300,
                            Some(200),
                            100,
                        ));
                    }
                }
                let mut child = snapshot.sessions[0].clone();
                child.id = "claude-child".into();
                child.parent_session_id = Some(snapshot.sessions[0].id.clone());
                child.worker.as_mut().unwrap().harness = Harness::ClaudeCode;
                child.profile.as_mut().unwrap().usage_reset_notifications =
                    Some(UsageResetNotifications::default());
                snapshot.sessions.push(child);
                store.save(&snapshot).unwrap();
                snapshot
            };
            drop(w);
            let w = crate::tests::workspace(&path, &snapshot, RuntimeConfig::default());
            tick(&w, 100, false).unwrap();
            tick(&w, 200, true).unwrap();
            tick(&w, 210, true).unwrap();
            let snapshot = w.store.lock().unwrap().snapshot().unwrap();
            assert_eq!(snapshot.submissions.len(), 2);
            assert!(
                snapshot
                    .submissions
                    .iter()
                    .all(|s| s.session_id == "reset-agent" && s.state == SubmissionState::Queued)
            );
            let prompts = serde_json::to_string(&snapshot.submissions).unwrap();
            assert!(prompts.contains("Codex") && prompts.contains("Claude Code"));
            assert!(!prompts.contains("weekly"));
            assert_eq!(
                snapshot.sessions[1].worker.as_ref().unwrap().status,
                WorkerStatus::Running
            );
        }
    }

    #[test]
    fn reset_notifications_split_windows_and_do_not_interrupt_running_agents() {
        for (harness, count) in [(Harness::Codex, 1), (Harness::ClaudeCode, 2)] {
            let w = fixture(Path::new(":memory:"), harness);
            tick(&w, 100, true).unwrap();
            assert!(
                w.store
                    .lock()
                    .unwrap()
                    .snapshot()
                    .unwrap()
                    .submissions
                    .is_empty()
            );
            tick(&w, 200, true).unwrap();
            tick(&w, 201, true).unwrap();
            let s = w.store.lock().unwrap().snapshot().unwrap();
            assert_eq!(s.submissions.len(), count);
            assert!(
                s.submissions
                    .iter()
                    .all(|s| s.state == SubmissionState::Queued)
            );
            assert_eq!(
                s.sessions[0].worker.as_ref().unwrap().status,
                WorkerStatus::Running
            );
        }
    }

    #[test]
    fn reset_notifications_survive_restart_and_hold_missed_resets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reset.sqlite");
        let w = fixture(&path, Harness::ClaudeCode);
        tick(&w, 100, true).unwrap();
        let snapshot = w.store.lock().unwrap().snapshot().unwrap();
        drop(w);
        let w = crate::tests::workspace(&path, &snapshot, RuntimeConfig::default());
        tick(&w, 210, false).unwrap();
        let snapshot = w.store.lock().unwrap().snapshot().unwrap();
        assert_eq!(snapshot.submissions.len(), 2);
        assert!(
            snapshot
                .submissions
                .iter()
                .all(|s| s.state == SubmissionState::Paused)
        );
        drop(w);
        let w = crate::tests::workspace(&path, &snapshot, RuntimeConfig::default());
        tick(&w, 220, true).unwrap();
        assert_eq!(
            w.store
                .lock()
                .unwrap()
                .snapshot()
                .unwrap()
                .submissions
                .len(),
            2
        );
    }

    #[test]
    fn reset_notifications_respect_stop_disable_and_archive() {
        for action in ["stop", "disable", "archive"] {
            let w = fixture(Path::new(":memory:"), Harness::Codex);
            tick(&w, 100, true).unwrap();
            {
                let mut store = w.store.lock().unwrap();
                let mut snapshot = store.snapshot().unwrap();
                match action {
                    "stop" => {
                        snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Stopped
                    }
                    "disable" => {
                        snapshot.sessions[0]
                            .profile
                            .as_mut()
                            .unwrap()
                            .notify_session_usage_reset = false
                    }
                    _ => snapshot.sessions[0].archived = true,
                }
                store.save(&snapshot).unwrap();
            }
            tick(&w, 200, true).unwrap();
            let snapshot = w.store.lock().unwrap().snapshot().unwrap();
            if action == "stop" {
                assert_eq!(snapshot.submissions.len(), 1);
                assert_eq!(snapshot.submissions[0].state, SubmissionState::Paused);
            } else {
                assert!(snapshot.submissions.is_empty());
            }
        }
    }

    #[test]
    fn reset_notifications_ignore_unknown_and_already_expired_windows() {
        let w = fixture(Path::new(":memory:"), Harness::Codex);
        tick(&w, 300, true).unwrap();
        assert!(
            w.store
                .lock()
                .unwrap()
                .snapshot()
                .unwrap()
                .submissions
                .is_empty()
        );
    }
}
