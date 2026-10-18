use crate::*;

pub fn demo_snapshot(defaults: AgentProfile) -> Snapshot {
    let project = Project {
        root: None,
        id: "demo".into(),
        name: "Relay · demo".into(),
        repository: "demo/relay".into(),
        fixture: true,
        columns: [
            ("backlog", "Backlog"),
            ("in_progress", "In progress"),
            ("review", "In review"),
        ]
        .into_iter()
        .map(|(id, title)| BoardColumn {
            id: id.into(),
            title: title.into(),
        })
        .collect(),
        defaults,
        github: None,
    };
    let issues = [
        ("issue-1", 1, "Connect the project board", "backlog", "integration", "Mirror the authoritative remote board and preserve its column order and issue identity. Relay adds agent activity alongside the existing workflow.", None),
        ("issue-2", 2, "Make director profiles declarative", "in_progress", "workflow", "Define project defaults and per-director overrides. Let several agent_configs specialize within a project and expose their effective configuration.", None),
        ("issue-3", 3, "Review conversations in context", "review", "experience", "Attach feedback to the exact message or quoted passage that prompted it. Keep the conversation and its issue links intact across sessions.", Some("Fixture result: a contextual comment prototype is ready for review.")),
        ("issue-4", 4, "Expose context and usage controls", "backlog", "efficiency", "Support compact, reset context, and archive/start-new as separate actions. Report cache state only when the connected harness provides evidence.", None),
    ].into_iter().map(|(id, number, title, column, label, body, result)| Issue {
        id: id.into(), project_id: "demo".into(), repository_connection_id: None, reference: Some(IssueRef {
            provider: Provider::Github, repository: "demo/relay".into(), number,
            url: format!("https://github.com/demo/relay/issues/{number}"),
        }), title: title.into(), body: body.into(), column_id: column.into(), labels: vec![label.into()], result: result.map(String::from),
    }).collect();
    let agent_configs = vec![
        AgentConfig {
            automatic_name: false,
            draft_selection: HarnessSelection::default(),
            template: "director".into(),
            id: "director-main".into(),
            project_id: "demo".into(),
            name: "Project director".into(),
            overrides: ProfileOverrides::default(),
        },
        AgentConfig {
            automatic_name: false,
            draft_selection: HarnessSelection::default(),
            template: "director".into(),
            id: "director-review".into(),
            project_id: "demo".into(),
            name: "Review director".into(),
            overrides: ProfileOverrides {
                harness: Some(Harness::ClaudeCode),
                scope: Some(AgentScope::Issues {
                    issue_ids: vec!["issue-3".into()],
                }),
                responsibilities: Some(vec![Task::Verify, Task::Review]),
                max_workers: Some(1),
                ..Default::default()
            },
        },
    ];
    let sessions = vec![
        session(
            "session-plan",
            None,
            "director-main",
            Some("issue-2"),
            "Profile design",
            SessionRole::Root,
            None,
        ),
        session(
            "session-worker",
            None,
            "director-main",
            Some("issue-2"),
            "Profile validation",
            SessionRole::Worker,
            None,
        ),
        session(
            "session-review",
            None,
            "director-review",
            Some("issue-3"),
            "Contextual review",
            SessionRole::Root,
            None,
        ),
    ];
    let messages = [
        (
            "m1",
            "session-plan",
            "You",
            "prompt",
            "Let agent_configs inherit project defaults, while keeping their explicit overrides intact when those defaults change.",
        ),
        (
            "m2",
            "session-plan",
            "Project director",
            "plan",
            "I will separate project defaults from director overrides.\n\n1. Define a typed profile for scope, responsibilities, completion, and permissions.\n2. Delegate validation to a worker.\n3. Return the effective profile and identify which values are inherited.",
        ),
        (
            "m3",
            "session-plan",
            "Project director",
            "handoff",
            "Profile validation is delegated to the linked worker session. Both conversations remain attached to issue #2. No agent is running in this fixture.",
        ),
        (
            "m4",
            "session-worker",
            "Worker",
            "message",
            "Validation checks worker limits, required permissions, and issue scope. Unknown TOML fields are rejected to catch misspelled configuration.",
        ),
        (
            "m5",
            "session-worker",
            "Worker",
            "result",
            "Fixture verification: inherited fields follow project defaults; explicit director fields remain unchanged. A zero worker limit represents paused delegation.",
        ),
        (
            "m6",
            "session-review",
            "Review director",
            "review",
            "Feedback should reference the original message, with an optional selected quote. The transcript remains immutable while comments accumulate alongside it.",
        ),
        (
            "m7",
            "session-review",
            "You",
            "prompt",
            "Keep review keyboard-friendly. I should be able to jump between messages and comment without losing my place.",
        ),
    ];
    let messages = messages
        .into_iter()
        .map(|(id, session, author, kind, body)| Message {
            id: id.into(),
            session_id: session.into(),
            author: author.into(),
            sender: Sender::User,
            kind: kind.into(),
            body: body.into(),
            parts: vec![],
            tool: None,
        })
        .collect();
    Snapshot {
        agent_templates: vec![],
        board_aliases: Default::default(),
        issue_aliases: Default::default(),
        protocol_version: PROTOCOL_VERSION,
        connections: vec![],
        boards: vec![],
        memberships: vec![],
        operations: vec![],
        bindings: vec![],
        installations: vec![],
        tool_permissions: vec![],
        revision: 0,
        projects: vec![project],
        issues,
        agent_configs,
        sessions,
        messages,
        comments: vec![],
        submissions: vec![],
        usage_limits: vec![],
        usage_ledger: vec![],
    }
}

