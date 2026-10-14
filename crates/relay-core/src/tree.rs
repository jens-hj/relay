//! Pure queries over the sub-agent tree formed by `Session::parent_session_id`.
//! Parent links come from persisted data, so every walk is cycle-safe.
use crate::{Session, Snapshot, ToolPermission, WorkerStatus};
use std::collections::BTreeSet;

/// Worker states across a subtree. Sessions without a run count as idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub idle: usize,
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub stopped: usize,
    pub interrupted: usize,
}

impl StatusCounts {
    pub fn add(&mut self, status: Option<&WorkerStatus>) {
        match status {
            None => self.idle += 1,
            Some(WorkerStatus::Queued) => self.queued += 1,
            Some(WorkerStatus::Running) => self.running += 1,
            Some(WorkerStatus::Completed) => self.completed += 1,
            Some(WorkerStatus::Failed) => self.failed += 1,
            Some(WorkerStatus::Stopped) => self.stopped += 1,
            Some(WorkerStatus::Interrupted) => self.interrupted += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.idle
            + self.queued
            + self.running
            + self.completed
            + self.failed
            + self.stopped
            + self.interrupted
    }

    /// Queued or running.
    pub fn active(&self) -> usize {
        self.queued + self.running
    }
}

impl Snapshot {
    fn session(&self, id: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Direct children in snapshot order.
    pub fn children_of(&self, session_id: &str) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|s| s.parent_session_id.as_deref() == Some(session_id) && s.id != session_id)
            .collect()
    }

    /// Parent first, root last. Stops at a missing parent or a cycle.
    pub fn ancestors(&self, session_id: &str) -> Vec<&Session> {
        let mut seen = BTreeSet::from([session_id]);
        let mut result = Vec::new();
        let mut current = self.session(session_id);
        while let Some(parent) = current
            .and_then(|s| s.parent_session_id.as_deref())
            .filter(|id| seen.insert(id))
            .and_then(|id| self.session(id))
        {
            result.push(parent);
            current = Some(parent);
        }
        result
    }

    /// Number of ancestors; a root session has depth 0.
    pub fn depth(&self, session_id: &str) -> usize {
        self.ancestors(session_id).len()
    }

    /// The topmost ancestor, or the session itself.
    pub fn root_of<'a>(&'a self, session_id: &str) -> Option<&'a Session> {
        self.ancestors(session_id)
            .last()
            .copied()
            .or_else(|| self.session(session_id))
    }

    /// Depth-first pre-order, excluding the session itself. Each session appears once.
    pub fn descendants(&self, session_id: &str) -> Vec<&Session> {
        let mut seen = BTreeSet::from([session_id]);
        let mut result = Vec::new();
        let mut stack = self.children_of(session_id);
        stack.reverse();
        while let Some(session) = stack.pop() {
            if !seen.insert(session.id.as_str()) {
                continue;
            }
            result.push(session);
            let mut children = self.children_of(&session.id);
            children.reverse();
            stack.extend(children);
        }
        result
    }

    /// Worker states of the session and all its descendants.
    pub fn subtree_status(&self, session_id: &str) -> StatusCounts {
        let mut counts = StatusCounts::default();
        for session in self
            .session(session_id)
            .into_iter()
            .chain(self.descendants(session_id))
        {
            counts.add(session.worker.as_ref().map(|w| &w.status));
        }
        counts
    }

    /// Undecided, unexpired permission requests from the session and its descendants.
    pub fn subtree_pending_permissions(&self, session_id: &str) -> Vec<&ToolPermission> {
        let ids = self
            .descendants(session_id)
            .into_iter()
            .map(|s| s.id.as_str())
            .chain([session_id])
            .collect::<BTreeSet<_>>();
        self.tool_permissions
            .iter()
            .filter(|p| p.decision.is_none() && !p.expired && ids.contains(p.session_id.as_str()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Harness, SessionRole, WorkerRun};

    fn session(id: &str, parent: Option<&str>, status: Option<WorkerStatus>) -> Session {
        Session {
            automatic_name: false,
            awaiting_first_message: false,
            archived: false,
            profile: None,
            connection_ids: vec![],
            workspaces: vec![],
            selection: Default::default(),
            conversations: vec![],
            id: id.into(),
            project_id: "p".into(),
            issue_id: None,
            agent_config_id: "d".into(),
            title: id.into(),
            role: SessionRole::Worker,
            fixture: false,
            worker: status.map(|status| WorkerRun {
                model: None,
                context_tokens: None,
                context_window: None,
                last_usage: None,
                harness: Harness::Codex,
                execution: None,
                status,
                thread_id: None,
                worktree: None,
                branch: None,
                base_commit: None,
                error: None,
                usage: None,
                changes: None,
            }),
            parent_session_id: parent.map(Into::into),
            spawn: None,
            compactions: vec![],
        }
    }

    fn ids(sessions: Vec<&Session>) -> Vec<&str> {
        sessions.into_iter().map(|s| s.id.as_str()).collect()
    }

    fn tree() -> Snapshot {
        Snapshot {
            sessions: vec![
                session("root", None, Some(WorkerStatus::Running)),
                session("a", Some("root"), Some(WorkerStatus::Completed)),
                session("b", Some("root"), None),
                session("a1", Some("a"), Some(WorkerStatus::Failed)),
                session("orphan", Some("missing"), Some(WorkerStatus::Queued)),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn walks_tree() {
        let snapshot = tree();
        assert_eq!(ids(snapshot.children_of("root")), ["a", "b"]);
        assert_eq!(ids(snapshot.descendants("root")), ["a", "a1", "b"]);
        assert_eq!(ids(snapshot.ancestors("a1")), ["a", "root"]);
        assert_eq!(snapshot.depth("a1"), 2);
        assert_eq!(snapshot.depth("orphan"), 0);
        assert_eq!(snapshot.root_of("a1").unwrap().id, "root");
        let counts = snapshot.subtree_status("root");
        assert_eq!(
            (counts.running, counts.completed, counts.failed, counts.idle),
            (1, 1, 1, 1)
        );
        assert_eq!(counts.total(), 4);
        assert_eq!(counts.active(), 1);
    }

    #[test]
    fn cycles_terminate() {
        let snapshot = Snapshot {
            sessions: vec![
                session("x", Some("y"), None),
                session("y", Some("x"), None),
                session("self", Some("self"), None),
            ],
            ..Default::default()
        };
        assert_eq!(ids(snapshot.ancestors("x")), ["y"]);
        assert_eq!(ids(snapshot.descendants("x")), ["y"]);
        assert_eq!(snapshot.depth("self"), 0);
        assert!(snapshot.descendants("self").is_empty());
        assert_eq!(snapshot.subtree_status("x").total(), 2);
    }

    #[test]
    fn pending_permissions_cover_subtree() {
        let mut snapshot = tree();
        let permission = |id: &str, session: &str, decision, expired| ToolPermission {
            id: id.into(),
            session_id: session.into(),
            run_id: "r".into(),
            tool: "Bash".into(),
            description: String::new(),
            decision,
            expired,
        };
        snapshot.tool_permissions = vec![
            permission("p1", "a1", None, false),
            permission("p2", "b", Some(true), false),
            permission("p3", "a", None, true),
            permission("p4", "orphan", None, false),
            permission("p5", "root", None, false),
        ];
        let pending = snapshot
            .subtree_pending_permissions("root")
            .into_iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(pending, ["p1", "p5"]);
        assert_eq!(snapshot.subtree_pending_permissions("a").len(), 1);
    }
}
