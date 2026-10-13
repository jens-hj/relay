use super::*;
use relay_core::*;
use relay_ui::prelude::{AgentNode, AgentStatus, ModelHue, Opening, ToolDetail as View};
use relay_ui::tokens::ToolKind as UiKind;
use relay_ui::tree::{Flatten, TreeLine, approvals, flatten};
use std::collections::{BTreeMap, BTreeSet};

fn demo() -> Snapshot {
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    demo_orchestration(&mut snapshot);
    snapshot
}

fn forest(snapshot: &Snapshot) -> Vec<AgentNode> {
    agent_forest(
        snapshot,
        "demo",
        "session-plan",
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
}

fn names(nodes: &[AgentNode]) -> Vec<&str> {
    nodes.iter().map(|n| n.name.as_str()).collect()
}

#[test]
fn forest_follows_parent_links_and_keeps_legacy_sessions_as_roots() {
    let mut snapshot = demo();
    let roots = forest(&snapshot);
    assert_eq!(names(&roots), ["Profile design", "Contextual review"]);
    assert!(roots[0].selected);
    assert_eq!(
        names(&roots[0].children),
        ["Profile validation", "Schema migration"]
    );
    let migration = &roots[0].children[1];
    assert_eq!(
        names(&migration.children),
        ["Write migrations", "Migration tests"]
    );
    // A missing parent never invents provenance; a cycle still shows once.
    snapshot.sessions[8].parent_session_id = Some("gone".into());
    snapshot.sessions[0].parent_session_id = Some("session-worker".into());
    let roots = forest(&snapshot);
    let mut seen = Vec::new();
    fn walk<'a>(node: &'a AgentNode, seen: &mut Vec<&'a str>) {
        seen.push(&node.id);
        node.children.iter().for_each(|c| walk(c, seen));
    }
    roots.iter().for_each(|r| walk(r, &mut seen));
    seen.sort();
    assert_eq!(seen.len(), snapshot.sessions.len());
    seen.dedup();
    assert_eq!(seen.len(), snapshot.sessions.len());
    assert!(roots.iter().any(|r| r.id == "session-review"));
}

#[test]
fn statuses_map_runs_permissions_and_queued_turns() {
    let mut snapshot = demo();
    let status = |snapshot: &Snapshot, id: &str| {
        session_status(
            snapshot,
            snapshot.sessions.iter().find(|s| s.id == id).unwrap(),
        )
    };
    assert_eq!(status(&snapshot, "session-plan"), AgentStatus::Working);
    assert_eq!(status(&snapshot, "session-worker"), AgentStatus::Done);
    assert_eq!(status(&snapshot, "session-write"), AgentStatus::Waiting);
    assert_eq!(status(&snapshot, "session-verify"), AgentStatus::Queued);
    assert_eq!(status(&snapshot, "session-tests"), AgentStatus::Failed);
    assert_eq!(status(&snapshot, "session-review"), AgentStatus::Done);
    snapshot.submissions.push(Submission {
        id: "turn".into(),
        session_id: "session-review".into(),
        parts: vec![],
        state: SubmissionState::Queued,
        approve_implementation: false,
        error: None,
        interrupts_run: None,
        last_edit_request: None,
    });
    assert_eq!(status(&snapshot, "session-review"), AgentStatus::Queued);
    snapshot.tool_permissions[0].decision = Some(true);
    assert_eq!(status(&snapshot, "session-write"), AgentStatus::Working);
}

#[test]
fn deep_pending_permission_surfaces_on_collapsed_ancestors_and_in_needs_you() {
    let snapshot = demo();
    let collapsed = BTreeSet::from(["session-plan".to_string()]);
    let roots = agent_forest(&snapshot, "demo", "", &collapsed, &BTreeMap::new());
    assert!(!roots[0].expanded);
    assert_eq!(roots[0].waiting_below(), 1);
    let lines = flatten(&roots, Flatten::default());
    let TreeLine::Agent(root) = &lines[0] else {
        panic!("root line")
    };
    assert!(root.shows_summary());
    assert_eq!(root.below.waiting, 1);
    let waiting = approvals(&roots);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id, "session-write");
    assert_eq!(waiting[0].path, ["Profile design", "Schema migration"]);
    let pending = needs_you(&snapshot, "demo");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].permission.id, "permission-migrate");
    assert_eq!(pending[0].path, ["Profile design", "Schema migration"]);
    // Every ancestor's transcript shows the request; siblings do not.
    for viewer in ["session-plan", "session-migration", "session-write"] {
        assert_eq!(
            transcript_permissions(&snapshot, viewer).len(),
            1,
            "{viewer}"
        );
    }
    assert!(transcript_permissions(&snapshot, "session-tests").is_empty());
    let block = approval_view(&snapshot, &pending[0].permission, "session-plan");
    assert_eq!(block.requester.as_deref(), Some("Write migrations"));
    assert_eq!(
        block.path.as_deref(),
        Some("Profile design / Schema migration")
    );
    let own = approval_view(&snapshot, &pending[0].permission, "session-write");
    assert!(own.path.is_none() && own.blocks.is_none());
    assert!(needs_you(&snapshot, "other").is_empty());
}