/// Turns the demo workspace into a running orchestration: the director
/// spawned a tree of agents more than four levels deep, one of them waits
/// for approval two levels down, and the director's transcript holds one
/// call of every tool kind. Its states are examples; fixture sessions never
/// run, so only the isolated demo applies this.
pub fn demo_orchestration(snapshot: &mut Snapshot) {
    let plan = run(
        Harness::ClaudeCode,
        "claude-opus-4-5",
        WorkerStatus::Running,
    )
    .context(124_000, 200_000);
    for existing in &mut snapshot.sessions {
        match existing.id.as_str() {
            "session-plan" => {
                existing.worker = Some(plan.clone());
                existing.selection.model = Some("claude-opus-4-5".into());
                existing.selection.effort = Some("high".into());
                existing.compactions = [Some(182_000), Some(176_000)]
                    .into_iter()
                    .map(|at_tokens| Compaction { at_tokens })
                    .collect();
            }
            "session-worker" => {
                existing.parent_session_id = Some("session-plan".into());
                existing.worker = Some(run(
                    Harness::Codex,
                    "gpt-5.1-codex",
                    WorkerStatus::Completed,
                ));
                existing.spawn = Some(grant(
                    "Validate worker limits, permissions and issue scope.",
                ));
            }
            _ => {}
        }
    }
    let review = snapshot
        .sessions
        .iter()
        .position(|s| s.id == "session-review")
        .unwrap_or(snapshot.sessions.len());
    let spawned = [
        (
            "session-migration",
            "session-plan",
            "Schema migration",
            Harness::ClaudeCode,
            "claude-sonnet-4-5",
            WorkerStatus::Running,
            "Add the profile override columns and migrate existing agent_configs.",
        ),
        (
            "session-write",
            "session-migration",
            "Write migrations",
            Harness::ClaudeCode,
            "claude-sonnet-4-5",
            WorkerStatus::Running,
            "Write the migration for the six profile tables.",
        ),
        (
            "session-backfill",
            "session-write",
            "Backfill script",
            Harness::Codex,
            "gpt-5.1-codex",
            WorkerStatus::Running,
            "Backfill override columns from existing profiles.",
        ),
        (
            "session-verify",
            "session-backfill",
            "Verify backfill",
            Harness::Codex,
            "gpt-5.1-codex-mini",
            WorkerStatus::Queued,
            "Compare row counts before and after the backfill.",
        ),
        (
            "session-index",
            "session-verify",
            "Index check",
            Harness::ClaudeCode,
            "claude-haiku-4-5",
            WorkerStatus::Completed,
            "Check that the new columns are indexed.",
        ),
        (
            "session-tests",
            "session-migration",
            "Migration tests",
            Harness::Codex,
            "gpt-5.1-codex",
            WorkerStatus::Failed,
            "Run the migration tests against a copy of the database.",
        ),
    ]
    .into_iter()
    .map(|(id, parent, title, harness, model, status, task)| {
        let mut child = session(
            id,
            Some(parent),
            "director-main",
            Some("issue-2"),
            title,
            SessionRole::Worker,
            Some(run(harness, model, status)),
        );
        child.spawn = Some(grant(task));
        if id == "session-migration" {
            child.worker = child.worker.map(|w| w.context(41_000, 200_000));
        }
        child
    });
    snapshot.sessions.splice(review..review, spawned);
    // The effort each example agent runs at; the demo catalog offers it.
    for (id, effort) in [
        ("session-plan", "high"),
        ("session-worker", "medium"),
        ("session-migration", "medium"),
        ("session-write", "high"),
        ("session-backfill", "medium"),
        ("session-verify", "low"),
        ("session-index", "low"),
        ("session-tests", "high"),
    ] {
        if let Some(session) = snapshot.sessions.iter_mut().find(|s| s.id == id) {
            session.selection.model = session.worker.as_ref().and_then(|w| w.model.clone());
            session.selection.effort = Some(effort.into());
        }
    }
    snapshot.messages.extend(tool_messages());
    snapshot.tool_permissions.push(ToolPermission {
        id: "permission-migrate".into(),
        session_id: "session-write".into(),
        run_id: "run-session-write".into(),
        tool: "Bash".into(),
        description: "psql -f migrations/0012_profile_overrides.sql".into(),
        decision: None,
        expired: false,
    });
}

