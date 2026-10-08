use crate::network::NetworkState;
use mosaic::prelude::*;
use relay_core::*;
use std::collections::BTreeSet;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Board,
    Sessions,
    Directors,
    Settings,
    NewProject,
    Connections,
    Publish,
    DirectorStart,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTarget {
    Defaults,
    Director(String),
    New,
}
#[derive(Clone)]
pub enum Saved {
    Buffer(Draft),
    #[cfg(test)]
    Comment(String),
    Profile,
    Start(String),
    DirectorStart {
        director_id: String,
        prompt: String,
    },
    Send(String),
    Action,
    CreatedProject(crate::projects::ProjectDraft),
}
#[derive(Clone)]
struct Pending {
    envelope: CommandEnvelope,
    saved: Saved,
}

#[derive(Clone, Copy)]
pub struct Model {
    pub harness_refresh: State<Option<UnboundedSender<()>>>,
    pub harnesses: State<Vec<HarnessStatus>>,
    pub harness_error: State<String>,
    pub buffer: State<crate::buffer::BufferState>,
    pub buffer_requests: State<Option<UnboundedSender<crate::buffer_network::Request>>>,
    pub preferences: State<crate::settings::Preferences>,
    pub snapshot: State<Snapshot>,
    pub connected: State<bool>,
    pub status: State<String>,
    pub notice: State<String>,
    pub busy: State<bool>,
    pub page: State<Page>,
    pub project: State<String>,
    pub discovery_requests: State<Option<UnboundedSender<BoardSource>>>,
    pub discovery: State<crate::project_network::DiscoveryUpdate>,
    pub recovery_requests: State<Option<UnboundedSender<crate::project_network::RecoveryRequest>>>,
    pub recovery: State<crate::project_network::RecoveryUpdate>,
    pub publish_drafts: State<std::collections::BTreeMap<String, crate::projects::PublishDraft>>,
    pub project_draft: State<crate::projects::ProjectDraft>,
    pub workspace_selection: State<Option<Vec<String>>>,
    pub expanded_projects: State<BTreeSet<String>>,
    pub expanded_directors: State<BTreeSet<String>>,
    pub issue: State<Option<String>>,
    pub session: State<String>,
    pub focused_message: State<String>,
    pub search: State<String>,
    pub searching: State<bool>,
    pub palette: State<bool>,
    pub palette_query: State<String>,
    #[cfg(test)]
    pub comment_target: State<String>,
    #[cfg(test)]
    pub comment_quote: State<String>,
    #[cfg(test)]
    pub comment_body: State<String>,
    #[cfg(test)]
    pub author: State<String>,
    pub editor: State<EditTarget>,
    pub editor_name: State<String>,
    pub editor_base: State<DirectorProfile>,
    pub editor_profile: State<DirectorProfile>,
    pub editor_overrides: State<ProfileOverrides>,
    pub editor_revision: State<u64>,
    pub toml: State<String>,
    pub advanced: State<bool>,
    pub worker_prompt: State<String>,
    pub director_prompts: State<std::collections::BTreeMap<String, String>>,
    pub worker_director: State<String>,
    pub worker_approval: State<bool>,
    pub review_changes: State<bool>,
    pub ui: State<Ui>,
    commands: State<UnboundedSender<CommandEnvelope>>,
    pending: State<Option<Pending>>,
    pending_conflict: State<bool>,
    pending_ambiguous: State<bool>,
    outcome_serial: State<u64>,
}

