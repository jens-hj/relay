//! Relay's domain and versioned client/server contract. No UI or execution dependencies.

mod transport;
pub use transport::*;
mod build_info;
pub use build_info::*;
mod harness_commands;
pub use harness_commands::*;
mod conversation;
mod tools;
pub use tools::*;
mod projects;
pub use projects::*;
mod execution;
pub use execution::*;
mod fixture;
mod profile;
mod tree;
pub use tree::*;
mod usage;
mod window_start;
mod worker_model;
pub use conversation::*;
pub use usage::*;
pub use window_start::*;
pub use worker_model::*;

pub use fixture::{demo_catalog, demo_orchestration, demo_snapshot, demo_usage};
pub use profile::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Gitlab,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRef {
    pub provider: Provider,
    pub repository: String,
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardColumn {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub root: Option<String>,
    pub id: String,
    pub name: String,
    pub repository: String,
    pub fixture: bool,
    pub columns: Vec<BoardColumn>,
    pub defaults: AgentProfile,
    #[serde(default)]
    pub github: Option<GitHubProject>,
    /// Off unless the project owner enables it. Omitted while off, so
    /// existing snapshots keep their shape.
    #[serde(default, skip_serializing_if = "NixCommands::is_off")]
    pub nix_commands: NixCommands,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubProject {
    pub owner: String,
    pub number: u64,
    pub url: String,
    pub last_synced_at: Option<u64>,
    pub sync_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    #[serde(default)]
    pub repository_connection_id: Option<String>,
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub reference: Option<IssueRef>,
    pub title: String,
    pub body: String,
    pub column_id: String,
    pub labels: Vec<String>,
    pub result: Option<String>,
}

fn director_template() -> String {
    "director".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub automatic_name: bool,
    #[serde(default)]
    pub draft_selection: HarnessSelection,
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default = "director_template")]
    pub template: String,
    pub overrides: ProfileOverrides,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    #[serde(alias = "director")]
    Root,
    Worker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub automatic_name: bool,
    #[serde(default)]
    pub awaiting_first_message: bool,
    #[serde(default)]
    pub archived: bool,
    /// Instance settings for a manually configured agent; inherited when absent.
    #[serde(default)]
    pub profile: Option<AgentProfile>,
    #[serde(default)]
    pub connection_ids: Vec<String>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspace>,
    #[serde(default)]
    pub selection: HarnessSelection,
    #[serde(default)]
    pub conversations: Vec<RetainedConversation>,
    pub id: String,
    pub project_id: String,
    pub issue_id: Option<String>,
    #[serde(alias = "director_id")]
    pub agent_config_id: String,
    pub title: String,
    pub role: SessionRole,
    pub fixture: bool,
    #[serde(default)]
    pub worker: Option<WorkerRun>,
    /// The session that requested this one, when it was started as a sub-agent.
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub spawn: Option<SpawnGrant>,
    /// Every context compaction reported by the harness, across runs.
    #[serde(default)]
    pub compactions: Vec<Compaction>,
}

/// What a parent session asked a sub-agent to do, and the permissions it granted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnGrant {
    pub task: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub permissions: std::collections::BTreeMap<Task, Permission>,
    #[serde(default)]
    pub requested_by_run: Option<String>,
    /// Why the parent chose the worker's harness, model and effort; required
    /// when the parent's worker model policy lets it choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_rationale: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    /// Context occupancy when the harness compacted, if it was measured.
    #[serde(default)]
    pub at_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Stopped,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub files: Vec<String>,
    pub diff: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRun {
    /// Actual model reported by the harness.
    #[serde(default)]
    pub model: Option<String>,
    /// Latest context occupancy and capacity reported by the harness.
    #[serde(default)]
    pub context_tokens: Option<u64>,
    #[serde(default)]
    pub context_window: Option<u64>,
    /// Most recent measured usage, retained while a new turn is queued.
    #[serde(default)]
    pub last_usage: Option<TokenUsage>,
    #[serde(default)]
    pub harness: Harness,
    #[serde(default)]
    pub execution: Option<ExecutionSettings>,
    pub status: WorkerStatus,
    pub thread_id: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub base_commit: Option<String>,
    pub error: Option<String>,
    pub usage: Option<TokenUsage>,
    pub changes: Option<ChangeSet>,
}