fn grant(task: &str) -> SpawnGrant {
    SpawnGrant {
        task: task.into(),
        context: None,
        permissions: [(Task::Implement, Permission::Allow)].into(),
        requested_by_run: None,
    }
}

fn session(
    id: &str,
    parent: Option<&str>,
    director: &str,
    issue: Option<&str>,
    title: &str,
    role: SessionRole,
    worker: Option<WorkerRun>,
) -> Session {
    Session {
        automatic_name: false,
        awaiting_first_message: false,
        archived: false,
        profile: None,
        workspaces: vec![],
        selection: Default::default(),
        conversations: vec![],
        connection_ids: vec![],
        id: id.into(),
        project_id: "demo".into(),
        issue_id: issue.map(Into::into),
        agent_config_id: director.into(),
        title: title.into(),
        role,
        fixture: true,
        worker,
        parent_session_id: parent.map(Into::into),
        spawn: None,
        compactions: vec![],
    }
}

fn run(harness: Harness, model: &str, status: WorkerStatus) -> WorkerRun {
    WorkerRun {
        model: Some(model.into()),
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness,
        execution: None,
        status,
        thread_id: None,
        worktree: None,
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    }
}

impl WorkerRun {
    fn context(mut self, tokens: u64, window: u64) -> Self {
        self.context_tokens = Some(tokens);
        self.context_window = Some(window);
        self
    }
}