impl Model {
    pub fn new(ui: &Ui, commands: UnboundedSender<CommandEnvelope>) -> Self {
        Self {
            harness_refresh: State::new(None),
            harnesses: State::new(vec![]),
            harness_error: State::new(String::new()),
            buffer: State::new(crate::buffer::BufferState::default()),
            buffer_requests: State::new(None),
            preferences: State::new(crate::settings::Preferences::default()),
            snapshot: State::new(Snapshot::default()),
            connected: State::new(false),
            status: State::new("Connecting…".into()),
            notice: State::new(String::new()),
            busy: State::new(false),
            page: State::new(Page::Board),
            project: State::new(String::new()),
            discovery_requests: State::new(None),
            discovery: State::new(Default::default()),
            recovery_requests: State::new(None),
            recovery: State::new(Default::default()),
            publish_drafts: State::new(Default::default()),
            project_draft: State::new(Default::default()),
            workspace_selection: State::new(None),
            expanded_projects: State::new(BTreeSet::new()),
            expanded_directors: State::new(BTreeSet::new()),
            issue: State::new(None),
            session: State::new(String::new()),
            focused_message: State::new(String::new()),
            search: State::new(String::new()),
            searching: State::new(false),
            palette: State::new(false),
            palette_query: State::new(String::new()),
            #[cfg(test)]
            comment_target: State::new(String::new()),
            #[cfg(test)]
            comment_quote: State::new(String::new()),
            #[cfg(test)]
            comment_body: State::new(String::new()),
            #[cfg(test)]
            author: State::new(std::env::var("RELAY_NAME").unwrap_or_else(|_| "Teammate".into())),
            editor: State::new(EditTarget::Defaults),
            editor_name: State::new(String::new()),
            editor_base: State::new(DirectorProfile::default()),
            editor_profile: State::new(DirectorProfile::default()),
            editor_overrides: State::new(ProfileOverrides::default()),
            editor_revision: State::new(0),
            toml: State::new(String::new()),
            advanced: State::new(false),
            worker_prompt: State::new(String::new()),
            director_prompts: State::new(Default::default()),
            worker_director: State::new(String::new()),
            worker_approval: State::new(false),
            review_changes: State::new(false),
            ui: State::new(ui.clone()),
            commands: State::new(commands),
            pending: State::new(None),
            pending_conflict: State::new(false),
            pending_ambiguous: State::new(false),
            outcome_serial: State::new(0),
        }
    }
    pub fn receive(&self, update: NetworkState) {
        self.harnesses.set(update.harnesses.clone());
        self.harness_error.set(update.harness_error.clone());
        let select = !update
            .snapshot
            .projects
            .iter()
            .any(|p| p.id == self.project.get_untracked());
        self.snapshot.set(update.snapshot);
        if select {
            let snapshot = self.snapshot.get_untracked();
            if let Some(project) = snapshot
                .projects
                .iter()
                .find(|p| !p.fixture)
                .or(snapshot.projects.first())
            {
                self.select_project(project.id.clone());
            }
        }
        self.connected.set(update.connected);
        self.status.set(update.status);
        self.reconcile_pending();
        if update.outcome_serial <= self.outcome_serial.get_untracked() {
            return;
        }
        self.outcome_serial.set(update.outcome_serial);
        let Some((id, result)) = update.outcome else {
            return;
        };
        let pending = self.pending.get_untracked();
        let Some(pending) = pending.filter(|p| p.envelope.request_id == id) else {
            return;
        };
        // An outcome is held in transport state until replaced; only consume it once.
        if !self.busy.get_untracked() {
            return;
        }
        self.busy.set(false);
        match result {
            Ok(()) => {
                self.acknowledge(pending, &id);
                self.notice.set("Server acknowledged the request.".into());
            }
            Err(message) => {
                if !update.outcome_ambiguous
                    && let Saved::Buffer(draft) = &pending.saved
                {
                    crate::buffer::rejected(*self, draft, &message);
                }
                self.pending_conflict.set(update.outcome_conflict);
                self.pending_ambiguous.set(update.outcome_ambiguous);
                self.notice.set(message);
            }
        }
    }
    fn acknowledge(&self, pending: Pending, id: &str) {
        match pending.saved {
            Saved::Buffer(draft) => crate::buffer::acknowledge(*self, &draft, id),
            #[cfg(test)]
            Saved::Comment(body) => {
                if self.comment_body.get_untracked() == body {
                    self.comment_body.set(String::new());
                    self.comment_quote.set(String::new());
                    self.comment_target.set(String::new());
                }
            }
            Saved::Start(body) => {
                self.clear_worker_draft(&body);
                self.open_session(format!("session-{id}"));
            }
            Saved::DirectorStart {
                director_id,
                prompt,
            } => {
                self.director_prompts.update(|drafts| {
                    if drafts.get(&director_id) == Some(&prompt) {
                        drafts.remove(&director_id);
                    }
                });
                self.open_session(format!("session-{id}"));
            }
            Saved::Send(body) => self.clear_worker_draft(&body),
            Saved::Action => {}
            Saved::CreatedProject(draft) => {
                if self.project_draft.get_untracked() == draft {
                    self.project_draft.set(Default::default());
                }
                self.select_project(format!("project-{id}"));
            }
            Saved::Profile => {
                if self.editor.get_untracked() == EditTarget::New {
                    self.editor
                        .set(EditTarget::Director(format!("director-{id}")));
                }
                self.editor_revision
                    .set(self.snapshot.get_untracked().revision);
            }
        }
        self.pending.set(None);
        self.pending_conflict.set(false);
        self.pending_ambiguous.set(false);
        self.busy.set(false);
    }
    fn reconcile_pending(&self) {
        let Some(pending) = self.pending.get_untracked() else {
            return;
        };
        let id = pending.envelope.request_id.clone();
        let snapshot = self.snapshot.get_untracked();
        let applied = match &pending.saved {
            Saved::CreatedProject(_) => snapshot
                .projects
                .iter()
                .any(|p| p.id == format!("project-{id}")),
            Saved::Buffer(_) => snapshot
                .submissions
                .iter()
                .any(|s| s.id == id || s.last_edit_request.as_deref() == Some(&id)),
            Saved::Start(_) | Saved::DirectorStart { .. } => snapshot
                .sessions
                .iter()
                .any(|s| s.id == format!("session-{id}")),
            Saved::Send(_) => snapshot
                .messages
                .iter()
                .any(|m| m.id == format!("prompt-{id}")),
            #[cfg(test)]
            Saved::Comment(_) => snapshot
                .comments
                .iter()
                .any(|c| c.id == format!("comment-{id}")),
            _ => false,
        };
        if applied {
            self.acknowledge(pending, &id);
            self.notice
                .set("Request confirmed by the workspace snapshot.".into());
        }
    }
    pub fn submit(&self, command: Command, revision: u64, saved: Saved) {
        if self.busy.get_untracked() {
            return;
        }
        if !self.connected.get_untracked() {
            self.notice
                .set("Reconnect before saving. Your draft is retained.".into());
            return;
        }
        let previous = self.pending.get_untracked();
        let command_json = serde_json::to_string(&command).unwrap();
        if self.pending_ambiguous.get_untracked()
            && previous.as_ref().is_some_and(|p| {
                serde_json::to_string(&p.envelope.command).unwrap() != command_json
            })
        {
            self.notice.set("An earlier request is unconfirmed. Retry its exact envelope before submitting another action; your draft is retained.".into());
            return;
        }
        let envelope = match previous {
            Some(p) if serde_json::to_string(&p.envelope.command).unwrap() == command_json => {
                p.envelope
            }
            _ => CommandEnvelope {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: revision,
                command,
            },
        };
        self.pending_conflict.set(false);
        self.pending_ambiguous.set(false);
        self.pending.set(Some(Pending {
            envelope: envelope.clone(),
            saved,
        }));
        self.busy.set(true);
        self.notice.set("Awaiting server acknowledgement…".into());
        if self.commands.get_untracked().send(envelope).is_err() {
            self.busy.set(false);
            self.notice
                .set("Network worker stopped. Your draft is retained.".into());
        }
    }
    pub fn can_retry(&self) -> bool {
        self.pending.get().is_some() && !self.busy.get()
    }
    pub fn retry_summary(&self) -> String {
        self.pending
            .get()
            .map(|p| {
                let content = match p.envelope.command {
                    Command::StartWorker {
                        prompt,
                        approve_implementation,
                        ..
                    }
                    | Command::StartSession {
                        prompt,
                        approve_implementation,
                        ..
                    }
                    | Command::StartDirector {
                        prompt,
                        approve_implementation,
                        ..
                    }
                    | Command::SendWorker {
                        prompt,
                        approve_implementation,
                        ..
                    } => format!(
                        "Prompt: {prompt} · implementation approval: {approve_implementation}"
                    ),
                    Command::SubmitTurn {
                        parts,
                        approve_implementation,
                        ..
                    } => format!(
                        "Message: {} · implementation approval: {approve_implementation}",
                        plain_text(&parts)
                    ),
                    Command::EditQueuedTurn { parts, .. } => {
                        format!("Queued message: {}", plain_text(&parts))
                    }
                    Command::AddComment { body, .. } => format!("Comment: {body}"),
                    _ => "Original workspace action".into(),
                };
                format!(
                    "Original request at revision {} · {}",
                    p.envelope.expected_revision,
                    content.chars().take(180).collect::<String>()
                )
            })
            .unwrap_or_default()
    }
    pub fn retry_pending(&self) {
        let Some(pending) = self.pending.get_untracked() else {
            return;
        };
        self.submit(
            pending.envelope.command,
            pending.envelope.expected_revision,
            pending.saved,
        );
    }
    pub fn can_rebase(&self) -> bool {
        self.pending_conflict.get() && self.can_retry()
    }
    pub fn rebase_conflict(&self) {
        if !self.can_rebase() {
            return;
        }
        self.review_latest();
        self.pending.set(None);
        self.pending_conflict.set(false);
        self.pending_ambiguous.set(false);
        self.notice.set("Conflict reviewed. Draft retained; submit from the form to create a new request at the latest revision.".into());
    }
    pub fn review_latest(&self) {
        if self.busy.get_untracked() {
            return;
        }
        let snapshot = self.snapshot.get_untracked();
        self.editor_revision.set(snapshot.revision);
        if let Ok(project) = snapshot.project(&self.project.get_untracked()) {
            self.editor_base.set(project.defaults.clone());
            if matches!(
                self.editor.get_untracked(),
                EditTarget::Director(_) | EditTarget::New
            ) {
                self.editor_profile.set(
                    self.editor_overrides
                        .get_untracked()
                        .resolve(&project.defaults),
                );
            }
        }
        self.notice
            .set("Latest state loaded. Your draft is retained; review it before saving.".into());
    }
    pub fn open_profile(&self, target: EditTarget) {
        if self.busy.get_untracked() {
            self.notice
                .set("Wait for the save to finish before switching profiles.".into());
            return;
        }
        let snapshot = self.snapshot.get_untracked();
        let Ok(project) = snapshot.project(&self.project.get_untracked()) else {
            return;
        };
        let (name, overrides) = match &target {
            EditTarget::Director(id) => {
                let Some(director) = snapshot.directors.iter().find(|d| &d.id == id) else {
                    return;
                };
                (director.name.clone(), director.overrides.clone())
            }
            EditTarget::New => ("New director".into(), ProfileOverrides::default()),
            EditTarget::Defaults => ("Project defaults".into(), ProfileOverrides::default()),
        };
        self.editor_profile
            .set(overrides.resolve(&project.defaults));
        self.editor_base.set(project.defaults.clone());
        self.editor_overrides.set(overrides);
        self.editor_name.set(name);
        self.editor_revision.set(snapshot.revision);
        self.editor.set(target);
        self.advanced.set(false);
        self.page.set(Page::Directors);
    }
    pub fn select_project(&self, id: String) {
        let snapshot = self.snapshot.get_untracked();
        let id = snapshot
            .bindings
            .iter()
            .find(|b| b.project_id() == id)
            .and_then(|b| {
                snapshot.projects.iter().find(|p| {
                    !p.fixture
                        && p.repository == b.repository
                        && p.github
                            .as_ref()
                            .is_some_and(|g| g.owner == b.owner && g.number == b.number)
                })
            })
            .map(|p| p.id.clone())
            .unwrap_or(id);
        self.expanded_projects.update(|ids| {
            ids.insert(id.clone());
        });
        self.project.set(id.clone());
        self.issue.set(None);
        self.worker_director.set(
            self.snapshot
                .get_untracked()
                .directors
                .iter()
                .find(|d| d.project_id == id)
                .map(|d| d.id.clone())
                .unwrap_or_default(),
        );
        self.worker_approval.set(false);
        self.workspace_selection.set(None);
        let snapshot = self.snapshot.get_untracked();
        self.session.set(
            snapshot
                .sessions
                .iter()
                .find(|s| s.project_id == id)
                .map(|s| s.id.clone())
                .unwrap_or_default(),
        );
        self.page.set(Page::Board);
    }
    pub fn open_session(&self, id: String) {
        if let Some(session) = self
            .snapshot
            .get_untracked()
            .sessions
            .iter()
            .find(|session| session.id == id)
        {
            self.project.set(session.project_id.clone());
            self.expanded_projects.update(|ids| {
                ids.insert(session.project_id.clone());
            });
            self.expanded_directors.update(|ids| {
                ids.insert(session.director_id.clone());
            });
            self.worker_director.set(session.director_id.clone());
            self.issue.set(session.issue_id.clone());
        }
        self.worker_approval.set(false);
        self.review_changes.set(false);
        self.session.set(id);
        self.page.set(Page::Sessions);
        self.search.set(String::new());
        self.focused_message.set(String::new());
    }
    pub fn open_director(&self, id: String) {
        let snapshot = self.snapshot.get_untracked();
        let Some(director) = snapshot.directors.iter().find(|d| d.id == id) else {
            return;
        };
        if let Some(session) = snapshot.sessions.iter().rev().find(|s| {
            s.project_id == director.project_id
                && s.director_id == id
                && s.role == SessionRole::Director
        }) {
            self.open_session(session.id.clone());
        } else {
            self.project.set(director.project_id.clone());
            self.worker_director.set(id);
            self.page.set(Page::DirectorStart);
        }
    }
    pub fn open_director_profile(&self, id: String) {
        let snapshot = self.snapshot.get_untracked();
        let Some(director) = snapshot.directors.iter().find(|d| d.id == id) else {
            return;
        };
        if self.project.get_untracked() != director.project_id {
            self.select_project(director.project_id.clone());
        }
        self.expanded_projects.update(|ids| {
            ids.insert(director.project_id.clone());
        });
        self.open_profile(EditTarget::Director(id));
    }
    fn clear_worker_draft(&self, body: &str) {
        if self.worker_prompt.get_untracked() == body {
            self.worker_prompt.set(String::new());
        }
        self.worker_approval.set(false);
    }
    pub fn open_worker_issue(&self) {
        let snapshot = self.snapshot.get_untracked();
        if let Some(session) = snapshot
            .sessions
            .iter()
            .find(|s| s.id == self.session.get_untracked())
        {
            self.project.set(session.project_id.clone());
            self.issue.set(session.issue_id.clone());
            self.worker_director.set(session.director_id.clone());
            self.worker_approval.set(false);
            self.page.set(Page::Board);
        }
    }
    pub fn worker_profile(&self, continuation: bool) -> Result<(DirectorProfile, usize), String> {
        let snapshot = self.snapshot.get();
        let (director_id, issue_id) = if continuation {
            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == self.session.get())
                .ok_or("Select a session")?;
            let worker = session
                .worker
                .as_ref()
                .ok_or("Fixture sessions cannot run workers")?;
            if matches!(worker.status, WorkerStatus::Running | WorkerStatus::Queued) {
                return Err("Worker is active; stop or wait before continuing".into());
            }
            if worker.thread_id.is_none() || worker.worktree.is_none() {
                return Err(
                    "This worker cannot continue. Open the linked issue to start a new worker."
                        .into(),
                );
            }
            (
                session.director_id.clone(),
                session.issue_id.clone().ok_or("Session has no issue")?,
            )
        } else {
            (
                self.worker_director.get(),
                self.issue.get().ok_or("Select an issue")?,
            )
        };
        let issue = snapshot
            .issues
            .iter()
            .find(|i| i.id == issue_id)
            .ok_or("Issue is unavailable")?;
        let project = snapshot.project(&issue.project_id)?;
        if project.fixture {
            return Err("Fixture issue: execution unavailable".into());
        }
        if !snapshot.visible_task(&issue.id) {
            return Err("Issue is no longer on this board. Restore it and sync before starting or continuing.".into());
        }
        let director = snapshot
            .directors
            .iter()
            .find(|d| d.id == director_id && d.project_id == issue.project_id)
            .ok_or("Select a project director")?;
        let profile = snapshot.effective_profile(director)?;
        let active = snapshot
            .sessions
            .iter()
            .filter(|s| {
                s.director_id == director_id
                    && s.worker.as_ref().is_some_and(|w| {
                        matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)
                    })
            })
            .count();
        Ok((profile, active))
    }
    pub fn worker_gate(&self, continuation: bool) -> Result<(), String> {
        let (profile, active) = self.worker_profile(continuation)?;
        let issue_id = if continuation {
            self.snapshot
                .get()
                .sessions
                .iter()
                .find(|s| s.id == self.session.get())
                .and_then(|s| s.issue_id.clone())
        } else {
            self.issue.get()
        };
        if matches!(&profile.scope, DirectorScope::Issues { issue_ids } if !issue_id.is_some_and(|id| issue_ids.contains(&id)))
        {
            return Err("Issue is outside director scope".into());
        }
        match profile
            .permissions
            .get(&Task::Implement)
            .copied()
            .unwrap_or(Permission::Deny)
        {
            Permission::Deny => return Err("Implementation denied by effective profile".into()),
            Permission::Ask if !self.worker_approval.get() => {
                return Err("Approve implementation for this turn".into());
            }
            _ => {}
        }
        if active >= usize::from(profile.max_workers) {
            return Err("No worker slots available".into());
        }
        if self.worker_prompt.get().trim().is_empty() {
            return Err("Enter a prompt".into());
        }
        if self.busy.get() || !self.connected.get() {
            return Err("Wait for a connected, idle client".into());
        }
        Ok(())
    }
    pub fn run_worker(&self, continuation: bool) {
        if let Err(error) = self.worker_gate(continuation) {
            self.notice.set(error);
            return;
        }
        let prompt = self.worker_prompt.get_untracked();
        let command = if continuation {
            Command::SendWorker {
                session_id: self.session.get_untracked(),
                prompt: prompt.clone(),
                approve_implementation: self.worker_approval.get_untracked(),
            }
        } else {
            Command::StartSession {
                issue_id: self.issue.get_untracked(),
                role: SessionRole::Worker,
                connection_ids: self.workspace_selection.get_untracked(),
                director_id: self.worker_director.get_untracked(),
                prompt: prompt.clone(),
                approve_implementation: self.worker_approval.get_untracked(),
            }
        };
        self.submit(
            command,
            self.snapshot.get_untracked().revision,
            if continuation {
                Saved::Send(prompt)
            } else {
                Saved::Start(prompt)
            },
        );
    }
    pub fn discover_destination(&self, source: BoardSource) {
        self.discovery.set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: None,
        });
        if self
            .discovery_requests
            .get_untracked()
            .is_none_or(|sender| sender.send(source.clone()).is_err())
        {
            self.discovery.set(crate::project_network::DiscoveryUpdate {
                source: Some(source),
                result: Some(Err(
                    "Cannot read destination statuses. Reconnect and try again.".into(),
                )),
            });
        }
    }
    pub fn selected_board(&self) -> Option<Board> {
        let snapshot = self.snapshot.get();
        let project = self.project.get();
        let selected = self
            .preferences
            .get()
            .selected_boards
            .get(&project)
            .cloned();
        snapshot
            .boards
            .iter()
            .find(|b| {
                b.project_id == project
                    && snapshot.board_active(&b.id)
                    && selected.as_ref() == Some(&b.id)
            })
            .or_else(|| {
                snapshot
                    .boards
                    .iter()
                    .find(|b| b.project_id == project && snapshot.board_active(&b.id))
            })
            .cloned()
    }
    pub fn board_columns(&self) -> Vec<BoardColumn> {
        self.selected_board().map(|b| b.columns).unwrap_or_else(|| {
            if self
                .snapshot
                .get()
                .boards
                .iter()
                .any(|b| b.project_id == self.project.get())
            {
                return Vec::new();
            }
            self.snapshot
                .get()
                .projects
                .iter()
                .find(|p| p.id == self.project.get())
                .map(|p| p.columns.clone())
                .unwrap_or_default()
        })
    }
    pub fn task_in_column(&self, issue: &Issue, column: &str) -> bool {
        if issue.project_id != self.project.get() {
            return false;
        }
        if let Some(board) = self.selected_board() {
            self.snapshot.get().memberships.iter().any(|m| {
                m.board_id == board.id
                    && m.issue_id == issue.id
                    && m.column_ids.iter().any(|c| c == column)
            })
        } else {
            !self
                .snapshot
                .get()
                .boards
                .iter()
                .any(|b| b.project_id == issue.project_id)
                && issue.column_id == column
        }
    }
    pub fn action(&self, mut command: Command) {
        if let Command::UpdateTask { issue_id, body, .. } = &mut command
            && let Some(issue) = self
                .snapshot
                .get_untracked()
                .issues
                .iter()
                .find(|i| &i.id == issue_id)
        {
            *body = crate::projects::preserve_task_markers(body, &issue.body);
        }
        if let Command::UpdateTask { issue_id, .. } = &command
            && !self.snapshot.get_untracked().visible_task(issue_id)
        {
            self.notice
                .set("Restore the task's board connection before editing.".into());
            return;
        }
        let board_id = match &command {
            Command::CreateTask { board_id, .. }
            | Command::MoveTask { board_id, .. }
            | Command::UpdateBoardColumns { board_id, .. }
            | Command::SyncBoard { board_id }
            | Command::PublishBoard { board_id, .. } => Some(board_id),
            _ => None,
        };
        if board_id.is_some_and(|id| !self.snapshot.get_untracked().board_active(id)) {
            self.notice.set(
                "This board connection is disabled. Restore it before starting new board work."
                    .into(),
            );
            return;
        }
        self.submit(
            command,
            self.snapshot.get_untracked().revision,
            Saved::Action,
        );
    }
    pub fn sync_project(&self) {
        if let Some(board) = self.selected_board() {
            self.action(Command::SyncBoard { board_id: board.id });
            return;
        }
        if self
            .snapshot
            .get_untracked()
            .boards
            .iter()
            .any(|b| b.project_id == self.project.get_untracked())
        {
            self.notice
                .set("Restore a board connection before syncing.".into());
            return;
        }
        if !self
            .snapshot
            .get_untracked()
            .projects
            .iter()
            .any(|p| p.id == self.project.get_untracked() && p.github.is_some())
        {
            self.notice.set("Select a remote project to sync.".into());
            return;
        }
        self.submit(
            Command::SyncProject {
                project_id: self.project.get_untracked(),
            },
            self.snapshot.get_untracked().revision,
            Saved::Action,
        );
    }
    pub fn stop_worker(&self) {
        if !self.snapshot.get_untracked().sessions.iter().any(|s| {
            s.id == self.session.get_untracked()
                && s.worker.as_ref().is_some_and(|w| {
                    matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)
                })
        }) {
            self.notice.set("Select an active worker to stop.".into());
            return;
        }
        self.submit(
            Command::StopWorker {
                session_id: self.session.get_untracked(),
            },
            self.snapshot.get_untracked().revision,
            Saved::Action,
        );
    }
    #[cfg(test)]
    pub fn start_comment(&self, message: &Message) {
        if !self.comment_body.get_untracked().trim().is_empty()
            && self.comment_target.get_untracked() != message.id
        {
            self.notice.set(
                "Send or discard your current comment before choosing another message.".into(),
            );
            return;
        }
        self.comment_target.set(message.id.clone());
        let quote = self
            .ui
            .get_untracked()
            .selected_text()
            .filter(|q| !q.is_empty() && message.body.contains(q))
            .unwrap_or_default();
        self.comment_quote.set(quote);
    }
    #[cfg(test)]
    pub fn save_comment(&self) {
        let body = self.comment_body.get_untracked();
        self.submit(
            Command::AddComment {
                message_id: self.comment_target.get_untracked(),
                quote: Some(self.comment_quote.get_untracked()).filter(|q| !q.is_empty()),
                author: self.author.get_untracked(),
                body: body.clone(),
            },
            self.snapshot.get_untracked().revision,
            Saved::Comment(body),
        );
    }
    pub fn save_profile(&self) {
        let profile = self.editor_profile.get_untracked();
        if let Err(error) = profile.validate() {
            self.notice.set(error);
            return;
        }
        let command = match self.editor.get_untracked() {
            EditTarget::Defaults => Command::UpdateProjectDefaults {
                project_id: self.project.get_untracked(),
                profile,
            },
            EditTarget::New => Command::CreateDirector {
                project_id: self.project.get_untracked(),
                name: self.editor_name.get_untracked(),
                overrides: self.editor_overrides.get_untracked(),
            },
            EditTarget::Director(director_id) => Command::UpdateDirector {
                director_id,
                name: self.editor_name.get_untracked(),
                overrides: self.editor_overrides.get_untracked(),
            },
        };
        self.submit(
            command,
            self.editor_revision.get_untracked(),
            Saved::Profile,
        );
    }
    pub fn modify_profile(&self, field: &str, update: impl FnOnce(&mut DirectorProfile)) {
        let mut profile = self.editor_profile.get_untracked();
        update(&mut profile);
        if self.editor.get_untracked() != EditTarget::Defaults {
            self.editor_overrides.update(|overrides| match field {
                "execution" => overrides.execution = Some(profile.execution.clone()),
                "harness" => overrides.harness = Some(profile.harness),
                "scope" => overrides.scope = Some(profile.scope.clone()),
                "responsibilities" => {
                    overrides.responsibilities = Some(profile.responsibilities.clone())
                }
                "completion" => overrides.completion = Some(profile.completion.clone()),
                "max_workers" => overrides.max_workers = Some(profile.max_workers),
                "permissions" => overrides.permissions = Some(profile.permissions.clone()),
                _ => unreachable!(),
            });
        }
        self.editor_profile.set(profile);
    }
    pub fn inherit(&self, field: &str) {
        self.editor_overrides.update(|o| match field {
            "execution" => o.execution = None,
            "harness" => o.harness = None,
            "scope" => o.scope = None,
            "responsibilities" => o.responsibilities = None,
            "completion" => o.completion = None,
            "max_workers" => o.max_workers = None,
            "permissions" => o.permissions = None,
            _ => unreachable!(),
        });
        self.editor_profile.set(
            self.editor_overrides
                .get_untracked()
                .resolve(&self.editor_base.get_untracked()),
        );
    }
    pub fn origin(&self, field: &str) -> &'static str {
        if self.editor.get() == EditTarget::Defaults {
            return "Project default";
        }
        let overrides = self.editor_overrides.get();
        let overridden = match field {
            "execution" => overrides.execution.is_some(),
            "harness" => overrides.harness.is_some(),
            "scope" => overrides.scope.is_some(),
            "responsibilities" => overrides.responsibilities.is_some(),
            "completion" => overrides.completion.is_some(),
            "max_workers" => overrides.max_workers.is_some(),
            "permissions" => overrides.permissions.is_some(),
            _ => false,
        };
        if overridden {
            "Director override"
        } else {
            "Inherited from project"
        }
    }
    pub fn export_toml(&self) {
        self.toml
            .set(if self.editor.get_untracked() == EditTarget::Defaults {
                self.editor_profile.get_untracked().to_toml()
            } else {
                self.editor_overrides.get_untracked().to_toml()
            });
        self.advanced.set(true);
    }
    pub fn import_toml(&self) {
        let result = if self.editor.get_untracked() == EditTarget::Defaults {
            DirectorProfile::from_toml(&self.toml.get_untracked())
                .map(|p| self.editor_profile.set(p))
        } else {
            ProfileOverrides::from_toml(&self.toml.get_untracked()).and_then(|o| {
                let profile = o.resolve(&self.editor_base.get_untracked());
                profile.validate()?;
                self.editor_overrides.set(o);
                self.editor_profile.set(profile);
                Ok(())
            })
        };
        self.notice.set(match result {
            Ok(()) => "TOML imported into the draft. Save to persist.".into(),
            Err(error) => error,
        });
    }
}
