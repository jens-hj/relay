use crate::network::NetworkState;
use mosaic::prelude::*;
use relay_core::*;
use std::collections::BTreeSet;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Board,
    Sessions,
    Profiles,
    Settings,
    NewProject,
    Connections,
    Publish,
    AgentStart,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTarget {
    Defaults,
    Config(String),
    New,
}
#[derive(Clone)]
pub enum Saved {
    Buffer(Draft),
    #[cfg(test)]
    Comment(String),
    Profile,
    AgentTemplate,
    ChildAgent,
    Start(String),
    AgentStart {
        draft: Draft,
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
    pub server_build: State<Option<BuildInfo>>,
    pub dismissed_update: State<Option<BuildInfo>>,
    pub window: State<WindowHandle>,
    pub window_gesture: State<Option<crate::window_chrome::NativeGesture>>,
    pub harness_refresh: State<Option<UnboundedSender<()>>>,
    pub transport_requests: State<Option<UnboundedSender<StreamRequest>>>,
    pub history: State<HistoryState>,
    pub history_loading: State<bool>,
    pub history_error: State<String>,
    pub history_generation: State<u64>,
    pub history_session: State<String>,
    pub history_before: State<Option<String>>,
    pub syncing: State<bool>,
    pub active_run_ids: State<std::collections::BTreeMap<String, String>>,
    pub remote_search: State<Option<String>>,
    pub search_snippet: State<String>,
    pub search_query: State<String>,
    pub source_anchor: State<Option<relay_core::Anchor>>,
    pub harnesses: State<Vec<HarnessStatus>>,
    pub harness_error: State<String>,
    pub catalogs: State<std::collections::BTreeMap<String, Result<HarnessCatalog, String>>>,
    pub completion: State<Option<crate::command_ui::Completion>>,
    pub command_flow: State<String>,
    pub command_argument: State<String>,
    pub command_model: State<String>,
    pub buffer: State<crate::buffer::BufferState>,
    pub conversation_controller: State<Option<crate::conversation::Shared>>,
    pub buffer_requests: State<Option<UnboundedSender<crate::buffer_network::Request>>>,
    pub preferences: State<crate::settings::Preferences>,
    pub collapsed_agents: State<BTreeSet<String>>,
    pub agent_focus: State<Option<String>>,
    pub mobile_navigation: State<bool>,
    pub browser_top_clearance: State<f32>,
    pub browser_viewport: State<Option<Size>>,
    pub sidebar_open: State<bool>,
    pub show_archived: State<bool>,
    pub settings_store: State<crate::settings::Store>,
    pub snapshot: State<Snapshot>,
    pub connected: State<bool>,
    pub status: State<String>,
    pub server_endpoint: State<String>,
    pub round_trip_ms: State<Option<u64>>,
    pub connection_telemetry: State<(Vec<u64>, Option<u64>, Option<u64>)>,
    pub notice: State<String>,
    pub command_feedback: State<String>,
    pub busy: State<bool>,
    confirmed_selection: State<Option<(String, HarnessSelection)>>,
    pub page: State<Page>,
    pub project: State<String>,
    pub discovery_requests: State<Option<UnboundedSender<BoardSource>>>,
    pub discovery: State<crate::project_network::DiscoveryUpdate>,
    pub recovery_requests: State<Option<UnboundedSender<crate::project_network::RecoveryRequest>>>,
    pub recovery: State<crate::project_network::RecoveryUpdate>,
    pub lookup_requests: State<Option<UnboundedSender<crate::project_network::LookupRequest>>>,
    pub lookup: State<crate::project_network::LookupUpdate>,
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
    pub editor_base: State<AgentProfile>,
    pub editor_profile: State<AgentProfile>,
    pub editor_overrides: State<ProfileOverrides>,
    pub editor_revision: State<u64>,
    pub editor_parent: State<Option<String>>,
    pub editor_template: State<String>,
    pub editor_template_name: State<String>,
    pub editor_override_accepted: State<bool>,
    pub toml: State<String>,
    pub advanced: State<bool>,
    pub worker_prompt: State<String>,
    pub worker_director: State<String>,
    pub worker_approval: State<bool>,
    pub review_changes: State<bool>,
    pub ui: State<Ui>,
    commands: State<UnboundedSender<CommandEnvelope>>,
    pending: State<Option<Pending>>,
    pending_conflict: State<bool>,
    pending_ambiguous: State<bool>,
    search_serial: State<u64>,
    outcome_serial: State<u64>,
}

impl Model {
    pub fn new(ui: &Ui, commands: UnboundedSender<CommandEnvelope>) -> Self {
        Self {
            window: State::new(AppContext::detached().window()),
            window_gesture: State::new(None),
            harness_refresh: State::new(None),
            transport_requests: State::new(None),
            history: State::new(Default::default()),
            history_loading: State::new(false),
            history_error: State::new(String::new()),
            history_generation: State::new(0),
            history_session: State::new(String::new()),
            history_before: State::new(None),
            syncing: State::new(false),
            active_run_ids: State::new(Default::default()),
            remote_search: State::new(None),
            search_snippet: State::new(String::new()),
            search_query: State::new(String::new()),
            source_anchor: State::new(None),
            harnesses: State::new(vec![]),
            harness_error: State::new(String::new()),
            server_build: State::new(None),
            dismissed_update: State::new(None),
            catalogs: State::new(Default::default()),
            completion: State::new(None),
            command_flow: State::new(String::new()),
            command_argument: State::new(String::new()),
            command_model: State::new(String::new()),
            buffer: State::new(crate::buffer::BufferState::default()),
            conversation_controller: State::new(None),
            buffer_requests: State::new(None),
            preferences: State::new(crate::settings::Preferences::default()),
            collapsed_agents: State::new(BTreeSet::new()),
            agent_focus: State::new(None),
            mobile_navigation: State::new(false),
            browser_top_clearance: State::new(crate::platform::top_clearance()),
            browser_viewport: State::new(None),
            sidebar_open: State::new(false),
            show_archived: State::new(false),
            settings_store: State::new(crate::settings::Store::default()),
            snapshot: State::new(Snapshot::default()),
            connected: State::new(false),
            status: State::new("Connecting…".into()),
            server_endpoint: State::new(String::new()),
            round_trip_ms: State::new(None),
            connection_telemetry: State::new((Vec::new(), None, None)),
            notice: State::new(String::new()),
            command_feedback: State::new(String::new()),
            busy: State::new(false),
            confirmed_selection: State::new(None),
            page: State::new(Page::Board),
            project: State::new(String::new()),
            discovery_requests: State::new(None),
            discovery: State::new(Default::default()),
            recovery_requests: State::new(None),
            recovery: State::new(Default::default()),
            lookup_requests: State::new(None),
            lookup: State::new(Default::default()),
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
            editor_parent: State::new(None),
            editor_template: State::new("director".into()),
            editor_template_name: State::new(String::new()),
            editor_override_accepted: State::new(false),
            editor_name: State::new(String::new()),
            editor_base: State::new(AgentProfile::default()),
            editor_profile: State::new(AgentProfile::default()),
            editor_overrides: State::new(ProfileOverrides::default()),
            editor_revision: State::new(0),
            toml: State::new(String::new()),
            advanced: State::new(false),
            worker_prompt: State::new(String::new()),
            worker_director: State::new(String::new()),
            worker_approval: State::new(false),
            review_changes: State::new(false),
            ui: State::new(ui.clone()),
            commands: State::new(commands),
            pending: State::new(None),
            pending_conflict: State::new(false),
            pending_ambiguous: State::new(false),
            search_serial: State::new(0),
            outcome_serial: State::new(0),
        }
    }
    pub fn available_update(&self) -> Option<BuildInfo> {
        fn revision(build: &BuildInfo) -> Option<&str> {
            let value = build
                .revision
                .strip_suffix("-local")
                .or_else(|| build.revision.strip_suffix("-dirty"))
                .unwrap_or(&build.revision);
            (value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then_some(value)
        }
        let deployed = self.server_build.get()?;
        let loaded = BuildInfo::current();
        let same = revision(&loaded)?.eq_ignore_ascii_case(revision(&deployed)?)
            && loaded.version == deployed.version;
        (!same && self.dismissed_update.get().as_ref() != Some(&deployed)).then_some(deployed)
    }

    pub fn receive(&self, update: NetworkState) {
        self.harnesses.set(update.harnesses.clone());
        self.harness_error.set(update.harness_error.clone());
        // Failed polls and reconnects do not erase the last deployed identity.
        if let Some(build) = &update.server_build {
            self.server_build.set(Some(build.clone()));
        }
        let select = !update
            .snapshot
            .projects
            .iter()
            .any(|p| p.id == self.project.get_untracked());
        self.active_run_ids
            .set(update.history.active_run_ids.clone());
        let mut snapshot = update.snapshot;
        let current = update.history.generation == self.history_generation.get_untracked()
            && update.history.session_id == self.history_session.get_untracked();
        if current {
            let before = self.history.get_untracked();
            let page_arrived = if let Some(cursor) = self.history_before.get_untracked() {
                update.history.oldest_message_id.as_ref() != Some(&cursor)
            } else {
                before.newest_message_id != update.history.newest_message_id
                    || before.session_id != update.history.session_id
            };
            if page_arrived || !update.history_error.is_empty() {
                self.history_loading.set(false);
                self.history_before.set(None);
            }
            self.history.set(update.history.clone());
            self.history_error.set(update.history_error.clone());
        } else if self.transport_requests.get_untracked().is_some() {
            let previous = self.snapshot.get_untracked();
            snapshot.messages = previous.messages;
            snapshot.comments = previous.comments;
        }
        self.syncing
            .set(snapshot.revision < update.acknowledged_revision);
        if !self.syncing.get_untracked() {
            self.confirmed_selection.set(None);
        }
        let removed_configs: Vec<_> = self
            .snapshot
            .get_untracked()
            .agent_configs
            .iter()
            .filter(|old| !snapshot.agent_configs.iter().any(|c| c.id == old.id))
            .map(|c| c.id.clone())
            .collect();
        let mut removed: std::collections::HashSet<_> = self
            .snapshot
            .get_untracked()
            .sessions
            .iter()
            .filter(|old| !snapshot.sessions.iter().any(|s| s.id == old.id))
            .map(|s| s.id.clone())
            .collect();
        if !removed.is_empty() {
            snapshot
                .messages
                .retain(|m| !removed.contains(&m.session_id));
            let message_ids: std::collections::HashSet<_> =
                snapshot.messages.iter().map(|m| m.id.clone()).collect();
            snapshot
                .comments
                .retain(|c| message_ids.contains(&c.message_id));
        }
        self.snapshot.set(snapshot);
        for id in &removed_configs {
            removed.insert(format!("agent-draft-{id}"));
            removed.insert(format!("config:{id}"));
            self.collapsed_agents.update(|ids| {
                ids.remove(&format!("config:{id}"));
            });
            if self.page.get_untracked() == Page::Profiles
                && self.editor.get_untracked() == EditTarget::Config(id.clone())
            {
                self.select_project(self.project.get_untracked());
                self.editor.set(EditTarget::Defaults);
            }
        }
        if !removed.is_empty() {
            crate::buffer::forget_sessions(*self, &removed);
            if self
                .agent_focus
                .get_untracked()
                .is_some_and(|id| removed.contains(&id))
            {
                self.agent_focus.set(None);
            }
            if removed.contains(&self.session.get_untracked()) {
                self.select_project(self.project.get_untracked());
            }
        }
        let new_search = update.search_serial > self.search_serial.get_untracked();
        if new_search {
            self.search_serial.set(update.search_serial);
        }
        if new_search
            && let Some((session, generation, query, message, snippet)) = update.search_result
            && session == self.session.get_untracked()
            && generation == self.history_generation.get_untracked()
            && query == self.search.get_untracked()
        {
            self.search_snippet.set(snippet);
            self.remote_search.set(message.clone());
            if let Some(id) = message {
                self.focused_message.set(id);
            }
        }

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
        if !update.connected {
            self.history_loading.set(false);
            self.history_before.set(None);
        }
        self.status.set(if self.syncing.get_untracked() {
            "Syncing…".into()
        } else {
            update.status
        });
        self.round_trip_ms.set(update.round_trip_ms);
        self.connection_telemetry.set((
            update.round_trip_samples,
            update.offline_since,
            update.retry_at,
        ));
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
        match result {
            Ok(()) => {
                self.acknowledge(pending, &id);
                self.notice.set(String::new());
            }
            Err(message) => {
                self.busy.set(false);
                if !update.outcome_ambiguous
                    && let Saved::Buffer(draft) = &pending.saved
                {
                    crate::buffer::rejected(*self, draft, &message);
                }
                self.pending_conflict.set(update.outcome_conflict);
                self.pending_ambiguous.set(update.outcome_ambiguous);
                // A confirmed rejection cannot be fixed by replaying its envelope.
                // Keep conflicts for explicit review and uncertain writes for exact retry.
                if !update.outcome_conflict && !update.outcome_ambiguous {
                    self.pending.set(None);
                }
                self.notice.set(message);
            }
        }
    }
    fn acknowledge(&self, pending: Pending, id: &str) {
        if self.syncing.get_untracked()
            && let Command::SetHarnessSelection {
                session_id,
                selection,
            } = &pending.envelope.command
        {
            self.confirmed_selection
                .set(Some((session_id.clone(), selection.clone())));
        }
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
            Saved::AgentStart { draft } => {
                crate::buffer::acknowledge_first(*self, &draft, &format!("session-{id}"));
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
                    if !self
                        .snapshot
                        .get_untracked()
                        .agent_configs
                        .iter()
                        .any(|c| c.id == format!("agent-config-{id}"))
                    {
                        return;
                    }
                    self.editor
                        .set(EditTarget::Config(format!("agent-config-{id}")));
                    self.open_root_agent(format!("agent-config-{id}"));
                }
                self.editor_revision
                    .set(self.snapshot.get_untracked().revision);
            }
            Saved::ChildAgent => {
                if !self
                    .snapshot
                    .get_untracked()
                    .sessions
                    .iter()
                    .any(|s| s.id == format!("session-{id}"))
                {
                    return;
                }
                self.open_session(format!("session-{id}"));
            }
            Saved::AgentTemplate => {
                let snapshot = self.snapshot.get_untracked();
                let Some(template) = snapshot
                    .agent_templates
                    .iter()
                    .find(|p| p.id == format!("profile-{id}"))
                else {
                    return;
                };
                self.editor_template.set(format!("profile-{id}"));
                self.editor_base.set(template.profile.clone());
                self.editor_overrides.set(ProfileOverrides::between(
                    &template.profile,
                    &self.editor_profile.get_untracked(),
                ));
                self.editor_revision
                    .set(self.snapshot.get_untracked().revision);
                self.editor_override_accepted.set(false);
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
            Saved::Start(_) | Saved::AgentStart { .. } => snapshot
                .sessions
                .iter()
                .any(|s| s.id == format!("session-{id}")),
            Saved::ChildAgent => snapshot
                .sessions
                .iter()
                .any(|s| s.id == format!("session-{id}")),
            Saved::AgentTemplate => snapshot
                .agent_templates
                .iter()
                .any(|p| p.id == format!("profile-{id}")),
            Saved::Profile
                if matches!(pending.envelope.command, Command::CreateAgentConfig { .. }) =>
            {
                snapshot
                    .agent_configs
                    .iter()
                    .any(|c| c.id == format!("agent-config-{id}"))
            }
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
            self.notice.set(String::new());
        }
    }
    /// Whether a command would be sent now: [`Model::submit`] drops it while a
    /// save or sync is running or the connection is down.
    pub fn can_submit(&self) -> bool {
        !self.busy.get_untracked() && !self.syncing.get_untracked() && self.connected.get_untracked()
    }
    /// Whether `connection` for the project is the request now awaiting
    /// acknowledgement, so a submit that was just made actually queued it.
    pub fn queued_connection(&self, project_id: &str, connection: &ConnectionInput) -> bool {
        self.pending.get_untracked().is_some_and(|pending| {
            matches!(&pending.envelope.command, Command::AddConnection { project_id: id, connection: queued }
                if id == project_id && queued == connection)
        })
    }
    pub fn submit(&self, command: Command, revision: u64, saved: Saved) {
        if self.busy.get_untracked() {
            return;
        }
        if self.syncing.get_untracked() {
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
        // A submitted retry replaces the decision notice with acknowledgement state.
        if self.pending.get_untracked().is_some() {
            self.notice.set(String::new());
        }
        self.pending_conflict.set(false);
        self.pending_ambiguous.set(false);
        self.pending.set(Some(Pending {
            envelope: envelope.clone(),
            saved,
        }));
        self.busy.set(true);
        if self.commands.get_untracked().send(envelope).is_err() {
            self.busy.set(false);
            self.notice
                .set("Network worker stopped. Your draft is retained.".into());
        }
    }
    /// The requested effort of an in-flight selection command for this chat.
    pub fn pending_effort(&self, session: &str) -> Option<String> {
        if !self.busy.get() {
            return None;
        }
        self.pending
            .get()
            .and_then(|pending| match pending.envelope.command {
                Command::SetHarnessSelection {
                    session_id,
                    selection,
                } if session_id == session => selection.effort,
                _ => None,
            })
    }

    /// Acknowledged effort while the authoritative snapshot catches up.
    pub fn acknowledged_effort(&self, session: &str) -> Option<String> {
        self.confirmed_selection
            .get()
            .filter(|(id, _)| id == session)
            .and_then(|(_, selection)| selection.effort)
    }

    pub fn harness_selection(&self) -> HarnessSelection {
        let id = self.session.get();
        if self.busy.get()
            && let Some(pending) = self.pending.get()
            && let Command::SetHarnessSelection {
                session_id,
                selection,
            } = pending.envelope.command
            && session_id == id
        {
            return selection;
        }
        if let Some((_, selection)) = self
            .confirmed_selection
            .get()
            .filter(|(session, _)| *session == id)
        {
            return selection;
        }
        let snapshot = self.snapshot.get();
        let id = self.session.get();
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.selection.clone())
            .or_else(|| {
                id.strip_prefix("agent-draft-")
                    .and_then(|id| snapshot.agent_configs.iter().find(|c| c.id == id))
                    .map(|c| {
                        let mut selection = c.draft_selection.clone();
                        if selection.model.is_none() {
                            selection.model = snapshot
                                .effective_profile(c)
                                .ok()
                                .and_then(|p| p.model_selection().model);
                        }
                        selection
                    })
            })
            .unwrap_or_default()
    }

