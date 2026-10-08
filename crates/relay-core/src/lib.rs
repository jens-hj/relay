//! Relay's domain and versioned client/server contract. No UI or execution dependencies.

mod conversation;
mod projects;
pub use projects::*;
mod execution;
pub use execution::*;
mod fixture;
mod profile;
pub use conversation::*;

pub use fixture::demo_snapshot;
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
    pub defaults: DirectorProfile,
    #[serde(default)]
    pub github: Option<GitHubProject>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Director {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub overrides: ProfileOverrides,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Director,
    Worker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub connection_ids: Vec<String>,
    #[serde(default)]
    pub workspaces: Vec<SessionWorkspace>,
    pub id: String,
    pub project_id: String,
    pub issue_id: Option<String>,
    pub director_id: String,
    pub title: String,
    pub role: SessionRole,
    pub fixture: bool,
    #[serde(default)]
    pub worker: Option<WorkerRun>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub author: String,
    pub kind: String,
    pub body: String,
    #[serde(default)]
    pub parts: Vec<Part>,
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
    pub directors: Vec<Director>,
    pub sessions: Vec<Session>,
    pub messages: Vec<Message>,
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub submissions: Vec<Submission>,
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
    CreateProject {
        name: String,
        root: String,
        connections: Vec<ConnectionInput>,
    },
    RenameProject {
        project_id: String,
        name: String,
    },
    AddConnection {
        project_id: String,
        connection: ConnectionInput,
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
        director_id: String,
        issue_id: Option<String>,
        role: SessionRole,
        prompt: String,
        approve_implementation: bool,
        connection_ids: Option<Vec<String>>,
    },
    StartDirector {
        director_id: String,
        prompt: String,
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
    SyncProject {
        project_id: String,
    },
    StartWorker {
        issue_id: String,
        director_id: String,
        prompt: String,
        approve_implementation: bool,
    },
    SendWorker {
        session_id: String,
        prompt: String,
        approve_implementation: bool,
    },
    StopWorker {
        session_id: String,
    },
    UpdateProjectDefaults {
        project_id: String,
        profile: DirectorProfile,
    },
    CreateDirector {
        project_id: String,
        name: String,
        overrides: ProfileOverrides,
    },
    UpdateDirector {
        director_id: String,
        name: String,
        overrides: ProfileOverrides,
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

    pub fn effective_profile(&self, director: &Director) -> Result<DirectorProfile, String> {
        Ok(director
            .overrides
            .resolve(&self.project(&director.project_id)?.defaults))
    }

    pub fn validate_profile(
        &self,
        project_id: &str,
        profile: &DirectorProfile,
    ) -> Result<(), String> {
        self.project(project_id)?;
        profile.validate()?;
        if let DirectorScope::Issues { issue_ids } = &profile.scope {
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

    /// Apply only to a candidate snapshot: the server commits it atomically after validation.
    pub fn apply(&mut self, command: Command, id: &str, now: u64) -> Result<(), String> {
        match command {
            Command::CreateProject { .. }
            | Command::RenameProject { .. }
            | Command::AddConnection { .. }
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
            | Command::StartDirector { .. }
            | Command::SetSessionConnections { .. }
            | Command::ConfigureProject { .. }
            | Command::ConfigureHarness { .. }
            | Command::RespondPermission { .. }
            | Command::SetWorkerExecution { .. }
            | Command::SyncProject { .. }
            | Command::StartWorker { .. }
            | Command::SendWorker { .. }
            | Command::SubmitTurn { .. }
            | Command::PromoteTurn { .. }
            | Command::CancelTurn { .. }
            | Command::EditQueuedTurn { .. }
            | Command::ResumeQueue { .. }
            | Command::StopWorker { .. } => {
                return Err("This command requires the server runtime".into());
            }
            Command::UpdateProjectDefaults {
                project_id,
                profile,
            } => {
                self.validate_profile(&project_id, &profile)?;
                for director in self.directors.iter().filter(|d| d.project_id == project_id) {
                    self.validate_profile(&project_id, &director.overrides.resolve(&profile))?;
                }
                self.projects
                    .iter_mut()
                    .find(|p| p.id == project_id)
                    .unwrap()
                    .defaults = profile;
            }
            Command::CreateDirector {
                project_id,
                name,
                overrides,
            } => {
                validate_text("Director name", &name, 100)?;
                self.validate_profile(
                    &project_id,
                    &overrides.resolve(&self.project(&project_id)?.defaults),
                )?;
                self.directors.push(Director {
                    id: format!("director-{id}"),
                    project_id,
                    name: name.trim().into(),
                    overrides,
                });
            }
            Command::UpdateDirector {
                director_id,
                name,
                overrides,
            } => {
                validate_text("Director name", &name, 100)?;
                let director = self
                    .directors
                    .iter()
                    .find(|d| d.id == director_id)
                    .ok_or("Director not found")?;
                self.validate_profile(
                    &director.project_id,
                    &overrides.resolve(&self.project(&director.project_id)?.defaults),
                )?;
                let director = self
                    .directors
                    .iter_mut()
                    .find(|d| d.id == director_id)
                    .unwrap();
                director.name = name.trim().into();
                director.overrides = overrides;
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
