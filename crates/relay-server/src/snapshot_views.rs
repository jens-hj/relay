//! Owned inputs for consumers that must not retain or copy transcript history.
//! No watch borrow crosses an await. No Arc::make_mut or hidden whole clone.
use relay_core::*;

fn workspace(space: &SessionWorkspace) -> SessionWorkspace {
    SessionWorkspace {
        connection_id: space.connection_id.clone(),
        path: space.path.clone(),
        repository: space.repository,
        branch: space.branch.clone(),
        base_commit: space.base_commit.clone(),
        changes: None,
    }
}

pub(super) fn review_workspaces(session: &Session) -> Vec<SessionWorkspace> {
    let mut spaces = session.workspaces.iter().map(workspace).collect::<Vec<_>>();
    if spaces.is_empty()
        && let Some(worker) = &session.worker
        && let (Some(path), Some(base)) = (&worker.worktree, &worker.base_commit)
    {
        spaces.push(SessionWorkspace {
            connection_id: String::new(),
            path: path.clone(),
            repository: true,
            branch: worker.branch.clone(),
            base_commit: Some(base.clone()),
            changes: None,
        });
    }
    spaces
}

fn worker(worker: &WorkerRun) -> WorkerRun {
    WorkerRun {
        model: worker.model.clone(),
        context_tokens: worker.context_tokens,
        context_window: worker.context_window,
        last_usage: worker.last_usage.clone(),
        harness: worker.harness,
        execution: worker.execution.clone(),
        launched_network_policy: worker.launched_network_policy,
        status: worker.status.clone(),
        thread_id: worker.thread_id.clone(),
        worktree: worker.worktree.clone(),
        branch: worker.branch.clone(),
        base_commit: worker.base_commit.clone(),
        error: worker.error.clone(),
        usage: worker.usage.clone(),
        changes: None,
    }
}

fn session(session: &Session) -> Session {
    Session {
        network_access: session.network_access,
        automatic_name: session.automatic_name,
        awaiting_first_message: session.awaiting_first_message,
        archived: session.archived,
        profile: session.profile.clone(),
        connection_ids: session.connection_ids.clone(),
        workspaces: session.workspaces.iter().map(workspace).collect(),
        selection: session.selection.clone(),
        conversations: session.conversations.clone(),
        id: session.id.clone(),
        project_id: session.project_id.clone(),
        issue_id: session.issue_id.clone(),
        agent_config_id: session.agent_config_id.clone(),
        title: session.title.clone(),
        role: session.role.clone(),
        fixture: session.fixture,
        worker: session.worker.as_ref().map(worker),
        parent_session_id: session.parent_session_id.clone(),
        spawn: session.spawn.clone(),
        compactions: session.compactions.clone(),
    }
}

fn pending(submission: &Submission) -> bool {
    matches!(
        submission.state,
        SubmissionState::Queued
            | SubmissionState::Launching
            | SubmissionState::Running
            | SubmissionState::Paused
    )
}

/// Construct only included records: never clone then discard excluded payloads.
/// Exhaustive Snapshot literal makes new fields require an explicit copy decision.
pub(super) fn metadata(source: &Snapshot) -> Snapshot {
    Snapshot {
        agent_templates: source.agent_templates.clone(),
        board_aliases: source.board_aliases.clone(),
        issue_aliases: source.issue_aliases.clone(),
        protocol_version: source.protocol_version,
        connections: source.connections.clone(),
        boards: source.boards.clone(),
        memberships: source.memberships.clone(),
        operations: source.operations.clone(),
        bindings: source.bindings.clone(),
        installations: source.installations.clone(),
        tool_permissions: source
            .tool_permissions
            .iter()
            .filter(|p| p.decision.is_none() && !p.expired)
            .cloned()
            .collect(),
        revision: source.revision,
        projects: source.projects.clone(),
        issues: source.issues.clone(),
        agent_configs: source.agent_configs.clone(),
        sessions: source.sessions.iter().map(session).collect(),
        messages: Vec::new(),
        comments: Vec::new(),
        submissions: source
            .submissions
            .iter()
            .filter(|s| pending(s))
            .cloned()
            .collect(),
        usage_limits: source.usage_limits.clone(),
        usage_ledger: source.usage_ledger.clone(),
        claude_window_start: source.claude_window_start.clone(),
        session_peers: source.session_peers.clone(),
    }
}

/// Execution reads no transcript/comment/permission records. Keep all policy,
/// ancestry, usage, installation and project fields and ONLY this run's parts.
/// Preserve full session reviews here: legacy preparation may return those fields.
pub(super) fn execution(source: &Snapshot, run: &str) -> Snapshot {
    // Unlike metadata, no unrelated queue or pending permission payload is copied.
    Snapshot {
        agent_templates: source.agent_templates.clone(),
        board_aliases: source.board_aliases.clone(),
        issue_aliases: source.issue_aliases.clone(),
        protocol_version: source.protocol_version,
        connections: source.connections.clone(),
        boards: source.boards.clone(),
        memberships: source.memberships.clone(),
        operations: source.operations.clone(),
        bindings: source.bindings.clone(),
        installations: source.installations.clone(),
        tool_permissions: Vec::new(),
        revision: source.revision,
        projects: source.projects.clone(),
        issues: source.issues.clone(),
        agent_configs: source.agent_configs.clone(),
        sessions: source.sessions.clone(),
        messages: Vec::new(),
        comments: Vec::new(),
        submissions: source
            .submissions
            .iter()
            .find(|s| s.id == run)
            .cloned()
            .into_iter()
            .collect(),
        usage_limits: source.usage_limits.clone(),
        usage_ledger: source.usage_ledger.clone(),
        claude_window_start: source.claude_window_start.clone(),
        session_peers: source.session_peers.clone(),
    }
}

/// prepare_workspaces needs only project/board detection, connection validation,
/// canonical issue resolution and legacy harness configuration. Session is passed
/// separately. These fields are owned for the blocking task; no history or policy
/// snapshot is duplicated just to prepare paths.
pub(super) fn preparation(source: &Snapshot) -> Snapshot {
    Snapshot {
        projects: source.projects.clone(),
        boards: source.boards.clone(),
        connections: source.connections.clone(),
        issues: source.issues.clone(),
        issue_aliases: source.issue_aliases.clone(),
        bindings: source.bindings.clone(),
        installations: source.installations.clone(),
        ..Snapshot::default()
    }
}