    pub fn can_retry(&self) -> bool {
        self.pending.get().is_some() && !self.busy.get()
    }
    /// Envelopes and submitted drafts awaiting persistence/turn acknowledgements.
    /// Retained conflicts and ambiguous outcomes require a decision, not a spinner.
    pub fn sending(&self) -> bool {
        (self.busy.get() && self.pending.get().is_some())
            || (!self.can_retry()
                && self.buffer.get().documents.values().any(|doc| {
                    !doc.conflict
                        && doc.error.is_empty()
                        && (doc.finalize.is_some() || doc.submitting)
                }))
    }
    pub fn submission_pending(&self) -> bool {
        (self.busy.get()
            && self
                .pending
                .get()
                .is_some_and(|pending| match pending.saved {
                    Saved::Buffer(draft) | Saved::AgentStart { draft } => {
                        draft.session_id == self.session.get()
                    }
                    _ => false,
                }))
            || (!self.can_retry()
                && self
                    .buffer
                    .get()
                    .documents
                    .get(&self.session.get())
                    .is_some_and(|doc| {
                        !doc.conflict
                            && doc.error.is_empty()
                            && (doc.finalize.is_some() || doc.submitting)
                    }))
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
                    | Command::StartRootAgent {
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
        let base = match self.editor.get_untracked() {
            EditTarget::Config(id) => snapshot
                .agent_configs
                .iter()
                .find(|c| c.id == id)
                .and_then(|c| snapshot.config_base(c).ok()),
            EditTarget::Defaults => snapshot
                .project(&self.project.get_untracked())
                .ok()
                .map(|p| p.defaults.clone()),
            EditTarget::New => None,
        };
        if let Some(base) = base {
            self.editor_base.set(base.clone());
            if matches!(self.editor.get_untracked(), EditTarget::Config(_)) {
                self.editor_profile
                    .set(self.editor_overrides.get_untracked().resolve(&base));
            }
        }
        self.notice
            .set("Latest state loaded. Your draft is retained; review it before saving.".into());
    }
    pub fn open_profile(&self, target: EditTarget) {
        self.sidebar_open.set(false);
        if self.busy.get_untracked() {
            self.notice
                .set("Wait for the save to finish before switching profiles.".into());
            return;
        }
        let snapshot = self.snapshot.get_untracked();
        let Ok(project) = snapshot.project(&self.project.get_untracked()) else {
            return;
        };
        let mut base = project.defaults.clone();
        let mut template = "director".to_string();
        let (name, overrides) = match &target {
            EditTarget::Config(id) => {
                let Some(director) = snapshot.agent_configs.iter().find(|d| &d.id == id) else {
                    return;
                };
                let Ok(config_base) = snapshot.config_base(director) else {
                    return;
                };
                base = config_base;
                template = director.template.clone();
                (
                    if director.automatic_name {
                        String::new()
                    } else {
                        director.name.clone()
                    },
                    director.overrides.clone(),
                )
            }
            EditTarget::New => (String::new(), ProfileOverrides::default()),
            EditTarget::Defaults => ("Project defaults".into(), ProfileOverrides::default()),
        };
        self.editor_profile.set(overrides.resolve(&base));
        self.editor_base.set(base);
        self.editor_overrides.set(overrides);
        self.editor_name.set(name);
        self.editor_revision.set(snapshot.revision);
        self.editor.set(target);
        self.editor_parent.set(None);
        self.editor_template.set(template);
        self.editor_template_name.set(String::new());
        self.editor_override_accepted.set(false);
        // A TOML draft belongs to the profile loaded above. Hiding and showing
        // its editor preserves the draft; loading a profile starts a new one.
        self.toml.set(String::new());
        self.advanced.set(false);
        self.page.set(Page::Profiles);
    }
    pub fn open_new_worker(&self, parent_id: String) {
        let snapshot = self.snapshot.get_untracked();
        let Some(parent) = snapshot.sessions.iter().find(|s| s.id == parent_id) else {
            return;
        };
        self.project.set(parent.project_id.clone());
        self.open_profile(EditTarget::New);
        self.editor_parent.set(Some(parent_id));
        self.worker_prompt.set(String::new());
        self.worker_approval.set(false);
        self.select_agent_template("worker".into());
    }
    pub fn select_agent_template(&self, id: String) {
        let snapshot = self.snapshot.get_untracked();
        let base = if let Some(parent) = self.editor_parent.get_untracked() {
            let Some(parent) = snapshot.sessions.iter().find(|s| s.id == parent) else {
                return;
            };
            let Ok(parent_profile) = snapshot.session_profile(parent) else {
                return;
            };
            let Ok(project) = snapshot.project(&parent.project_id) else {
                return;
            };
            let mut profile = project.defaults.clone();
            profile.scope = parent_profile.scope;
            profile.execution = parent_profile.execution;
            profile
        } else {
            let Ok(project) = snapshot.project(&self.project.get_untracked()) else {
                return;
            };
            project.defaults.clone()
        };
        let profile = match id.as_str() {
            "director" => base,
            "worker" => AgentProfile::worker(&base),
            _ => {
                let Some(template) = snapshot
                    .agent_templates
                    .iter()
                    .find(|p| p.id == id && p.project_id == self.project.get_untracked())
                else {
                    return;
                };
                template.profile.clone()
            }
        };
        self.editor_template.set(id);
        self.editor_base.set(profile.clone());
        self.editor_profile.set(profile);
        self.editor_overrides.set(Default::default());
        self.editor_override_accepted.set(false);
        self.toml.set(String::new());
    }
    pub fn agent_template_name(&self) -> String {
        match self.editor_template.get().as_str() {
            "director" => "Director".into(),
            "worker" => "Worker".into(),
            id => self
                .snapshot
                .get()
                .agent_templates
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Agent".into()),
        }
    }
    pub fn discard_profile_changes(&self) {
        self.editor_profile.set(self.editor_base.get_untracked());
        self.editor_overrides.set(Default::default());
        self.editor_override_accepted.set(false);
        self.toml.set(String::new());
    }
    pub fn save_agent_template(&self) {
        self.submit(
            Command::SaveAgentTemplate {
                project_id: self.project.get_untracked(),
                name: self.editor_template_name.get_untracked(),
                profile: self.editor_profile.get_untracked(),
            },
            self.editor_revision.get_untracked(),
            Saved::AgentTemplate,
        );
    }
    pub fn select_project(&self, id: String) {
        self.sidebar_open.set(false);
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
                .agent_configs
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
    pub fn request_history(&self, request: StreamRequest) -> bool {
        self.transport_requests
            .get_untracked()
            .is_some_and(|sender| sender.send(request).is_ok())
    }
    pub fn subscribe_history(&self, session: String) {
        if self.history_session.get_untracked() == session {
            return;
        }
        let generation = self.history_generation.get_untracked() + 1;
        self.history_generation.set(generation);
        self.history_session.set(session.clone());
        self.history.set(HistoryState::default());
        self.history_loading.set(!session.is_empty());
        self.history_before.set(None);
        self.history_error.set(String::new());
        self.remote_search.set(None);
        self.search_snippet.set(String::new());
        self.search_query.set(String::new());
        self.request_history(StreamRequest::Subscribe {
            session_id: session,
            generation,
        });
    }
    pub fn load_older(&self) -> bool {
        let history = self.history.get_untracked();
        if self.history_loading.get_untracked()
            || !history.has_more
            || history.session_id != self.session.get_untracked()
        {
            return false;
        }
        let Some(before) = history.oldest_message_id else {
            return false;
        };
        if !self.request_history(StreamRequest::Older {
            session_id: history.session_id,
            generation: history.generation,
            before: before.clone(),
        }) {
            return false;
        }
        self.history_loading.set(true);
        self.history_before.set(Some(before));
        self.history_error.set(String::new());
        true
    }
    pub fn retry_history(&self) {
        self.history_loading.set(false);
        self.history_error.set(String::new());
        if !self.load_older() {
            self.request_history(StreamRequest::Subscribe {
                session_id: self.history_session.get_untracked(),
                generation: self.history_generation.get_untracked(),
            });
            self.history_loading.set(true);
        }
    }
    pub fn load_newer(&self) -> bool {
        let history = self.history.get_untracked();
        if self.history_loading.get_untracked()
            || !history.has_newer
            || history.session_id != self.session.get_untracked()
        {
            return false;
        }
        let Some(after) = history.newest_message_id else {
            return false;
        };
        if !self.request_history(StreamRequest::Newer {
            session_id: history.session_id,
            generation: history.generation,
            after,
        }) {
            return false;
        }
        self.history_loading.set(true);
        true
    }
    pub fn load_message(&self, id: String) {
        self.request_history(StreamRequest::Detail {
            session_id: self.session.get_untracked(),
            generation: self.history_generation.get_untracked(),
            message_id: id,
        });
    }
    pub fn search_history(&self, backwards: bool) {
        let query = self.search.get_untracked();
        if self.search_query.get_untracked() != query {
            self.remote_search.set(None);
            self.search_snippet.set(String::new());
            self.search_query.set(query.clone());
        }
        self.request_history(StreamRequest::Search {
            session_id: self.session.get_untracked(),
            generation: self.history_generation.get_untracked(),
            query,
            after: self.remote_search.get_untracked(),
            backwards,
        });
    }
    pub fn open_session(&self, id: String) {
        self.command_feedback.set(String::new());
        self.sidebar_open.set(false);
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
                ids.insert(session.agent_config_id.clone());
            });
            self.worker_director.set(session.agent_config_id.clone());
            if let Some(issue_id) = &session.issue_id {
                self.select_task_board(issue_id);
            }
            self.issue.set(session.issue_id.as_ref().map(|id| {
                self.snapshot
                    .get_untracked()
                    .canonical_issue_id(id)
                    .to_owned()
            }));
        }
        self.worker_approval.set(false);
        self.review_changes.set(false);
        self.session.set(id);
        self.page.set(Page::Sessions);
        self.search.set(String::new());
        self.focused_message.set(String::new());
    }
    pub fn open_root_agent(&self, id: String) {
        self.command_feedback.set(String::new());
        self.sidebar_open.set(false);
        let snapshot = self.snapshot.get_untracked();
        let Some(director) = snapshot.agent_configs.iter().find(|d| d.id == id) else {
            return;
        };
        if let Some(session) = snapshot.sessions.iter().rev().find(|s| {
            s.project_id == director.project_id
                && s.agent_config_id == id
                && s.role == SessionRole::Root
        }) {
            self.open_session(session.id.clone());
        } else {
            self.project.set(director.project_id.clone());
            let task = snapshot
                .effective_profile(director)
                .ok()
                .and_then(|profile| {
                    if let AgentScope::Issues { issue_ids } = profile.scope {
                        let current = self.issue.get_untracked();
                        current
                            .filter(|id| {
                                issue_ids.iter().any(|allowed| {
                                    snapshot.canonical_issue_id(allowed)
                                        == snapshot.canonical_issue_id(id)
                                })
                            })
                            .or_else(|| {
                                issue_ids
                                    .first()
                                    .map(|id| snapshot.canonical_issue_id(id).to_owned())
                            })
                    } else {
                        None
                    }
                });
            self.issue.set(task);
            self.session.set(format!("agent-draft-{id}"));
            self.worker_director.set(id);
            self.worker_approval.set(false);
            self.page.set(Page::AgentStart);
        }
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
            if let Some(issue_id) = &session.issue_id {
                self.select_task_board(issue_id);
            }
            self.issue.set(session.issue_id.as_ref().map(|id| {
                self.snapshot
                    .get_untracked()
                    .canonical_issue_id(id)
                    .to_owned()
            }));
            self.worker_director.set(session.agent_config_id.clone());
            self.worker_approval.set(false);
            self.page.set(Page::Board);
        }
    }
    pub fn scope_contains(&self, ids: &[String], id: &str) -> bool {
        let snapshot = self.snapshot.get();
        ids.iter().any(|candidate| {
            snapshot.canonical_issue_id(candidate) == snapshot.canonical_issue_id(id)
        })
    }
    pub fn sessions_for_task(&self, id: &str) -> Vec<Session> {
        let snapshot = self.snapshot.get();
        snapshot
            .sessions
            .iter()
            .filter(|s| {
                s.issue_id.as_deref().is_some_and(|task| {
                    snapshot.canonical_issue_id(task) == snapshot.canonical_issue_id(id)
                })
            })
            .cloned()
            .collect()
    }
    pub fn worker_profile(&self, continuation: bool) -> Result<(AgentProfile, usize), String> {
        let snapshot = self.snapshot.get();
        let (agent_config_id, issue_id) = if continuation {
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
                session.agent_config_id.clone(),
                session.issue_id.clone().ok_or("Session has no issue")?,
            )
        } else {
            (
                self.worker_director.get(),
                self.issue.get().ok_or("Select an issue")?,
            )
        };
        let issue = snapshot.issue(&issue_id)?;
        let project = snapshot.project(&issue.project_id)?;
        if project.fixture {
            return Err("Fixture issue: execution unavailable".into());
        }
        if !snapshot.visible_task(&issue.id) {
            return Err("Issue is no longer on this board. Restore it and sync before starting or continuing.".into());
        }
        let director = snapshot
            .agent_configs
            .iter()
            .find(|d| d.id == agent_config_id && d.project_id == issue.project_id)
            .ok_or("Select a project director")?;
        let profile = snapshot.effective_profile(director)?;
        // Standalone workers share the configuration's slots; delegated
        // children occupy their parents' slots instead.
        let active = snapshot
            .sessions
            .iter()
            .filter(|s| {
                s.agent_config_id == agent_config_id
                    && s.role == SessionRole::Worker
                    && s.parent_session_id.is_none()
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
        if matches!(&profile.scope, AgentScope::Issues { issue_ids } if !issue_id.is_some_and(|id| self.scope_contains(issue_ids, &id)))
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
        // A continued session needs only its parent's slot; its own children
        // never block it. A new board worker takes a standalone slot.
        let snapshot = self.snapshot.get();
        let free = match snapshot
            .sessions
            .iter()
            .find(|s| s.id == self.session.get())
        {
            Some(session) if continuation => snapshot.worker_slot(session).is_ok(),
            _ => active < usize::from(profile.max_workers),
        };
        if !free {
            return Err("No worker slots available".into());
        }
        if !continuation && profile.model.is_empty() {
            return Err(
                "Choose a model in the agent configuration before creating a worker".into(),
            );
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
                agent_config_id: self.worker_director.get_untracked(),
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
    fn select_task_board(&self, issue_id: &str) {
        let snapshot = self.snapshot.get_untracked();
        let issue_id = snapshot.canonical_issue_id(issue_id);
        let candidates: Vec<_> = snapshot
            .boards
            .iter()
            .filter(|b| {
                b.project_id == self.project.get_untracked()
                    && snapshot.board_active(&b.id)
                    && snapshot.memberships.iter().any(|m| {
                        m.board_id == b.id
                            && m.issue_id == issue_id
                            && m.column_ids
                                .iter()
                                .any(|id| b.columns.iter().any(|c| &c.id == id))
                    })
            })
            .collect();
        if self
            .selected_board()
            .is_some_and(|b| candidates.iter().any(|c| c.id == b.id))
        {
            return;
        }
        if let Some(board) = candidates.first() {
            self.preferences.update(|p| {
                p.selected_boards
                    .insert(board.project_id.clone(), board.id.clone());
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
                    && selected
                        .as_ref()
                        .is_some_and(|id| snapshot.canonical_board_id(id) == b.id)
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
            && let Some(issue) = self.snapshot.get_untracked().issue(issue_id).ok()
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
    /// Holds the open session's queued turns, or resumes held ones.
    pub fn toggle_queue(&self) {
        let snapshot = self.snapshot.get_untracked();
        let session_id = self.session.get_untracked();
        let held = snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == session_id && s.state == SubmissionState::Paused);
        let queued = snapshot
            .submissions
            .iter()
            .any(|s| s.session_id == session_id && s.state == SubmissionState::Queued);
        let command = if held {
            Command::ResumeQueue { session_id }
        } else if queued {
            Command::PauseQueue { session_id }
        } else {
            return;
        };
        self.submit(command, snapshot.revision, Saved::Action);
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
        // Only this session's turn stops; its descendants keep running.
        self.submit(
            Command::StopWorker {
                session_id: self.session.get_untracked(),
                include_descendants: false,
            },
            self.snapshot.get_untracked().revision,
            Saved::Action,
        );
    }
    /// Stops the selected session and every descendant, pausing their queues.
    pub fn stop_all(&self) {
        let snapshot = self.snapshot.get_untracked();
        let session_id = self.session.get_untracked();
        if snapshot.subtree_status(&session_id).active() == 0 {
            self.notice
                .set("Select an agent with active work to stop.".into());
            return;
        }
        self.submit(
            Command::StopWorker {
                session_id,
                include_descendants: true,
            },
            snapshot.revision,
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
        if self.editor.get_untracked() == EditTarget::New && profile.model.is_empty() {
            self.notice
                .set("Choose a model before creating this agent".into());
            return;
        }
        if let Err(error) = profile.validate() {
            self.notice.set(error);
            return;
        }
        let command = match self.editor.get_untracked() {
            EditTarget::Defaults => Command::UpdateProjectDefaults {
                project_id: self.project.get_untracked(),
                profile,
            },
            EditTarget::New => {
                if let Some(parent_session_id) = self.editor_parent.get_untracked() {
                    self.submit(
                        Command::StartChildAgent {
                            parent_session_id,
                            name: self.editor_name.get_untracked(),
                            profile,
                            prompt: String::new(),
                            approve_implementation: false,
                        },
                        self.editor_revision.get_untracked(),
                        Saved::ChildAgent,
                    );
                    return;
                }
                let snapshot = self.snapshot.get_untracked();
                let Ok(project) = snapshot.project(&self.project.get_untracked()) else {
                    return;
                };
                Command::CreateAgentConfig {
                    template: self.editor_template.get_untracked(),
                    project_id: project.id.clone(),
                    name: self.editor_name.get_untracked(),
                    overrides: ProfileOverrides::between(
                        &self.editor_base.get_untracked(),
                        &profile,
                    ),
                }
            }
            EditTarget::Config(agent_config_id) => Command::UpdateAgentConfig {
                agent_config_id,
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
    pub fn modify_profile(&self, field: &str, update: impl FnOnce(&mut AgentProfile)) {
        self.editor_override_accepted.set(false);
        let mut profile = self.editor_profile.get_untracked();
        let previous_harness = profile.harness;
        update(&mut profile);
        if field == "harness" && profile.harness != previous_harness {
            profile.model.clear();
        }
        if self.editor.get_untracked() == EditTarget::New {
            self.editor_overrides.set(ProfileOverrides::between(
                &self.editor_base.get_untracked(),
                &profile,
            ));
        } else if self.editor.get_untracked() != EditTarget::Defaults {
            self.editor_overrides.update(|overrides| match field {
                "execution" => overrides.execution = Some(profile.execution.clone()),
                "model" => overrides.model = Some(profile.model.clone()),
                "harness" => {
                    overrides.harness = Some(profile.harness);
                    if previous_harness != profile.harness {
                        overrides.model = None;
                    }
                }
                "scope" => overrides.scope = Some(profile.scope.clone()),
                "responsibilities" => {
                    overrides.responsibilities = Some(profile.responsibilities.clone())
                }
                "completion" => overrides.completion = Some(profile.completion.clone()),
                "max_workers" => overrides.max_workers = Some(profile.max_workers),
                "permissions" => overrides.permissions = Some(profile.permissions.clone()),
                "worker_models" => overrides.worker_models = Some(profile.worker_models.clone()),
                _ => unreachable!(),
            });
        }
        self.editor_profile.set(profile);
    }
    pub fn inherit(&self, field: &str) {
        self.editor_overrides.update(|o| match field {
            "execution" => o.execution = None,
            "model" => o.model = None,
            "harness" => {
                o.harness = None;
                o.model = None;
            }
            "scope" => o.scope = None,
            "responsibilities" => o.responsibilities = None,
            "completion" => o.completion = None,
            "max_workers" => o.max_workers = None,
            "permissions" => o.permissions = None,
            "worker_models" => o.worker_models = None,
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
            "model" => overrides.model.is_some(),
            "harness" => overrides.harness.is_some(),
            "scope" => overrides.scope.is_some(),
            "responsibilities" => overrides.responsibilities.is_some(),
            "completion" => overrides.completion.is_some(),
            "max_workers" => overrides.max_workers.is_some(),
            "permissions" => overrides.permissions.is_some(),
            "worker_models" => overrides.worker_models.is_some(),
            _ => false,
        };
        if overridden {
            "Agent override"
        } else if self.editor.get() == EditTarget::New || self.editor_template.get() != "director" {
            "Inherited from profile"
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
            AgentProfile::from_toml(&self.toml.get_untracked()).map(|p| self.editor_profile.set(p))
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
