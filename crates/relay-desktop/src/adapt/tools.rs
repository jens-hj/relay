//! Tool calls, fingerprints and approval blocks.

use super::identity::{harness, session_model};
use super::tree::{ancestor_path, session_status};
use relay_core::{
    HarnessCatalog, Message, Snapshot, ToolCall, ToolDetail as Measured, ToolKind as Kind,
    ToolPermission, ToolState,
};
use relay_ui::prelude::{
    AgentStatus, ApprovalView, CodeLine, Hunk, Opening, PlanStep, SearchFile, TestFailure,
    ToolCallView, ToolCount, ToolDetail, WaitAgent,
};
use relay_ui::tokens::ToolKind;
use std::collections::BTreeMap;

/// Lines of tool output kept in a summary's tail.
const TAIL: usize = 6;

pub fn ui_kind(kind: Kind) -> ToolKind {
    match kind {
        Kind::Read => ToolKind::Read,
        Kind::Edit => ToolKind::Edit,
        Kind::Create => ToolKind::Create,
        Kind::Search => ToolKind::Search,
        Kind::Run => ToolKind::Run,
        Kind::Test => ToolKind::Test,
        Kind::Fetch => ToolKind::Fetch,
        Kind::Plan => ToolKind::Plan,
        Kind::Spawn => ToolKind::Spawn,
        Kind::Message => ToolKind::Message,
        Kind::Wait => ToolKind::Wait,
        Kind::Result => ToolKind::Result,
        Kind::Other => ToolKind::Other,
    }
}

fn pending(permission: &ToolPermission) -> bool {
    permission.decision.is_none() && !permission.expired
}

/// Whether a pending permission request belongs to this running call:
/// Codex names the item, Claude repeats the tool and its input.
fn awaits(permission: &ToolPermission, message: &Message, tool: &ToolCall) -> bool {
    if permission.session_id != message.session_id
        || permission.run_id != tool.run_id
        || !pending(permission)
    {
        return false;
    }
    let input: serde_json::Value =
        serde_json::from_str(&permission.description).unwrap_or_default();
    input["itemId"]
        .as_str()
        .is_some_and(|id| message.id == format!("codex-{}-{id}", tool.run_id))
        || (permission.tool == tool.name && permission.description == tool.input)
}

/// A call's state. A recorded call whose outcome was never reported shows
/// as queued (hollow) rather than borrowing success.
pub fn tool_status(snapshot: &Snapshot, message: &Message) -> AgentStatus {
    let Some(tool) = &message.tool else {
        return AgentStatus::Queued;
    };
    if tool.state == ToolState::Running
        && snapshot
            .tool_permissions
            .iter()
            .any(|p| awaits(p, message, tool))
    {
        return AgentStatus::Waiting;
    }
    match tool.state {
        ToolState::Running => AgentStatus::Working,
        ToolState::Completed => AgentStatus::Done,
        ToolState::Failed | ToolState::Interrupted => AgentStatus::Failed,
        ToolState::Unknown => AgentStatus::Queued,
    }
}

fn lines(text: &str) -> Option<Vec<CodeLine>> {
    let lines: Vec<CodeLine> = text
        .lines()
        .map(|line| CodeLine {
            text: line.to_string(),
            ..Default::default()
        })
        .collect();
    (!lines.is_empty()).then_some(lines)
}

fn tail(text: &str) -> Option<Vec<CodeLine>> {
    lines(text).map(|lines| lines[lines.len().saturating_sub(TAIL)..].to_vec())
}

fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, ms / 1000 % 60)
    }
}

fn title(snapshot: &Snapshot, id: &Option<String>) -> Option<String> {
    let id = id.as_ref()?;
    Some(
        snapshot
            .sessions
            .iter()
            .find(|s| &s.id == id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| id.clone()),
    )
}

fn clamp(value: u64) -> u32 {
    value.min(u64::from(u32::MAX)) as u32
}

