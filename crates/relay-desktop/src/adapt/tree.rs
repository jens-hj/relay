//! The agent tree of a project, built from `Session::parent_session_id`.

use super::identity::{harness, session_model};
use relay_core::{
    HarnessCatalog, Session, Snapshot, SubmissionState, ToolPermission, WorkerStatus,
};
use relay_ui::prelude::{AgentNode, AgentStatus};
use std::collections::{BTreeMap, BTreeSet};

fn pending(permission: &ToolPermission) -> bool {
    permission.decision.is_none() && !permission.expired
}

/// A session's own state. A pending permission request on the session
/// means it waits for you; otherwise its worker run decides. A session
/// that never ran is queued while a turn waits for it and done otherwise.
pub fn session_status(snapshot: &Snapshot, session: &Session) -> AgentStatus {
    if snapshot
        .tool_permissions
        .iter()
        .any(|p| p.session_id == session.id && pending(p))
    {
        return AgentStatus::Waiting;
    }
    match session.worker.as_ref().map(|w| &w.status) {
        Some(WorkerStatus::Running) => AgentStatus::Working,
        Some(WorkerStatus::Queued) => AgentStatus::Queued,
        Some(WorkerStatus::Completed | WorkerStatus::Stopped) => AgentStatus::Done,
        Some(WorkerStatus::Failed | WorkerStatus::Interrupted) => AgentStatus::Failed,
        None => {
            if snapshot.submissions.iter().any(|s| {
                s.session_id == session.id
                    && matches!(
                        s.state,
                        SubmissionState::Queued
                            | SubmissionState::Launching
                            | SubmissionState::Paused
                    )
            }) {
                AgentStatus::Queued
            } else {
                AgentStatus::Done
            }
        }
    }
}

/// The agents of one project as a forest. Sessions without a parent in the
/// project are roots, so legacy sessions stand alone rather than gaining an
/// invented parent. Parent links are persisted data, so a cycle is broken
/// at the first session reached.
pub fn agent_forest(
    snapshot: &Snapshot,
    project_id: &str,
    selected: &str,
    collapsed: &BTreeSet<String>,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> Vec<AgentNode> {
    let sessions: Vec<&Session> = snapshot
        .sessions
        .iter()
        .filter(|s| s.project_id == project_id)
        .collect();
    let ids: BTreeSet<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
    fn parent<'a>(session: &'a Session, ids: &BTreeSet<&str>) -> Option<&'a str> {
        session
            .parent_session_id
            .as_deref()
            .filter(|id| *id != session.id && ids.contains(id))
    }
    let mut children: BTreeMap<&str, Vec<&Session>> = BTreeMap::new();
    for session in &sessions {
        if let Some(parent) = parent(session, &ids) {
            children.entry(parent).or_default().push(session);
        }
    }
    let mut visited = BTreeSet::new();
    let mut forest = Vec::new();
    let build = |session: &Session, visited: &mut BTreeSet<String>| -> AgentNode {
        fn node(
            session: &Session,
            snapshot: &Snapshot,
            children: &BTreeMap<&str, Vec<&Session>>,
            visited: &mut BTreeSet<String>,
            selected: &str,
            collapsed: &BTreeSet<String>,
            catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
        ) -> AgentNode {
            visited.insert(session.id.clone());
            let mut built = AgentNode::new(
                session.id.clone(),
                session.title.clone(),
                session_status(snapshot, session),
                harness(
                    session
                        .worker
                        .as_ref()
                        .map(|w| w.harness)
                        .unwrap_or_else(|| director_harness(snapshot, session)),
                ),
            );
            let (model, effort) = session_model(session, catalogs);
            built.model = model;
            built.effort = effort;
            built.expanded = !collapsed.contains(&session.id);
            built.selected = session.id == selected;
            let mut nodes = Vec::new();
            for child in children.get(session.id.as_str()).into_iter().flatten() {
                if !visited.contains(&child.id) {
                    nodes.push(node(
                        child, snapshot, children, visited, selected, collapsed, catalogs,
                    ));
                }
            }
            built.children = nodes;
            built
        }
        node(
            session, snapshot, &children, visited, selected, collapsed, catalogs,
        )
    };
    for session in sessions.iter().filter(|s| parent(s, &ids).is_none()) {
        forest.push(build(session, &mut visited));
    }
    for session in &sessions {
        if !visited.contains(&session.id) {
            forest.push(build(session, &mut visited));
        }
    }
    forest
}

/// The harness a session will run with before it has run: its director's
/// effective profile.
fn director_harness(snapshot: &Snapshot, session: &Session) -> relay_core::Harness {
    snapshot
        .directors
        .iter()
        .find(|d| d.id == session.director_id)
        .and_then(|d| snapshot.effective_profile(d).ok())
        .map(|p| p.harness)
        .unwrap_or_default()
}

/// Ancestor titles, root first.
pub fn ancestor_path(snapshot: &Snapshot, session_id: &str) -> Vec<String> {
    let mut path: Vec<String> = snapshot
        .ancestors(session_id)
        .into_iter()
        .map(|s| s.title.clone())
        .collect();
    path.reverse();
    path
}

/// The states of everything below a session, in tree order.
pub fn subtree_statuses(snapshot: &Snapshot, session_id: &str) -> Vec<AgentStatus> {
    snapshot
        .descendants(session_id)
        .into_iter()
        .map(|s| session_status(snapshot, s))
        .collect()
}

/// A permission request waiting for you, with the agent that asked and its
/// ancestors.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingApproval {
    pub permission: ToolPermission,
    pub session_id: String,
    pub name: String,
    /// Ancestor titles, root first.
    pub path: Vec<String>,
}

/// Every pending permission request in a project's sessions, in snapshot
/// order. Answering one with `RespondPermission` resolves it everywhere it
/// is shown.
pub fn needs_you(snapshot: &Snapshot, project_id: &str) -> Vec<PendingApproval> {
    snapshot
        .tool_permissions
        .iter()
        .filter(|p| pending(p))
        .filter_map(|permission| {
            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == permission.session_id && s.project_id == project_id)?;
            Some(PendingApproval {
                permission: permission.clone(),
                session_id: session.id.clone(),
                name: session.title.clone(),
                path: ancestor_path(snapshot, &session.id),
            })
        })
        .collect()
}