/// One call of every tool kind in the director's transcript, with the
/// details a harness reports.
fn tool_messages() -> Vec<Message> {
    let call = |id: &str,
                session: &str,
                name: &str,
                kind: ToolKind,
                state: ToolState,
                target: &str,
                detail: Option<ToolDetail>,
                output: &str| Message {
        id: id.into(),
        session_id: session.into(),
        author: "Agent".into(),
        sender: Sender::User,
        kind: "tool_call".into(),
        body: output.into(),
        parts: vec![],
        tool: Some(ToolCall {
            run_id: format!("run-{session}"),
            name: name.into(),
            kind,
            state,
            input: String::new(),
            target: target.into(),
            directory: None,
            exit_code: None,
            started_ms: None,
            finished_ms: None,
            detail,
        }),
    };
    use ToolState::{Completed, Running};
    vec![
        call(
            "t-read",
            "session-plan",
            "Read",
            ToolKind::Read,
            Completed,
            "crates/relay-core/src/profile.rs",
            Some(ToolDetail::Read {
                start_line: Some(1),
                end_line: Some(40),
                total_lines: Some(243),
            }),
            "pub struct AgentProfile {\n    pub harness: Harness,\n    pub scope: AgentScope,\n    pub max_workers: u16,\n}",
        ),
        call(
            "t-search",
            "session-plan",
            "Grep",
            ToolKind::Search,
            Completed,
            "resolve(",
            Some(ToolDetail::Search {
                query: "resolve(".into(),
                files: vec![
                    SearchFile {
                        path: "crates/relay-core/src/profile.rs".into(),
                        hits: 4,
                    },
                    SearchFile {
                        path: "crates/relay-desktop/src/model.rs".into(),
                        hits: 3,
                    },
                    SearchFile {
                        path: "crates/relay-server/src/commands.rs".into(),
                        hits: 1,
                    },
                ],
            }),
            "",
        ),
        call(
            "t-edit",
            "session-plan",
            "Edit",
            ToolKind::Edit,
            Completed,
            "crates/relay-core/src/profile.rs",
            Some(ToolDetail::Edit {
                hunks: vec![
                    Hunk {
                        line: 118,
                        label: Some("fn resolve".into()),
                        added: 9,
                        removed: 4,
                    },
                    Hunk {
                        line: 201,
                        label: None,
                        added: 3,
                        removed: 0,
                    },
                ],
                added: 12,
                removed: 4,
            }),
            "",
        ),
        call(
            "t-create",
            "session-plan",
            "Write",
            ToolKind::Create,
            Completed,
            "docs/profiles.md",
            Some(ToolDetail::Create { lines: 42 }),
            "# Director profiles\n\nA director inherits every field it does not override.",
        ),
        call(
            "t-run",
            "session-plan",
            "Bash",
            ToolKind::Run,
            Completed,
            "cargo check -p relay-core",
            Some(ToolDetail::Run {
                exit_code: Some(0),
                duration_ms: Some(8_400),
            }),
            "    Checking relay-core v0.1.0\n    Finished `dev` profile in 8.40s",
        ),
        call(
            "t-test",
            "session-plan",
            "Bash",
            ToolKind::Test,
            ToolState::Failed,
            "cargo test -p relay-core profile",
            Some(ToolDetail::Test {
                passed: Some(47),
                failed: Some(1),
                failures: vec!["profile::tests::overrides_survive_default_changes".into()],
            }),
            "test result: FAILED. 47 passed; 1 failed",
        ),
        call(
            "t-fetch",
            "session-plan",
            "WebFetch",
            ToolKind::Fetch,
            Completed,
            "https://toml.io/en/v1.0.0",
            Some(ToolDetail::Fetch {
                status: Some(200),
                title: Some("TOML v1.0.0".into()),
            }),
            "",
        ),
        call(
            "t-plan",
            "session-plan",
            "TodoWrite",
            ToolKind::Plan,
            Completed,
            "Profile overrides",
            Some(ToolDetail::Plan {
                steps: vec![
                    PlanStep {
                        title: "Define the profile type".into(),
                        done: true,
                        owner_session_id: None,
                    },
                    PlanStep {
                        title: "Validate worker limits".into(),
                        done: true,
                        owner_session_id: Some("session-worker".into()),
                    },
                    PlanStep {
                        title: "Migrate stored agent_configs".into(),
                        done: false,
                        owner_session_id: Some("session-migration".into()),
                    },
                ],
            }),
            "",
        ),
        call(
            "t-spawn",
            "session-plan",
            "mcp__relay__spawn_agent",
            ToolKind::Spawn,
            Completed,
            "Schema migration",
            Some(ToolDetail::Spawn {
                child_session_id: Some("session-migration".into()),
            }),
            "",
        ),
        call(
            "t-message",
            "session-plan",
            "mcp__relay__message_agent",
            ToolKind::Message,
            Completed,
            "Profile validation",
            Some(ToolDetail::Message {
                target_session_id: Some("session-worker".into()),
                read: true,
            }),
            "",
        ),
        call(
            "t-result",
            "session-plan",
            "mcp__relay__report_result",
            ToolKind::Result,
            Completed,
            "Profile validation",
            Some(ToolDetail::Result {
                from_session_id: Some("session-worker".into()),
                summary: Some(
                    "Inherited fields follow project defaults; explicit overrides stay unchanged."
                        .into(),
                ),
            }),
            "",
        ),
        call(
            "t-other",
            "session-plan",
            "mcp__github__get_issue",
            ToolKind::Other,
            Completed,
            "issue #2",
            None,
            "Make director profiles declarative",
        ),
        call(
            "t-wait",
            "session-plan",
            "mcp__relay__wait_for_agents",
            ToolKind::Wait,
            Running,
            "2 agents",
            Some(ToolDetail::Wait {
                session_ids: vec!["session-migration".into(), "session-tests".into()],
            }),
            "",
        ),
        call(
            "t-migrate",
            "session-write",
            "Bash",
            ToolKind::Run,
            Running,
            "psql -f migrations/0012_profile_overrides.sql",
            None,
            "",
        ),
    ]
}