/// The presentation detail of a call: the server's measured detail where
/// present, the recorded output otherwise. Nothing is estimated.
fn detail(
    snapshot: &Snapshot,
    message: &Message,
    tool: &ToolCall,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> Option<ToolDetail> {
    let output = message.body.as_str();
    let target = (!tool.target.is_empty()).then(|| tool.target.clone());
    let elapsed = tool
        .started_ms
        .zip(tool.finished_ms)
        .map(|(start, end)| end.saturating_sub(start));
    Some(match (&tool.detail, tool.kind) {
        (
            Some(Measured::Read {
                start_line,
                end_line,
                total_lines,
            }),
            _,
        ) => ToolDetail::Read {
            start: start_line.map(clamp),
            end: end_line.map(clamp),
            total: total_lines.map(clamp),
            found: None,
            lines: lines(output),
        },
        (None, Kind::Read) => ToolDetail::Read {
            start: None,
            end: None,
            total: None,
            found: None,
            lines: lines(output),
        },
        (
            Some(Measured::Edit {
                hunks,
                added,
                removed,
            }),
            _,
        ) => ToolDetail::Edit {
            added: Some(clamp(*added)),
            removed: Some(clamp(*removed)),
            hunks: Some(
                hunks
                    .iter()
                    .map(|h| Hunk {
                        line: Some(clamp(h.line)),
                        label: h.label.clone(),
                        added: Some(clamp(h.added)),
                        removed: Some(clamp(h.removed)),
                        lines: None,
                    })
                    .collect(),
            ),
        },
        (Some(Measured::Create { lines: count }), _) => ToolDetail::Create {
            lines: Some(clamp(*count)),
            first_line: None,
            content: lines(output),
        },
        (Some(Measured::Search { query, files }), _) => ToolDetail::Search {
            query: (!query.is_empty()).then(|| query.clone()),
            scope: None,
            files: Some(
                files
                    .iter()
                    .map(|f| SearchFile {
                        path: Some(f.path.clone()),
                        hits: Some(clamp(f.hits)),
                        matches: None,
                    })
                    .collect(),
            ),
        },
        (
            Some(Measured::Run {
                exit_code,
                duration_ms,
            }),
            _,
        ) => ToolDetail::Run {
            command: target,
            exit_code: exit_code.or(tool.exit_code).map(|c| c as i32),
            duration: duration_ms.or(elapsed).map(duration),
            tail: tail(output),
            full: lines(output),
        },
        (None, Kind::Run) => ToolDetail::Run {
            command: target,
            exit_code: tool.exit_code.map(|c| c as i32),
            duration: elapsed.map(duration),
            tail: tail(output),
            full: lines(output),
        },
        (
            Some(Measured::Test {
                passed,
                failed,
                failures,
            }),
            _,
        ) => ToolDetail::Test {
            target,
            passed: passed.map(clamp),
            failed: failed.map(clamp),
            failures: (!failures.is_empty()).then(|| {
                failures
                    .iter()
                    .map(|name| TestFailure {
                        name: name.clone(),
                        trace: None,
                    })
                    .collect()
            }),
        },
        (Some(Measured::Fetch { status, title }), _) => ToolDetail::Fetch {
            url: target,
            status: *status,
            title: title.clone(),
            read: None,
            used: None,
            kept: None,
        },
        (Some(Measured::Plan { steps }), _) => ToolDetail::Plan {
            title: None,
            steps: Some(
                steps
                    .iter()
                    .map(|step| PlanStep {
                        title: step.title.clone(),
                        done: Some(step.done),
                        owner: title(snapshot, &step.owner_session_id),
                    })
                    .collect(),
            ),
        },
        (Some(Measured::Spawn { child_session_id }), _) => {
            let child = child_session_id
                .as_ref()
                .and_then(|id| snapshot.sessions.iter().find(|s| &s.id == id));
            let (model, effort) = child
                .map(|child| session_model(child, catalogs))
                .unwrap_or_default();
            let grant = child.and_then(|child| child.spawn.as_ref());
            ToolDetail::Spawn {
                child: title(snapshot, child_session_id),
                harness: child
                    .and_then(|c| c.worker.as_ref())
                    .map(|w| harness(w.harness)),
                model,
                effort,
                task: grant.map(|g| g.task.clone()),
                context: grant.and_then(|g| g.context.clone()),
                reports_to: title(snapshot, &Some(message.session_id.clone())),
                permissions: grant.filter(|g| !g.permissions.is_empty()).map(|g| {
                    g.permissions
                        .iter()
                        .map(|(task, permission)| relay_ui::prelude::ToolPermission {
                            capability: task.label().to_lowercase(),
                            mode: match permission {
                                relay_core::Permission::Allow => {
                                    relay_ui::prelude::PermissionMode::Allow
                                }
                                relay_core::Permission::Ask => {
                                    relay_ui::prelude::PermissionMode::Ask
                                }
                                relay_core::Permission::Deny => {
                                    relay_ui::prelude::PermissionMode::Deny
                                }
                            },
                        })
                        .collect()
                }),
            }
        }
        (
            Some(Measured::Message {
                target_session_id,
                read,
            }),
            _,
        ) => ToolDetail::Message {
            target: title(snapshot, target_session_id),
            read: Some(*read),
            exchange: None,
        },
        (Some(Measured::Wait { session_ids }), _) => ToolDetail::Wait {
            agents: Some(
                session_ids
                    .iter()
                    .map(|id| {
                        let session = snapshot.sessions.iter().find(|s| &s.id == id);
                        WaitAgent {
                            name: session
                                .map(|s| s.title.clone())
                                .unwrap_or_else(|| id.clone()),
                            harness: session
                                .and_then(|s| s.worker.as_ref())
                                .map(|w| harness(w.harness)),
                            status: session.map(|s| session_status(snapshot, s)),
                            lane: None,
                        }
                    })
                    .collect(),
            ),
        },
        (
            Some(Measured::Result {
                from_session_id,
                summary,
            }),
            _,
        ) => ToolDetail::Result {
            quantities: None,
            from: title(snapshot, from_session_id),
            summary: summary.clone(),
            full: (!output.is_empty()).then(|| output.to_string()),
            attachments: None,
            used_by: None,
        },
        _ if !output.trim().is_empty() => ToolDetail::Result {
            quantities: None,
            from: None,
            summary: None,
            full: Some(output.to_string()),
            attachments: None,
            used_by: None,
        },
        _ => return None,
    })
}

/// A transcript tool message as a tool call row.
pub fn tool_view(
    snapshot: &Snapshot,
    message: &Message,
    opening: Opening,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> Option<ToolCallView> {
    let tool = message.tool.as_ref()?;
    let kind = ui_kind(tool.kind);
    Some(ToolCallView {
        kind,
        status: tool_status(snapshot, message),
        verb: if tool.kind == Kind::Other {
            tool.name.clone()
        } else {
            kind.name().to_string()
        },
        target: (!tool.target.is_empty()).then(|| tool.target.clone()),
        opening,
        detail: detail(snapshot, message, tool, catalogs),
    })
}

/// Calls per tool type in a session's loaded transcript, in the order the
/// types first appear. Older transcripts' tool messages count as the typed
/// calls the transcript shows them as.
pub fn fingerprint(messages: &[Message], session_id: &str) -> Vec<ToolCount> {
    let mut counts: Vec<ToolCount> = Vec::new();
    for tool in messages
        .iter()
        .filter(|m| m.session_id == session_id)
        .filter_map(|m| crate::tool_activity::with_legacy_tool(m.clone()).tool)
    {
        let kind = ui_kind(tool.kind);
        match counts.iter_mut().find(|c| c.kind == kind) {
            Some(count) => count.count += 1,
            None => counts.push(ToolCount { kind, count: 1 }),
        }
    }
    counts
}

/// Pending permission requests a session's transcript shows: its own and
/// every descendant's, so managers see what blocks their agents.
pub fn transcript_permissions(snapshot: &Snapshot, session_id: &str) -> Vec<ToolPermission> {
    snapshot
        .subtree_pending_permissions(session_id)
        .into_iter()
        .cloned()
        .collect()
}

fn action(tool: &str) -> &str {
    match tool {
        "item/commandExecution/requestApproval" => "Run command",
        "item/fileChange/requestApproval" => "Change files",
        other => other,
    }
}

/// An approval block for a permission request, seen from `viewer`'s
/// transcript. A request from below names its agent and path.
pub fn approval_view(
    snapshot: &Snapshot,
    permission: &ToolPermission,
    viewer: &str,
) -> ApprovalView {
    let requester = snapshot
        .sessions
        .iter()
        .find(|s| s.id == permission.session_id);
    let description = serde_json::from_str::<serde_json::Value>(&permission.description)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| permission.description.clone());
    let below = permission.session_id != viewer;
    ApprovalView {
        requester: Some(
            requester
                .map(|s| s.title.clone())
                .unwrap_or_else(|| "Agent".into()),
        ),
        path: below
            .then(|| ancestor_path(snapshot, &permission.session_id).join(" / "))
            .filter(|path| !path.is_empty()),
        heading: format!("Approval Requested · {}", action(&permission.tool)),
        command: (!description.trim().is_empty()).then_some(description),
        why: None,
        touches: None,
        blocks: below.then(|| {
            requester
                .map(|s| s.title.clone())
                .unwrap_or_else(|| permission.session_id.clone())
        }),
    }
}