#[test]
fn deep_branches_fold_past_the_fold_depth() {
    let roots = forest(&demo());
    let lines = flatten(&roots, Flatten::default());
    let folded: Vec<_> = lines
        .iter()
        .filter_map(|line| match line {
            TreeLine::Folded(folded) => Some(folded),
            _ => None,
        })
        .collect();
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0].parent, "session-backfill");
    assert_eq!(folded[0].text(), "2 more levels · 2 agents");
    assert!(
        !lines
            .iter()
            .any(|l| matches!(l, TreeLine::Agent(a) if a.id == "session-index"))
    );
}

#[test]
fn model_identity_splits_family_version_and_vendor() {
    let cases = [
        (
            "claude-opus-4-5-20251101",
            "Opus",
            "4.5",
            "CLAUDE",
            ModelHue::Opus,
        ),
        (
            "claude-sonnet-4-5",
            "Sonnet",
            "4.5",
            "CLAUDE",
            ModelHue::Sonnet,
        ),
        (
            "claude-3-5-haiku-latest",
            "Haiku",
            "3.5",
            "CLAUDE",
            ModelHue::Haiku,
        ),
        ("sonnet[1m]", "Sonnet", "", "CLAUDE", ModelHue::Sonnet),
        (
            "gpt-5.1-codex-max",
            "GPT",
            "5.1 codex max",
            "GPT",
            ModelHue::Sol,
        ),
        ("gpt5", "GPT", "5", "GPT", ModelHue::Sol),
        ("codex-mini-latest", "Codex", "mini", "GPT", ModelHue::Sol),
        ("sol-6.1", "Sol", "6.1", "GPT", ModelHue::Sol),
        ("llama-3.1-70b", "llama-3.1-70b", "", "", ModelHue::Neutral),
    ];
    for (id, family, version, vendor, hue) in cases {
        let identity = model_identity(id);
        assert_eq!(
            (
                identity.family.as_str(),
                identity.version.as_str(),
                identity.vendor.as_str(),
                identity.hue
            ),
            (family, version, vendor, hue),
            "{id}"
        );
    }
    assert_eq!(model_identity("claude-opus-5-5").short_name(), "Opus 5.5");
}

fn catalog_model(efforts: &[&str]) -> HarnessModel {
    HarnessModel {
        id: "gpt-5.1-codex".into(),
        name: "GPT-5.1 Codex".into(),
        efforts: efforts.iter().map(|e| e.to_string()).collect(),
        default_effort: Some("medium".into()),
        fast: false,
    }
}

#[test]
fn effort_tracks_offered_steps_and_max_only_when_offered() {
    let mut selection = HarnessSelection::default();
    let model = catalog_model(&["low", "medium", "high"]);
    let default = effort(&selection, &model).unwrap();
    assert_eq!(
        (default.levels, default.current, default.has_max),
        (3, 1, false)
    );
    assert_eq!(default.name(), "medium");
    assert_eq!(default.step_names(), ["low", "medium", "high"]);
    selection.effort = Some("high".into());
    assert_eq!(effort(&selection, &model).unwrap().name(), "high");
    let with_max = catalog_model(&["minimal", "low", "medium", "high", "xhigh", "max"]);
    selection.effort = Some("max".into());
    let max = effort(&selection, &with_max).unwrap();
    assert!(max.has_max && max.at_max);
    assert_eq!(max.levels, 5);
    assert_eq!(max.name(), "max");
    assert_eq!(effort_choice(&with_max, 5).as_deref(), Some("max"));
    assert_eq!(effort_choice(&with_max, 0).as_deref(), Some("minimal"));
    assert_eq!(effort_choice(&model, 3), None);
    // Names the track cannot spell, or no effort at all, hide the track.
    assert!(effort(&selection, &catalog_model(&["fast", "slow"])).is_none());
    assert!(effort(&HarnessSelection::default(), &catalog_model(&[])).is_none());
}

