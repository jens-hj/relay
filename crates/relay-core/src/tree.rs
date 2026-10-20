//! Pure queries over the sub-agent tree formed by `Session::parent_session_id`.
//! Parent links come from persisted data, so every walk is cycle-safe.
use crate::{Session, SessionPeer, SessionRole, Snapshot, ToolPermission, WorkerStatus};
use std::collections::{BTreeSet, HashSet};

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

    /// Whether `parent_id` can take one more active descendant. The parent and
    /// every ancestor need a free slot under their own `max_workers`, each
    /// capped by the agent configuration's limit. `exclude` is the session
    /// asking for the slot, so its own activity is not counted twice.
    pub fn child_slot(&self, parent_id: &str, exclude: Option<&str>) -> Result<(), String> {
        let parent = self.session(parent_id).ok_or("Parent agent missing")?;
        for owner in std::iter::once(parent).chain(self.ancestors(parent_id)) {
            let config = self
                .agent_configs
                .iter()
                .find(|d| d.id == owner.agent_config_id)
                .ok_or("Agent configuration missing")?;
            let limit = self
                .session_profile(owner)?
                .max_workers
                .min(self.effective_profile(config)?.max_workers);
            let used = self
                .descendants(&owner.id)
                .into_iter()
                .filter(|s| Some(s.id.as_str()) != exclude && active(s))
                .count();
            if used >= usize::from(limit) {
                return Err("Current director worker limit reached".into());
            }
        }
        Ok(())
    }

    /// Whether a standalone worker (a parentless `Worker` session started from
    /// the board) can take a slot of its agent configuration. Standalone
    /// workers share that pool; their descendants never count against it.
    pub fn standalone_slot(
        &self,
        agent_config_id: &str,
        exclude: Option<&str>,
    ) -> Result<(), String> {
        let config = self
            .agent_configs
            .iter()
            .find(|d| d.id == agent_config_id)
            .ok_or("Agent configuration missing")?;
        let used = self
            .sessions
            .iter()
            .filter(|s| {
                Some(s.id.as_str()) != exclude
                    && s.agent_config_id == agent_config_id
                    && s.parent_session_id.is_none()
                    && s.role == SessionRole::Worker
                    && active(s)
            })
            .count();
        if used >= usize::from(self.effective_profile(config)?.max_workers) {
            return Err("Current director worker limit reached".into());
        }
        Ok(())
    }

    /// Whether a session may run a turn. A child occupies one slot of each
    /// ancestor, a root session occupies none, and a standalone worker
    /// occupies one slot of its configuration. A session's own descendants
    /// never block its turns.
    pub fn worker_slot(&self, session: &Session) -> Result<(), String> {
        match session.parent_session_id.as_deref() {
            Some(parent) => self.child_slot(parent, Some(&session.id)),
            None if session.role == SessionRole::Worker => {
                self.standalone_slot(&session.agent_config_id, Some(&session.id))
            }
            None => Ok(()),
        }
    }

    /// A session that can hold a peer grant: present, unarchived and not a fixture.
    pub fn is_live(&self, session_id: &str) -> bool {
        self.session(session_id)
            .is_some_and(|s| !s.archived && !s.fixture)
    }

    /// Whether a human mention granted these two live sessions direct messaging.
    pub fn is_peer(&self, a: &str, b: &str) -> bool {
        let pair = peer_pair(a, b);
        a != b
            && self.is_live(a)
            && self.is_live(b)
            && self.session_peers.iter().any(|p| p.sessions == pair)
    }

    /// Live direct peers of a session, in grant order. Peers of peers are not included.
    pub fn peers_of(&self, session_id: &str) -> Vec<&Session> {
        let mut seen = BTreeSet::new();
        self.session_peers
            .iter()
            .filter_map(|p| match &p.sessions {
                [a, b] if a == session_id => Some(b.as_str()),
                [a, b] if b == session_id => Some(a.as_str()),
                _ => None,
            })
            .filter(|id| seen.insert(*id) && self.is_peer(session_id, id))
            .filter_map(|id| self.session(id))
            .collect()
    }

    /// Record a grant between two distinct sessions unless one already exists.
    /// Returns whether a new grant was added.
    pub fn grant_peer(
        &mut self,
        source_session_id: &str,
        target_session_id: &str,
        request_id: &str,
        granted_at: u64,
    ) -> bool {
        let sessions = peer_pair(source_session_id, target_session_id);
        if source_session_id == target_session_id
            || self.session_peers.iter().any(|p| p.sessions == sessions)
        {
            return false;
        }
        self.session_peers.push(SessionPeer {
            id: format!("peer:{}:{}", sessions[0], sessions[1]),
            sessions,
            request_id: request_id.into(),
            source_session_id: source_session_id.into(),
            granted_at,
        });
        true
    }

    /// Remove every grant touching one of these sessions.
    pub fn prune_peers(&mut self, session_ids: &HashSet<String>) {
        self.session_peers
            .retain(|p| !p.sessions.iter().any(|id| session_ids.contains(id)));
    }
}

fn peer_pair(a: &str, b: &str) -> [String; 2] {
    let mut pair = [a.to_owned(), b.to_owned()];
    pair.sort();
    pair
}

fn active(session: &Session) -> bool {
    session
        .worker
        .as_ref()
        .is_some_and(|w| matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Harness, WorkerRun};

    fn session(id: &str, parent: Option<&str>, status: Option<WorkerStatus>) -> Session {
        Session {
            network_access: None,
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
                launched_network_policy: None,
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

    #[test]
    fn peer_grants_are_direct_symmetric_deduplicated_and_live_only() {
        let mut snapshot = tree();
        snapshot.sessions.push(session("other", None, None));
        snapshot.sessions.push(session("third", None, None));
        assert!(!snapshot.is_peer("a", "other"));
        assert!(snapshot.grant_peer("other", "a", "r1", 1));
        assert!(!snapshot.grant_peer("a", "other", "r2", 2));
        assert!(!snapshot.grant_peer("a", "a", "r3", 3));
        assert_eq!(snapshot.session_peers.len(), 1);
        assert_eq!(snapshot.session_peers[0].sessions, ["a", "other"]);
        assert_eq!(snapshot.session_peers[0].id, "peer:a:other");
        assert_eq!(snapshot.session_peers[0].source_session_id, "other");
        assert_eq!(snapshot.session_peers[0].request_id, "r1");
        assert!(snapshot.is_peer("a", "other") && snapshot.is_peer("other", "a"));
        // Neither the parent, the children nor peers of peers gain anything.
        assert!(snapshot.grant_peer("other", "third", "r4", 4));
        assert!(!snapshot.is_peer("a", "third"));
        assert!(!snapshot.is_peer("root", "other") && !snapshot.is_peer("a1", "other"));
        assert_eq!(ids(snapshot.peers_of("other")), ["a", "third"]);
        assert_eq!(ids(snapshot.peers_of("a")), ["other"]);
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "third")
            .unwrap()
            .archived = true;
        assert!(!snapshot.is_peer("other", "third"));
        assert_eq!(ids(snapshot.peers_of("other")), ["a"]);
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "a")
            .unwrap()
            .fixture = true;
        assert!(snapshot.peers_of("other").is_empty());
        snapshot.prune_peers(&HashSet::from(["third".to_owned()]));
        assert_eq!(snapshot.session_peers.len(), 1);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["session_peers"][0]["sessions"][1], "other");
        snapshot.prune_peers(&HashSet::from(["a".to_owned()]));
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json.get("session_peers").is_none());
        let restored: Snapshot = serde_json::from_value(json).unwrap();
        assert!(restored.session_peers.is_empty());
    }
}