/// Who submitted a prompt. Records written before senders were stored are the user's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Sender {
    #[default]
    User,
    Agent {
        session_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub author: String,
    #[serde(default)]
    pub sender: Sender,
    pub kind: String,
    pub body: String,
    #[serde(default)]
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolCall>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub message_id: String,
    /// Optional quoted passage from the immutable message. Not a line-number anchor.
    pub quote: Option<String>,
    /// A display label supplied by a member of the token-trusted workspace.
    pub author: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub agent_templates: Vec<AgentTemplate>,
    /// Retained IDs redirect to the identities preserved by board publication.
    #[serde(default)]
    pub board_aliases: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub issue_aliases: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub protocol_version: u32,
    #[serde(default)]
    pub connections: Vec<ProjectConnection>,
    #[serde(default)]
    pub boards: Vec<Board>,
    #[serde(default)]
    pub memberships: Vec<BoardMembership>,
    #[serde(default)]
    pub operations: Vec<ProjectOperation>,
    #[serde(default)]
    pub bindings: Vec<ProjectBinding>,
    #[serde(default)]
    pub installations: Vec<HarnessInstallation>,
    #[serde(default)]
    pub tool_permissions: Vec<ToolPermission>,
    pub revision: u64,
    pub projects: Vec<Project>,
    pub issues: Vec<Issue>,
    #[serde(alias = "directors")]
    pub agent_configs: Vec<AgentConfig>,
    pub sessions: Vec<Session>,
    pub messages: Vec<Message>,
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub submissions: Vec<Submission>,
    #[serde(default)]
    pub usage_limits: Vec<UsageLimit>,
    #[serde(default)]
    pub usage_ledger: Vec<UsageEntry>,
    /// The Claude window-start message setting and status; omitted until the
    /// owner touches it, so existing snapshots keep their shape.
    #[serde(default, skip_serializing_if = "WindowStart::is_default")]
    pub claude_window_start: WindowStart,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub request_id: String,
    pub expected_revision: u64,
    pub command: Command,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    DeleteSessionBranch {
        session_id: String,
    },
    SetSessionArchived {
        session_id: String,
        archived: bool,
    },
    SetHarnessSelection {
        session_id: String,
        selection: HarnessSelection,
    },
    ConversationCommand {
        session_id: String,
        name: String,
        argument: String,
    },
    CreateProject {
        name: String,
        /// An absolute directory on the server. Empty places the project
        /// under the server's own project directory, named after the project.
        root: String,
        connections: Vec<ConnectionInput>,
    },
    RenameProject {
        project_id: String,
        name: String,
    },
    /// Whether the project's agents may run `nix develop --command` on the
    /// server, outside their sandbox. Only workspace clients can send this.
    SetNixCommands {
        project_id: String,
        mode: NixCommands,
    },
    /// Whether the server sends one tiny Claude message after each reported
    /// five-hour window ends. It uses subscription usage, so it stays off
    /// until the owner turns it on. Only workspace clients can send this.
    SetClaudeWindowStart {
        enabled: bool,
    },
    AddConnection {
        project_id: String,
        connection: ConnectionInput,
    },
    /// Rename only; resource identity and captured session roots are immutable.
    UpdateConnection {
        connection_id: String,
        name: String,
    },
    RetryConnection {
        connection_id: String,
    },
    RemoveConnection {
        connection_id: String,
    },
    CreateTask {
        repository_connection_id: Option<String>,
        board_id: String,
        title: String,
        body: String,
    },
    UpdateTask {
        issue_id: String,
        title: String,
        body: String,
    },
    MoveTask {
        board_id: String,
        issue_id: String,
        column_id: String,
    },
    UpdateBoardColumns {
        board_id: String,
        columns: Vec<BoardColumn>,
    },
    SyncBoard {
        board_id: String,
    },
    PublishBoard {
        board_id: String,
        target: PublishTarget,
        columns: Vec<ColumnMapping>,
        tasks: Vec<TaskPublication>,
    },
    RetryOperation {
        operation_id: String,
    },
    ReconcileOperation {
        operation_id: String,
        key: String,
        result: String,
    },
    StartSession {
        agent_config_id: String,
        issue_id: Option<String>,
        role: SessionRole,
        prompt: String,
        approve_implementation: bool,
        connection_ids: Option<Vec<String>>,
    },
    StartRootAgent {
        agent_config_id: String,
        #[serde(default)]
        issue_id: Option<String>,
        prompt: String,
        #[serde(default)]
        parts: Vec<Part>,
        approve_implementation: bool,
    },
    SetSessionConnections {
        session_id: String,
        connection_ids: Vec<String>,
    },
    ConfigureProject {
        binding: ProjectBinding,
    },
    ConfigureHarness {
        harness: Harness,
        executable: String,
    },
    SetWorkerExecution {
        session_id: String,
        execution: Option<ExecutionSettings>,
    },
    RespondPermission {
        permission_id: String,
        run_id: String,
        allow: bool,
    },
    SubmitTurn {
        session_id: String,
        draft_revision: u64,
        parts: Vec<Part>,
        approve_implementation: bool,
    },
    PromoteTurn {
        submission_id: String,
        active_run_id: Option<String>,
    },
    CancelTurn {
        submission_id: String,
    },
    EditQueuedTurn {
        submission_id: String,
        draft_revision: u64,
        parts: Vec<Part>,
        #[serde(default)]
        approve_implementation: bool,
    },
    ResumeQueue {
        session_id: String,
    },
    /// Holds a session's queued turns: they become paused, nothing launches
    /// after the current turn, and `ResumeQueue` continues them.
    PauseQueue {
        session_id: String,
    },
    SyncProject {
        project_id: String,
    },
    StartWorker {
        issue_id: String,
        agent_config_id: String,
        prompt: String,
        approve_implementation: bool,
    },
    StartChildAgent {
        parent_session_id: String,
        name: String,
        profile: AgentProfile,
        prompt: String,
        approve_implementation: bool,
    },
    SaveAgentTemplate {
        project_id: String,
        name: String,
        profile: AgentProfile,
    },
    SendWorker {
        session_id: String,
        prompt: String,
        approve_implementation: bool,
    },
    /// Stops a session's active turn. Descendants keep running unless
    /// `include_descendants` asks to stop the whole subtree.
    StopWorker {
        session_id: String,
        #[serde(default)]
        include_descendants: bool,
    },
    UpdateProjectDefaults {
        project_id: String,
        profile: AgentProfile,
    },
    CreateAgentConfig {
        project_id: String,
        name: String,
        template: String,
        overrides: ProfileOverrides,
    },
    UpdateAgentConfig {
        agent_config_id: String,
        name: String,
        overrides: ProfileOverrides,
    },
    RemoveAgentConfig {
        agent_config_id: String,
    },
    AddComment {
        message_id: String,
        quote: Option<String>,
        author: String,
        body: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl Snapshot {
    pub fn project(&self, id: &str) -> Result<&Project, String> {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "Project not found".into())
    }

    pub fn effective_profile(&self, director: &AgentConfig) -> Result<AgentProfile, String> {
        Ok(director.overrides.resolve(&self.config_base(director)?))
    }

    pub fn config_base(&self, config: &AgentConfig) -> Result<AgentProfile, String> {
        self.template_profile(
            &config.project_id,
            &config.template,
            &self.project(&config.project_id)?.defaults,
        )
    }

    fn template_profile(
        &self,
        project_id: &str,
        template: &str,
        defaults: &AgentProfile,
    ) -> Result<AgentProfile, String> {
        match template {
            "director" => Ok(defaults.clone()),
            "worker" => Ok(AgentProfile::worker(defaults)),
            id => self
                .agent_templates
                .iter()
                .find(|p| p.id == id && p.project_id == project_id)
                .map(|p| p.profile.clone())
                .ok_or_else(|| "Agent profile not found in this project".into()),
        }
    }

    pub fn session_profile(&self, session: &Session) -> Result<AgentProfile, String> {
        let director = self
            .agent_configs
            .iter()
            .find(|d| d.id == session.agent_config_id && d.project_id == session.project_id)
            .ok_or("Agent configuration missing")?;
        let mut profile = self.effective_profile(director)?;
        let mut chain = vec![session];
        let mut seen = std::collections::BTreeSet::from([session.id.as_str()]);
        while let Some(id) = chain.last().unwrap().parent_session_id.as_deref() {
            if !seen.insert(id) {
                return Err("Invalid session ancestry".into());
            }
            let parent = self
                .sessions
                .iter()
                .find(|s| {
                    s.id == id
                        && s.project_id == session.project_id
                        && s.agent_config_id == session.agent_config_id
                })
                .ok_or("Parent missing")?;
            chain.push(parent);
        }
        for session in chain.into_iter().rev() {
            if let Some(settings) = &session.profile {
                profile = settings.clone();
            }
            if let Some(grant) = &session.spawn {
                profile.permissions.extend(grant.permissions.clone());
            }
        }
        Ok(profile)
    }

    pub fn validate_profile(&self, project_id: &str, profile: &AgentProfile) -> Result<(), String> {
        self.project(project_id)?;
        profile.validate()?;
        if let AgentScope::Issues { issue_ids } = &profile.scope {
            for id in issue_ids {
                if !self
                    .issues
                    .iter()
                    .any(|i| i.id == *id && i.project_id == project_id)
                {
                    return Err(format!("Scope issue {id} does not belong to this project"));
                }
            }
        }
        Ok(())
    }

    /// A child's explicit profile inherits its parent's worker model policy
    /// and may only narrow it.
    pub fn validate_child_profile(
        &self,
        parent: &Session,
        profile: &AgentProfile,
    ) -> Result<(), String> {
        self.validate_profile(&parent.project_id, profile)?;
        profile
            .worker_models
            .narrows(&self.session_profile(parent)?.worker_models)
    }

    pub fn session_branch_ids(&self, session_id: &str) -> std::collections::HashSet<String> {
        self.sessions
            .iter()
            .find(|s| s.id == session_id)
            .into_iter()
            .chain(self.descendants(session_id))
            .map(|s| s.id.clone())
            .collect()
    }

    /// An archive never hides an executing agent or an executing descendant.
    pub fn can_archive_session(&self, session_id: &str) -> bool {
        if !self.sessions.iter().any(|s| s.id == session_id) {
            return false;
        }
        let branch = self.session_branch_ids(session_id);
        !self.sessions.iter().any(|s| {
            branch.contains(&s.id)
                && s.worker.as_ref().is_some_and(|w| {
                    matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued)
                })
        })
    }

    /// Apply only to a candidate snapshot: the server commits it atomically after validation.
    pub fn apply(&mut self, command: Command, id: &str, now: u64) -> Result<(), String> {
        match command {
            Command::SetSessionArchived {
                session_id,
                archived,
            } => {
                if archived && !self.can_archive_session(&session_id) {
                    return Err(
                        "Stop this agent and its children before archiving this chat".into(),
                    );
                }
                if !self.sessions.iter().any(|s| s.id == session_id) {
                    return Err("Session not found".into());
                }
                let branch = self.session_branch_ids(&session_id);
                for session in &mut self.sessions {
                    if branch.contains(&session.id) {
                        session.archived = archived;
                    }
                }
                if archived {
                    for turn in self.submissions.iter_mut().filter(|s| {
                        branch.contains(&s.session_id) && s.state == SubmissionState::Queued
                    }) {
                        turn.state = SubmissionState::Paused;
                        turn.error = Some("Chat archived; unarchive and resume to continue".into());
                    }
                }
            }
            Command::DeleteSessionBranch { session_id } => {
                let branch = self.session_branch_ids(&session_id);
                if branch.is_empty() {
                    return Err("Session not found".into());
                }
                if !self.can_archive_session(&session_id)
                    || self
                        .sessions
                        .iter()
                        .any(|s| branch.contains(&s.id) && !s.archived)
                {
                    return Err(
                        "Archive this entire branch and stop its workers before deleting it".into(),
                    );
                }
                let configs: std::collections::HashSet<_> = self
                    .sessions
                    .iter()
                    .filter(|s| branch.contains(&s.id))
                    .map(|s| s.agent_config_id.clone())
                    .collect();
                let messages: std::collections::HashSet<_> = self
                    .messages
                    .iter()
                    .filter(|m| branch.contains(&m.session_id))
                    .map(|m| m.id.clone())
                    .collect();
                self.sessions.retain(|s| !branch.contains(&s.id));
                self.messages.retain(|m| !branch.contains(&m.session_id));
                self.comments.retain(|c| !messages.contains(&c.message_id));
                self.submissions.retain(|s| !branch.contains(&s.session_id));
                self.usage_ledger
                    .retain(|entry| !branch.contains(&entry.session_id));
                self.tool_permissions
                    .retain(|p| !branch.contains(&p.session_id));
                self.agent_configs.retain(|c| {
                    !configs.contains(&c.id)
                        || self.sessions.iter().any(|s| s.agent_config_id == c.id)
                });
            }

            Command::CreateProject { .. }
            | Command::RenameProject { .. }
            | Command::SetNixCommands { .. }
            | Command::SetClaudeWindowStart { .. }
            | Command::AddConnection { .. }
            | Command::UpdateConnection { .. }
            | Command::RetryConnection { .. }
            | Command::RemoveConnection { .. }
            | Command::CreateTask { .. }
            | Command::UpdateTask { .. }
            | Command::MoveTask { .. }
            | Command::UpdateBoardColumns { .. }
            | Command::SyncBoard { .. }
            | Command::PublishBoard { .. }
            | Command::RetryOperation { .. }
            | Command::ReconcileOperation { .. }
            | Command::StartSession { .. }
            | Command::StartRootAgent { .. }
            | Command::SetSessionConnections { .. }
            | Command::ConfigureProject { .. }
            | Command::ConfigureHarness { .. }
            | Command::RespondPermission { .. }
            | Command::SetWorkerExecution { .. }
            | Command::SetHarnessSelection { .. }
            | Command::ConversationCommand { .. }
            | Command::SyncProject { .. }
            | Command::StartWorker { .. }
            | Command::StartChildAgent { .. }
            | Command::SendWorker { .. }
            | Command::SubmitTurn { .. }
            | Command::PromoteTurn { .. }
            | Command::CancelTurn { .. }
            | Command::EditQueuedTurn { .. }
            | Command::ResumeQueue { .. }
            | Command::PauseQueue { .. }
            | Command::StopWorker { .. } => {
                return Err("This command requires the server runtime".into());
            }
            Command::SaveAgentTemplate {
                project_id,
                name,
                profile,
            } => {
                validate_text("Profile name", &name, 100)?;
                self.validate_profile(&project_id, &profile)?;
                if ["director", "worker"].contains(&name.trim().to_lowercase().as_str())
                    || self.agent_templates.iter().any(|p| {
                        p.project_id == project_id && p.name.eq_ignore_ascii_case(name.trim())
                    })
                {
                    return Err("Choose a unique profile name".into());
                }
                self.agent_templates.push(AgentTemplate {
                    id: format!("profile-{id}"),
                    project_id,
                    name: name.trim().into(),
                    profile,
                });
            }
            Command::UpdateProjectDefaults {
                project_id,
                profile,
            } => {
                self.validate_profile(&project_id, &profile)?;
                for director in self
                    .agent_configs
                    .iter()
                    .filter(|d| d.project_id == project_id)
                {
                    self.validate_profile(
                        &project_id,
                        &director.overrides.resolve(&self.template_profile(
                            &project_id,
                            &director.template,
                            &profile,
                        )?),
                    )?;
                }
                self.projects
                    .iter_mut()
                    .find(|p| p.id == project_id)
                    .unwrap()
                    .defaults = profile;
            }
            Command::CreateAgentConfig {
                project_id,
                name,
                template,
                overrides,
            } => {
                if !name.trim().is_empty() {
                    validate_text("Agent name", &name, 100)?;
                }
                let profile = overrides.resolve(&self.template_profile(
                    &project_id,
                    &template,
                    &self.project(&project_id)?.defaults,
                )?);
                self.validate_profile(&project_id, &profile)?;
                if profile.model.is_empty() {
                    return Err("Choose a model before creating this agent".into());
                }
                self.agent_configs.push(AgentConfig {
                    automatic_name: name.trim().is_empty(),
                    draft_selection: HarnessSelection::default(),
                    id: format!("agent-config-{id}"),
                    project_id,
                    name: if name.trim().is_empty() {
                        "New agent".into()
                    } else {
                        name.trim().into()
                    },
                    template,
                    overrides,
                });
            }
            Command::UpdateAgentConfig {
                agent_config_id,
                name,
                overrides,
            } => {
                if !name.trim().is_empty() {
                    validate_text("Agent name", &name, 100)?;
                }
                let director = self
                    .agent_configs
                    .iter()
                    .find(|d| d.id == agent_config_id)
                    .ok_or("Director not found")?;
                let previous_profile = self.effective_profile(director)?;
                let next_profile = overrides.resolve(&self.config_base(director)?);
                self.validate_profile(
                    &director.project_id,
                    &overrides.resolve(&self.config_base(director)?),
                )?;
                let director = self
                    .agent_configs
                    .iter_mut()
                    .find(|d| d.id == agent_config_id)
                    .unwrap();
                if previous_profile.model != next_profile.model
                    || previous_profile.harness != next_profile.harness
                {
                    director.draft_selection = HarnessSelection::default();
                }
                director.automatic_name = name.trim().is_empty();
                director.name = if director.automatic_name {
                    "New agent".into()
                } else {
                    name.trim().into()
                };
                director.overrides = overrides;
                for session in self
                    .sessions
                    .iter_mut()
                    .filter(|s| s.agent_config_id == agent_config_id && s.role == SessionRole::Root)
                {
                    session.automatic_name = name.trim().is_empty();
                    if !session.automatic_name {
                        session.title = name.trim().into();
                    } else if let Some(message) = self.messages.iter().find(|m| {
                        m.session_id == session.id && matches!(m.kind.as_str(), "user" | "prompt")
                    }) {
                        session.title = message
                            .body
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(80)
                            .collect();
                    }
                }
            }
            Command::RemoveAgentConfig { agent_config_id } => {
                let index = self
                    .agent_configs
                    .iter()
                    .position(|c| c.id == agent_config_id)
                    .ok_or("Agent configuration not found")?;
                if self
                    .sessions
                    .iter()
                    .any(|s| s.agent_config_id == agent_config_id)
                {
                    return Err("Only agents without sessions can be removed".into());
                }
                self.agent_configs.remove(index);
            }
            Command::AddComment {
                message_id,
                quote,
                author,
                body,
            } => {
                validate_text("Comment", &body, 16_000)?;
                validate_text("Author", &author, 100)?;
                let message = self
                    .messages
                    .iter()
                    .find(|m| m.id == message_id)
                    .ok_or("Message not found")?;
                let quote = quote.filter(|q| !q.is_empty());
                if let Some(quote) = &quote {
                    validate_text("Quote", quote, 16_000)?;
                    if !message.body.contains(quote) {
                        return Err("Quote is not a passage from this message".into());
                    }
                }
                self.comments.push(Comment {
                    id: format!("comment-{id}"),
                    message_id,
                    quote,
                    author: author.trim().into(),
                    body: body.trim().into(),
                    created_at: now,
                });
            }
        }
        self.revision = self.revision.checked_add(1).ok_or("Revision exhausted")?;
        Ok(())
    }
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{label} must be nonempty and at most {max} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod connection_command_tests {
    use super::*;

    #[test]
    fn connection_update_contract_accepts_only_identity_and_display_name() {
        let value = serde_json::json!({"type": "update_connection", "connection_id": "resource", "name": "Source"});
        let command: Command = serde_json::from_value(value.clone()).unwrap();
        assert!(
            matches!(&command, Command::UpdateConnection { connection_id, name } if connection_id == "resource" && name == "Source")
        );
        assert_eq!(serde_json::to_value(command).unwrap(), value);
        for field in [
            "path",
            "remote",
            "kind",
            "board_id",
            "enabled",
            "project_id",
        ] {
            let mut widened = value.clone();
            widened[field] = serde_json::json!("changed");
            assert!(
                serde_json::from_value::<Command>(widened).is_err(),
                "{field}"
            );
        }
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;

    #[test]
    fn archive_defaults_for_legacy_sessions_and_round_trips() {
        let snapshot = demo_snapshot(AgentProfile::default());
        let mut json = serde_json::to_value(&snapshot.sessions[0]).unwrap();
        json.as_object_mut().unwrap().remove("archived");
        let mut session: Session = serde_json::from_value(json).unwrap();
        assert!(!session.archived);
        session.archived = true;
        assert!(
            serde_json::from_value::<Session>(serde_json::to_value(session).unwrap())
                .unwrap()
                .archived
        );
    }

    #[test]
    fn archive_rejects_running_descendants_even_when_the_parent_is_idle() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        let mut parent = snapshot.sessions[0].clone();
        parent.id = "archive-parent".into();
        parent.parent_session_id = None;
        parent.worker = None;
        let mut child = parent.clone();
        child.id = "archive-child".into();
        child.parent_session_id = Some(parent.id.clone());
        child.worker = Some(WorkerRun {
            model: None,
            context_tokens: None,
            context_window: None,
            last_usage: None,
            harness: Harness::Codex,
            execution: None,
            status: WorkerStatus::Running,
            thread_id: None,
            worktree: None,
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
        });
        snapshot.sessions = vec![parent, child];
        assert!(!snapshot.can_archive_session("archive-parent"));
        assert!(
            snapshot
                .apply(
                    Command::SetSessionArchived {
                        session_id: "archive-parent".into(),
                        archived: true
                    },
                    "archive",
                    1
                )
                .is_err()
        );
        assert!(!snapshot.sessions[0].archived);
        snapshot.sessions[1].worker.as_mut().unwrap().status = WorkerStatus::Queued;
        assert!(!snapshot.can_archive_session("archive-parent"));
        snapshot.sessions[1].worker.as_mut().unwrap().status = WorkerStatus::Completed;
        assert!(snapshot.can_archive_session("archive-parent"));
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: "archive-parent".into(),
                    archived: true,
                },
                "archive",
                2,
            )
            .unwrap();
        assert!(snapshot.sessions[0].archived);
    }

    #[test]
    fn archive_and_delete_apply_to_descendants_and_preserve_ancestors_and_siblings() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        let mut ancestor = snapshot.sessions[0].clone();
        ancestor.id = "ancestor".into();
        ancestor.parent_session_id = None;
        ancestor.worker = None;
        let mut branch = ancestor.clone();
        branch.id = "branch".into();
        branch.parent_session_id = Some(ancestor.id.clone());
        let mut child = branch.clone();
        child.id = "child".into();
        child.parent_session_id = Some(branch.id.clone());
        let mut sibling = branch.clone();
        sibling.id = "sibling".into();
        snapshot.sessions = vec![ancestor, branch, child, sibling];
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: "branch".into(),
                    archived: true,
                },
                "archive",
                1,
            )
            .unwrap();
        assert!(!snapshot.sessions[0].archived);
        assert!(snapshot.sessions[1].archived && snapshot.sessions[2].archived);
        assert!(!snapshot.sessions[3].archived);
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: "branch".into(),
                    archived: false,
                },
                "unarchive",
                2,
            )
            .unwrap();
        assert!(snapshot.sessions.iter().all(|s| !s.archived));
        assert!(
            snapshot
                .apply(
                    Command::DeleteSessionBranch {
                        session_id: "branch".into()
                    },
                    "delete",
                    3
                )
                .is_err()
        );
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: "branch".into(),
                    archived: true,
                },
                "archive",
                4,
            )
            .unwrap();
        snapshot
            .apply(
                Command::DeleteSessionBranch {
                    session_id: "branch".into(),
                },
                "delete",
                5,
            )
            .unwrap();
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ancestor", "sibling"]
        );
    }

    #[test]
    fn archived_chats_keep_transcripts_and_can_be_unarchived() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        let id = snapshot.sessions[0].id.clone();
        if let Some(worker) = snapshot.sessions[0].worker.as_mut() {
            worker.status = WorkerStatus::Completed;
        }
        let messages = snapshot.messages.clone();
        let workspaces = snapshot.sessions[0].workspaces.clone();
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: id.clone(),
                    archived: true,
                },
                "archive",
                1,
            )
            .unwrap();
        assert!(snapshot.sessions[0].archived);
        assert_eq!(snapshot.messages, messages);
        assert_eq!(snapshot.sessions[0].workspaces, workspaces);
        snapshot
            .apply(
                Command::SetSessionArchived {
                    session_id: id,
                    archived: false,
                },
                "unarchive",
                2,
            )
            .unwrap();
        assert!(!snapshot.sessions[0].archived);
    }
}