#[test]
fn tool_calls_map_measured_details_and_pending_permissions() {
    let mut snapshot = demo();
    let catalogs = BTreeMap::new();
    let message = |snapshot: &Snapshot, id: &str| {
        snapshot
            .messages
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .clone()
    };
    let read = tool_view(
        &snapshot,
        &message(&snapshot, "t-read"),
        Opening::Summary,
        &catalogs,
    )
    .unwrap();
    assert_eq!(read.kind, UiKind::Read);
    assert_eq!(read.verb, "Read");
    assert_eq!(read.status, AgentStatus::Done);
    assert!(matches!(
        read.detail,
        Some(View::Read {
            start: Some(1),
            end: Some(40),
            total: Some(243),
            ..
        })
    ));
    let spawn = tool_view(
        &snapshot,
        &message(&snapshot, "t-spawn"),
        Opening::Expanded,
        &catalogs,
    )
    .unwrap();
    let Some(View::Spawn {
        child,
        task,
        reports_to,
        ..
    }) = spawn.detail
    else {
        panic!("spawn detail");
    };
    assert_eq!(child.as_deref(), Some("Schema migration"));
    assert_eq!(reports_to.as_deref(), Some("Profile design"));
    assert!(task.is_some());
    let other = tool_view(
        &snapshot,
        &message(&snapshot, "t-other"),
        Opening::Collapsed,
        &catalogs,
    )
    .unwrap();
    assert_eq!(other.verb, "mcp__github__get_issue");
    // The running command the permission names waits for you.
    let pending = message(&snapshot, "t-migrate");
    snapshot.tool_permissions[0].tool = "Bash".into();
    if let Some(tool) = snapshot
        .messages
        .iter_mut()
        .find(|m| m.id == "t-migrate")
        .and_then(|m| m.tool.as_mut())
    {
        tool.input = snapshot.tool_permissions[0].description.clone();
    }
    assert_eq!(tool_status(&snapshot, &pending), AgentStatus::Working);
    let pending = message(&snapshot, "t-migrate");
    assert_eq!(tool_status(&snapshot, &pending), AgentStatus::Waiting);
    // Unreported outcomes do not borrow success.
    let mut unknown = pending.clone();
    unknown.tool.as_mut().unwrap().state = ToolState::Unknown;
    assert_eq!(tool_status(&snapshot, &unknown), AgentStatus::Queued);
}

#[test]
fn fingerprint_counts_every_tool_kind_once_in_first_seen_order() {
    let snapshot = demo();
    let counts = fingerprint(&snapshot.messages, "session-plan");
    assert_eq!(counts.len(), 13);
    assert_eq!(counts[0].kind, UiKind::Read);
    assert!(counts.iter().all(|c| c.count == 1));
    assert_eq!(fingerprint(&snapshot.messages, "session-write").len(), 1);
    assert!(fingerprint(&snapshot.messages, "session-review").is_empty());
}

#[test]
fn usage_hides_missing_and_reset_windows_and_forecasts_the_rest() {
    let mut snapshot = demo();
    assert!(harness_usage(&snapshot, 1_000_000).is_empty());
    let now = 2_000_000;
    demo_usage(&mut snapshot, now);
    let usage = harness_usage(&snapshot, now);
    assert_eq!(usage.len(), 2);
    let claude = &usage[1];
    let session = claude.session.as_ref().unwrap();
    assert_eq!(session.resets, "resets in 3h 00m");
    // 71% in two of five hours runs out before the reset.
    assert!(session.forecast().unwrap().projected > 100.0);
    assert_eq!(claude.insight.model_mix[0].hue, ModelHue::Opus);
    assert_eq!(claude.insight.heaviest[0].0, "Profile design");
    assert!(claude.insight.suggestion.is_none());
    // An observation from a window that has since reset is dropped.
    let later = harness_usage(&snapshot, now + 4 * 3600);
    assert!(later[1].session.is_none());
    assert!(later[1].week.is_some());
}

#[test]
fn context_needs_both_measurements_and_reports_compactions() {
    let snapshot = demo();
    let plan = &snapshot.sessions[0];
    let view = context_view(plan).unwrap();
    assert_eq!(
        (view.used, view.window, view.compactions),
        (124_000, 200_000, 2)
    );
    assert_eq!(view.history, "Compacted twice, last at 176k");
    assert_eq!(view.model, "Opus 4.5");
    assert!(view.parts.is_empty());
    let worker = snapshot
        .sessions
        .iter()
        .find(|s| s.id == "session-worker")
        .unwrap();
    assert!(context_view(worker).is_none());
}

#[test]
fn session_model_reads_reported_model_and_catalog_effort() {
    let snapshot = demo();
    let plan = snapshot.sessions[0].clone();
    let (model, effort) = session_model(&plan, &BTreeMap::new());
    assert_eq!(model.unwrap().hue, ModelHue::Opus);
    assert!(effort.is_none());
    let catalogs = BTreeMap::from([(
        plan.id.clone(),
        Ok(HarnessCatalog {
            models: vec![HarnessModel {
                id: "claude-opus-4-5".into(),
                name: "Opus".into(),
                efforts: vec!["low".into(), "medium".into(), "high".into(), "max".into()],
                default_effort: None,
                fast: false,
            }],
            ..Default::default()
        }),
    )]);
    let (_, effort) = session_model(&plan, &catalogs);
    let effort = effort.unwrap();
    assert_eq!(
        (effort.levels, effort.current, effort.has_max),
        (3, 2, true)
    );
    assert_eq!(subtree_statuses(&snapshot, "session-backfill").len(), 2);
    assert_eq!(
        ancestor_path(&snapshot, "session-index"),
        [
            "Profile design",
            "Schema migration",
            "Write migrations",
            "Backfill script",
            "Verify backfill"
        ]
    );
    assert_eq!(
        harness(relay_core::Harness::Codex),
        relay_ui::prelude::Harness::Codex
    );
    assert_eq!(ui_kind(ToolKind::Other), UiKind::Other);
}