/// Rate-limit windows and a week of ledger entries for the demo workspace,
/// as observed at `now` (Unix seconds). Claude's session window runs out
/// before it resets. Only the isolated demo uses these: a live workspace
/// must show only what its harnesses report.
pub fn demo_usage(snapshot: &mut Snapshot, now: u64) {
    let hour = 3600;
    for limit in [
        UsageLimit::new(
            Harness::Codex,
            UsageWindow::FiveHour,
            38.0,
            300,
            Some(now + 2 * hour),
            now,
        ),
        UsageLimit::new(
            Harness::Codex,
            UsageWindow::Weekly,
            41.0,
            10_080,
            Some(now + 80 * hour),
            now,
        ),
        UsageLimit::new(
            Harness::ClaudeCode,
            UsageWindow::FiveHour,
            71.0,
            300,
            Some(now + 3 * hour),
            now,
        ),
        UsageLimit::new(
            Harness::ClaudeCode,
            UsageWindow::Weekly,
            48.0,
            10_080,
            Some(now + 96 * hour),
            now,
        ),
    ] {
        snapshot.record_usage_limit(limit);
    }
    let entries = [
        (
            "session-plan",
            Harness::ClaudeCode,
            "claude-opus-4-5",
            412_000,
            180_000,
            9_000,
        ),
        (
            "session-migration",
            Harness::ClaudeCode,
            "claude-sonnet-4-5",
            238_000,
            120_000,
            6_000,
        ),
        (
            "session-write",
            Harness::ClaudeCode,
            "claude-sonnet-4-5",
            96_000,
            40_000,
            3_000,
        ),
        (
            "session-index",
            Harness::ClaudeCode,
            "claude-haiku-4-5",
            18_000,
            4_000,
            800,
        ),
        (
            "session-worker",
            Harness::Codex,
            "gpt-5.1-codex",
            150_000,
            60_000,
            5_000,
        ),
        (
            "session-backfill",
            Harness::Codex,
            "gpt-5.1-codex",
            88_000,
            30_000,
            2_500,
        ),
        (
            "session-tests",
            Harness::Codex,
            "gpt-5.1-codex",
            61_000,
            20_000,
            1_800,
        ),
    ];
    for (index, (session, harness, model, input, cached, output)) in entries.into_iter().enumerate()
    {
        snapshot.record_usage(
            UsageEntry {
                id: format!("demo-usage-{index}"),
                session_id: session.into(),
                harness,
                model: Some(model.into()),
                input_tokens: input,
                cached_input_tokens: cached,
                output_tokens: output,
                completed_at: now - (index as u64 + 1) * 5 * hour,
            },
            now,
        );
    }
}

/// The model catalog the demo's example sessions describe, so their effort
/// tracks render. Fixture sessions never run a harness, so the server
/// answers discovery for them with this instead.
pub fn demo_catalog(session_id: &str, harness: Harness) -> HarnessCatalog {
    let model = |id: &str, name: &str, efforts: &[&str], default: &str| HarnessModel {
        id: id.into(),
        name: name.into(),
        efforts: efforts.iter().map(|e| e.to_string()).collect(),
        default_effort: Some(default.into()),
        fast: false,
        ..Default::default()
    };
    let mut models = match harness {
        Harness::ClaudeCode => vec![
            model(
                "claude-opus-4-5",
                "Opus 4.5",
                &["low", "medium", "high", "xhigh", "max"],
                "high",
            ),
            model(
                "claude-sonnet-4-5",
                "Sonnet 4.5",
                &["low", "medium", "high"],
                "medium",
            ),
            model(
                "claude-haiku-4-5",
                "Haiku 4.5",
                &["low", "medium", "high"],
                "low",
            ),
        ],
        Harness::Codex => vec![
            model(
                "gpt-5.1-codex",
                "GPT-5.1 Codex",
                &["minimal", "low", "medium", "high"],
                "medium",
            ),
            model(
                "gpt-5.1-codex-mini",
                "GPT-5.1 Codex mini",
                &["low", "medium", "high"],
                "medium",
            ),
        ],
    };
    // The first listed model is the harness default.
    models[0].default = true;
    HarnessCatalog {
        session_id: session_id.into(),
        harness,
        commands: vec![],
        skills: vec![],
        models,
        warnings: vec![],
        mcp: vec![],
    }
}
