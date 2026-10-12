//! Rust host for Claude Code's streaming CLI/control protocol.
use super::*;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};

pub(super) fn arguments(
    cmd: &mut tokio::process::Command,
    mode: ApprovalMode,
    thread: Option<&str>,
    path: &str,
    inputs: &str,
    roots: &[String],
    selection: &HarnessSelection,
) {
    cmd.args([
        "--print",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
    ]);
    cmd.arg(if selection.plan {
        "plan"
    } else {
        match mode {
            ApprovalMode::Automatic => "auto",
            ApprovalMode::Ask => "default",
            ApprovalMode::Unrestricted => "bypassPermissions",
        }
    });
    if let Some(thread) = thread {
        cmd.args(["--resume", thread]);
    }
    cmd.args(["--add-dir", inputs]);
    for root in roots {
        if root != path {
            cmd.args(["--add-dir", root]);
        }
    }
    if mode != ApprovalMode::Unrestricted {
        let mut settings = json!({"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":true,"filesystem":{"allowWrite":roots},"network":{"allowAllUnixSockets":false}}});
        if selection.model.is_some() {
            settings["fastMode"] = json!(selection.fast);
        }
        cmd.arg("--settings").arg(settings.to_string());
    } else if selection.model.is_some() {
        cmd.arg("--settings")
            .arg(json!({"fastMode":selection.fast}).to_string());
    }
    // Explicit turns drive resume; an inherited setting must not replay an interrupted turn.
    cmd.env_remove("CLAUDE_CODE_RESUME_INTERRUPTED_TURN")
        .env("CLAUDE_CODE_SDK_READS_SESSION_STATE", "1");
}
async fn send(stdin: &mut tokio::process::ChildStdin, value: Value) -> Result<(), Error> {
    let mut bytes = serde_json::to_vec(&value).map_err(Error::internal)?;
    bytes.push(b'\n');
    tokio::time::timeout(std::time::Duration::from_secs(15), stdin.write_all(&bytes))
        .await
        .map_err(|_| Error::invalid("Claude input timed out"))?
        .map_err(|_| Error::invalid("Cannot send input to Claude Code"))
}
fn message(
    snapshot: &mut Snapshot,
    session: &str,
    run: &str,
    id: &str,
    kind: &str,
    body: &str,
    append: bool,
) {
    let id = format!("claude-{run}-{id}");
    let value = if let Some(value) = snapshot.messages.iter_mut().find(|m| m.id == id) {
        value
    } else {
        snapshot.messages.push(Message {
            id: id.clone(),
            session_id: session.into(),
            author: "Claude Code".into(),
            kind: kind.into(),
            body: String::new(),
            parts: vec![],
            tool: None,
        });
        snapshot.messages.last_mut().unwrap()
    };
    if !append {
        // Replies may already anchor a streamed prefix. Final events cannot
        // rewrite recorded text that was visible to another client.
        if !body.starts_with(&value.body) {
            return;
        }
        value.body.clear();
    }
    let remaining = (64 * 1024usize).saturating_sub(value.body.len());
    let mut end = body.len().min(remaining);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    value.body.push_str(&body[..end]);
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    workspace: &Workspace,
    session: &str,
    run: &str,
    input: Vec<Value>,
    fork: Option<&Session>,
    expected_skills: &[String],
    previous: Option<&str>,
    child: &mut tokio::process::Child,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), Error> {
    let mut content = vec![];
    for part in input {
        if part["type"] == "localImage" {
            let path = part["path"]
                .as_str()
                .ok_or_else(|| Error::invalid("Image path missing"))?;
            let bytes = tokio::fs::read(path).await.map_err(Error::internal)?;
            use base64::Engine;
            let media = image::guess_format(&bytes)
                .map_err(|_| Error::invalid("Image format unavailable"))?;
            let media = match media {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                image::ImageFormat::WebP => "image/webp",
                _ => return Err(Error::invalid("Claude image format unavailable")),
            };
            content.push(json!({"type":"image","source":{"type":"base64","media_type":media,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}}));
        } else {
            content.push(json!({"type":"text","text":part["text"]}));
        }
    }
    let mut stdin = child.stdin.take();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    send(stdin.as_mut().ok_or_else(|| Error::invalid("Claude input already closed"))?, json!({"type":"control_request","request_id":"relay-initialize","request":{"subtype":"initialize","hooks":null}})).await?;
    let mut invoked_skills = std::collections::HashSet::new();
    let mut sent = false;
    let mut thread = previous.map(str::to_owned);
    let mut current_messages = std::collections::HashMap::new();
    let mut completed = false;
    let mut state: Option<String> = None;
    let mut tasks = std::collections::HashSet::new();
    let mut compactions = 0usize;
    // Full tool inputs by tool_use_id; persisted inputs are truncated.
    let mut tool_inputs = std::collections::HashMap::<String, (String, Value)>::new();
    let mut deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(30));
    loop {
        let bytes = tokio::select! { biased; _ = stop.changed() => {
            let _ = send(stdin.as_mut().ok_or_else(|| Error::invalid("Claude input already closed"))?, json!({"type":"control_request","request_id":"relay-interrupt","request":{"subtype":"interrupt"}})).await;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
                while let Some(bytes) = runtime::line(&mut stdout).await? { let value: Value = serde_json::from_slice(&bytes).map_err(Error::internal)?; if value["type"] == "result" { break; } }
                Ok::<_,Error>(())
            }).await;
            return Ok(());
        }, line = async {
            if let Some(deadline) = deadline {
                tokio::time::timeout_at(deadline, runtime::line(&mut stdout)).await.map_err(|_| Error::invalid(if completed {"Claude Code did not finish after its result"} else {"Claude Code initialization timed out"}))?
            } else {runtime::line(&mut stdout).await}
        } => line? };
        let Some(bytes) = bytes else {
            if completed && tasks.is_empty() && state.as_deref().is_none_or(|s| s == "idle") {
                break;
            }
            return Err(Error::invalid(
                "Claude Code disconnected before completing the turn",
            ));
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Invalid Claude Code streaming event"))?;
        if let Some(id) = value["session_id"].as_str()
            && thread.as_deref().is_some_and(|existing| existing != id)
        {
            return Err(Error::invalid("Claude Code resumed a different session"));
        }
        match value["type"].as_str().unwrap_or("") {
            "control_response" if value["response"]["request_id"] == "relay-initialize" => {
                if value["response"]["subtype"] != "success" {
                    return Err(Error::invalid(
                        "Claude Code rejected initialization; check installed version and execution mode",
                    ));
                }
                if !sent {
                    send(stdin.as_mut().ok_or_else(|| Error::invalid("Claude input already closed"))?, json!({"type":"user","session_id":previous.unwrap_or(""),"uuid":run,"parent_tool_use_id":null,"message":{"role":"user","content":content}})).await?;
                    sent = true;
                    workspace.update_run(session, run, |snapshot| {
                        crate::agent_tools::mark_read(snapshot, run);
                        Ok(())
                    })?;
                    deadline = None;
                }
            }
            "control_request" => {
                let request = &value["request"];
                let response = if request["subtype"] == "can_use_tool" {
                    let allow = crate::harness::permission(
                        workspace,
                        session,
                        run,
                        request["tool_name"].as_str().unwrap_or("Tool"),
                        &request["input"].to_string(),
                        stop,
                    )
                    .await?;
                    if allow {
                        json!({"behavior":"allow","updatedInput":request["input"]})
                    } else {
                        json!({"behavior":"deny","message":"User denied this action in Relay"})
                    }
                } else {
                    json!({"behavior":"deny","message":"Relay does not support this control request"})
                };
                send(stdin.as_mut().ok_or_else(|| Error::invalid("Claude input already closed"))?, json!({"type":"control_response","response":{"subtype":"success","request_id":value["request_id"],"response":response}})).await?;
            }
            "system" if value["subtype"] == "init" => {
                let id = value["session_id"]
                    .as_str()
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .ok_or_else(|| Error::invalid("Claude session identity missing"))?
                    .to_owned();
                thread = Some(id.clone());
                if let Some(fork) = fork {
                    crate::commands::publish_fork(workspace, session, run, fork, &id)?;
                    continue;
                }
                workspace.update_run(session, run, |snapshot| {
                    let worker = snapshot
                        .sessions
                        .iter_mut()
                        .find(|s| s.id == session)
                        .unwrap()
                        .worker
                        .as_mut()
                        .unwrap();
                    worker.thread_id = Some(id);
                    worker.model = value["model"]
                        .as_str()
                        .filter(|model| !model.is_empty())
                        .map(str::to_owned);
                    worker.context_tokens = None;
                    worker.context_window = None;
                    if let Some(submission) = snapshot.submissions.iter_mut().find(|s| s.id == run)
                    {
                        submission.state = SubmissionState::Running;
                    }
                    Ok(())
                })?;
            }
            "system" => {
                let subtype = value["subtype"].as_str().unwrap_or("");
                if subtype == "slash_commands_updated" {
                    workspace.catalogs.lock().map_err(Error::internal)?.clear();
                }
                if subtype == "compact_boundary" {
                    compactions += 1;
                    workspace.update_run(session, run, |snapshot| {
                        let key = if compactions == 1 {
                            "compaction".to_owned()
                        } else {
                            format!("compaction-{compactions}")
                        };
                        message(
                            snapshot,
                            session,
                            run,
                            &key,
                            "harness_command",
                            "Compaction completed",
                            false,
                        );
                        let session = snapshot
                            .sessions
                            .iter_mut()
                            .find(|s| s.id == session)
                            .unwrap();
                        let at_tokens = value["compact_metadata"]["pre_tokens"]
                            .as_u64()
                            .or_else(|| session.worker.as_ref().and_then(|w| w.context_tokens));
                        session.compactions.push(Compaction { at_tokens });
                        Ok(())
                    })?;
                }
                if subtype == "session_state_changed" {
                    state = value["state"].as_str().map(str::to_owned);
                }
                if let Some(id) = value["task_id"].as_str() {
                    match subtype {
                        "task_started"
                            if matches!(
                                value["task_type"].as_str(),
                                Some("local_agent" | "local_workflow")
                            ) =>
                        {
                            tasks.insert(id.to_owned());
                        }
                        "task_notification" => {
                            tasks.remove(id);
                        }
                        "task_updated"
                            if matches!(
                                value["patch"]["status"].as_str(),
                                Some("completed" | "failed" | "stopped" | "cancelled")
                            ) =>
                        {
                            tasks.remove(id);
                        }
                        _ => {}
                    }
                }
            }
            "user" => {
                if let Some(blocks) = value["message"]["content"].as_array() {
                    // The structured result describes the event's only tool result.
                    let structured =
                        if blocks.iter().filter(|b| b["type"] == "tool_result").count() == 1 {
                            &value["tool_use_result"]
                        } else {
                            &Value::Null
                        };
                    workspace.update_run(session, run, |s| {
                        for block in blocks {
                            if block["type"] == "tool_result" {
                                let text = if let Some(text) = block["content"].as_str() {
                                    text.to_owned()
                                } else if let Some(blocks) = block["content"].as_array() {
                                    blocks
                                        .iter()
                                        .map(|b| {
                                            b["text"]
                                                .as_str()
                                                .map(str::to_owned)
                                                .unwrap_or_else(|| b.to_string())
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                } else {
                                    block["content"].to_string()
                                };
                                let id = format!(
                                    "claude-{run}-tool-{}",
                                    block["tool_use_id"].as_str().unwrap_or("result")
                                );
                                if !s.messages.iter().any(|m| m.id == id) {
                                    tool_activity::start(
                                        s,
                                        session,
                                        id.clone(),
                                        "Claude Code",
                                        tool_activity::call(
                                            run,
                                            "Tool",
                                            ToolKind::Other,
                                            String::new(),
                                            String::new(),
                                        ),
                                    );
                                }
                                let message = s.messages.iter_mut().find(|m| m.id == id).unwrap();
                                tool_activity::output(message, &text, false);
                                let failed = block["is_error"].as_bool().unwrap_or(false);
                                let finished = message
                                    .tool
                                    .as_ref()
                                    .is_some_and(|t| t.finished_ms.is_some());
                                tool_activity::finish(
                                    message,
                                    if failed {
                                        ToolState::Failed
                                    } else {
                                        ToolState::Completed
                                    },
                                );
                                if !finished
                                    && let Some((name, input)) = block["tool_use_id"]
                                        .as_str()
                                        .and_then(|id| tool_inputs.get(id))
                                    && let Some(tool) = &mut message.tool
                                {
                                    let (kind, detail) =
                                        result_detail(name, input, &text, structured, failed);
                                    if let Some(kind) = kind {
                                        tool.kind = kind;
                                    }
                                    if let Some(detail) = detail {
                                        tool.detail = Some(detail.bounded());
                                    }
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
            }
            "rate_limit_event" => {
                workspace.update_run(session, run, |s| {
                    rate_limits(s, &value["rate_limit_info"], crate::now());
                    Ok(())
                })?;
            }
            "stream_event" => {
                let event = &value["event"];
                let stream = value["parent_tool_use_id"].as_str().unwrap_or("");
                if event["type"] == "message_start" {
                    current_messages.insert(
                        stream.to_owned(),
                        event["message"]["id"]
                            .as_str()
                            .unwrap_or("response")
                            .to_owned(),
                    );
                }
                if event["type"] == "content_block_delta"
                    && let Some(text) = event["delta"]["text"].as_str()
                {
                    let id = format!(
                        "{}-{}",
                        current_messages
                            .get(stream)
                            .map(String::as_str)
                            .unwrap_or("response"),
                        event["index"].as_u64().unwrap_or(0)
                    );
                    workspace.update_run(session, run, |s| {
                        message(s, session, run, &id, "agent_message", text, true);
                        Ok(())
                    })?;
                }
            }
            "assistant" => {
                let id = value["message"]["id"].as_str().unwrap_or("response");
                if let Some(blocks) = value["message"]["content"].as_array() {
                    for block in blocks {
                        if block["type"] == "tool_use"
                            && let Some(id) = block["id"].as_str()
                        {
                            tool_inputs.insert(
                                id.to_owned(),
                                (
                                    block["name"].as_str().unwrap_or("Tool").to_owned(),
                                    block["input"].clone(),
                                ),
                            );
                        }
                        if block["type"] == "tool_use"
                            && block["name"] == "Skill"
                            && let Some(name) = block["input"]["skill"].as_str()
                        {
                            invoked_skills.insert(name.to_owned());
                        }
                    }

                    workspace.update_run(session, run, |s| {
                        context(s, session, &value);
                        for (index, block) in blocks.iter().enumerate() {
                            let key = format!("{id}-{index}");
                            match block["type"].as_str() {
                                Some("text") => message(
                                    s,
                                    session,
                                    run,
                                    &key,
                                    "agent_message",
                                    block["text"].as_str().unwrap_or(""),
                                    false,
                                ),
                                Some("tool_use") => {
                                    let name = block["name"].as_str().unwrap_or("Tool");
                                    let id = format!(
                                        "claude-{run}-tool-{}",
                                        block["id"].as_str().unwrap_or(&key)
                                    );
                                    let input = &block["input"];
                                    let (kind, detail) = use_detail(name, input);
                                    tool_activity::start(
                                        s,
                                        session,
                                        id,
                                        "Claude Code",
                                        ToolCall {
                                            directory: input["cwd"]
                                                .as_str()
                                                .map(tool_activity::bounded),
                                            detail: detail.map(ToolDetail::bounded),
                                            ..tool_activity::call(
                                                run,
                                                name,
                                                kind,
                                                tool_activity::bounded(&input.to_string()),
                                                tool_activity::target(input),
                                            )
                                        },
                                    );
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    })?;
                }
            }
            "result" => {
                if thread.is_none() {
                    return Err(Error::invalid(
                        "Claude completed without a session identity",
                    ));
                }
                if value["is_error"] == true || value["subtype"] != "success" {
                    return Err(Error::invalid(
                        "Claude Code turn failed; check authentication, execution mode, and harness configuration",
                    ));
                }
                workspace.update_run(session, run, |s| {
                    if let Some(result) = value["result"].as_str().filter(|s| !s.is_empty())
                        && !s.messages.iter().any(|m| {
                            m.session_id == session
                                && m.id.starts_with(&format!("claude-{run}-"))
                                && m.body == result
                        })
                    {
                        message(s, session, run, "result", "harness_command", result, false);
                    }
                    let usage = &value["usage"];
                    if let (Some(input), Some(output)) = (
                        usage["input_tokens"].as_u64(),
                        usage["output_tokens"].as_u64(),
                    ) {
                        let cached = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                        let creation = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                        s.sessions
                            .iter_mut()
                            .find(|s| s.id == session)
                            .unwrap()
                            .worker
                            .as_mut()
                            .unwrap()
                            .usage = Some(TokenUsage {
                            input_tokens: input.saturating_add(cached).saturating_add(creation),
                            cached_input_tokens: cached,
                            output_tokens: output,
                        });
                        let worker = s
                            .sessions
                            .iter_mut()
                            .find(|s| s.id == session)
                            .unwrap()
                            .worker
                            .as_mut()
                            .unwrap();
                        worker.last_usage = worker.usage.clone();
                    }
                    context_window(s, session, &value["modelUsage"]);
                    Ok(())
                })?;
                if let Some(missing) = expected_skills
                    .iter()
                    .find(|name| !invoked_skills.contains(*name))
                {
                    return Err(Error::invalid(format!(
                        "Claude did not invoke the selected skill ${missing}; review the result before retrying"
                    )));
                }
                completed = true;
            }
            _ => {}
        }
        // A result ends a turn, but delegated work may still need the control
        // channel. Follow the native SDK's task/state markers before EOF.
        if completed
            && tasks.is_empty()
            && state.as_deref().is_none_or(|s| s == "idle")
            && stdin.is_some()
        {
            drop(stdin.take());
            deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(10));
        }
    }
    // The native success result is the turn's outcome. Leave the child
    // unreaped so the runtime can terminate its owned process group before
    // waiting; reaping here would lose the group identity and leak children.
    Ok(())
}

/// Claude reports subscription windows as fractions; API-key sessions omit them.
pub(super) fn rate_limits(snapshot: &mut Snapshot, info: &Value, now: u64) {
    let mut record = |kind: &str, window: &Value| {
        let (kind, minutes) = match kind {
            "five_hour" => (UsageWindow::FiveHour, 5 * 60),
            "seven_day" => (UsageWindow::Weekly, 7 * 24 * 60),
            _ => return,
        };
        let Some(utilization) = window["utilization"].as_f64() else {
            return;
        };
        snapshot.record_usage_limit(UsageLimit::new(
            Harness::ClaudeCode,
            kind,
            utilization * 100.0,
            minutes,
            window["resetsAt"].as_u64(),
            now,
        ));
    };
    if let Some(windows) = info["unifiedWindows"].as_object() {
        for (kind, window) in windows {
            record(kind, window);
        }
    } else if let Some(kind) = info["rateLimitType"].as_str() {
        record(kind, info);
    }
}

/// Kind and detail known when a tool is requested.
fn use_detail(name: &str, input: &Value) -> (ToolKind, Option<ToolDetail>) {
    let kind = ToolKind::from_name(name);
    match name {
        "Bash" if input["command"].as_str().is_some_and(is_test_command) => (ToolKind::Test, None),
        "TodoWrite" => (
            kind,
            input["todos"].as_array().map(|todos| ToolDetail::Plan {
                steps: todos
                    .iter()
                    .filter_map(|todo| {
                        Some(PlanStep {
                            title: todo["content"].as_str()?.to_owned(),
                            done: todo["status"] == "completed",
                            owner_session_id: None,
                        })
                    })
                    .collect(),
            }),
        ),
        "Edit" | "MultiEdit" => {
            let edits = if name == "Edit" {
                vec![input]
            } else {
                input["edits"]
                    .as_array()
                    .map(|e| e.iter().collect())
                    .unwrap_or_default()
            };
            let (mut added, mut removed) = (0, 0);
            for edit in edits {
                let (a, r) = line_changes(
                    edit["old_string"].as_str().unwrap_or(""),
                    edit["new_string"].as_str().unwrap_or(""),
                );
                added += a;
                removed += r;
            }
            (
                kind,
                Some(ToolDetail::Edit {
                    hunks: vec![],
                    added,
                    removed,
                }),
            )
        }
        "Grep" | "Glob" => (
            kind,
            Some(ToolDetail::Search {
                query: input["pattern"].as_str().unwrap_or("").to_owned(),
                files: vec![],
            }),
        ),
        _ => (kind, None),
    }
}

/// Lines removed and added between two replaced passages, ignoring a shared prefix and suffix.
fn line_changes(old: &str, new: &str) -> (u64, u64) {
    let old = old.lines().collect::<Vec<_>>();
    let new = new.lines().collect::<Vec<_>>();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (
        (new.len() - prefix - suffix) as u64,
        (old.len() - prefix - suffix) as u64,
    )
}

/// Hunks from Claude's `structuredPatch`, when it reported one.
fn patch_detail(patch: &Value) -> Option<ToolDetail> {
    let patch = patch.as_array().filter(|p| !p.is_empty())?;
    let (mut added, mut removed) = (0, 0);
    let hunks = patch
        .iter()
        .map(|hunk| {
            let lines = hunk["lines"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let count = |prefix| {
                lines
                    .iter()
                    .filter(|l| l.as_str().is_some_and(|l| l.starts_with(prefix)))
                    .count() as u64
            };
            let (a, r) = (count('+'), count('-'));
            added += a;
            removed += r;
            Hunk {
                line: hunk["newStart"].as_u64().unwrap_or(0),
                label: None,
                added: a,
                removed: r,
            }
        })
        .collect();
    Some(ToolDetail::Edit {
        hunks,
        added,
        removed,
    })
}

/// Kind and detail known once a tool reports its result. `structured` is the
/// event's `tool_use_result`, or null when it cannot be attributed to this call.
fn result_detail(
    name: &str,
    input: &Value,
    text: &str,
    structured: &Value,
    failed: bool,
) -> (Option<ToolKind>, Option<ToolDetail>) {
    if failed && name != "Bash" {
        return (None, None);
    }
    match name {
        "Write" => {
            let created = structured["type"] == "create"
                || (structured.is_null() && text.starts_with("File created"));
            if created {
                let content = input["content"]
                    .as_str()
                    .or_else(|| structured["content"].as_str())
                    .unwrap_or("");
                (
                    Some(ToolKind::Create),
                    Some(ToolDetail::Create {
                        lines: line_count(content),
                    }),
                )
            } else {
                (None, patch_detail(&structured["structuredPatch"]))
            }
        }
        "Edit" | "MultiEdit" => (None, patch_detail(&structured["structuredPatch"])),
        "Read" => {
            let file = &structured["file"];
            let start = file["startLine"]
                .as_u64()
                .or_else(|| input["offset"].as_u64())
                .unwrap_or(1)
                .max(1);
            let lines = file["numLines"].as_u64().or_else(|| {
                structured
                    .is_null()
                    .then(|| text.lines().filter(|l| !l.trim().is_empty()).count() as u64)
            });
            (
                None,
                Some(ToolDetail::Read {
                    start_line: Some(start),
                    end_line: lines.filter(|n| *n > 0).map(|n| start + n - 1),
                    total_lines: file["totalLines"].as_u64(),
                }),
            )
        }
        "Glob" => {
            let files = structured["filenames"]
                .as_array()
                .map(|f| {
                    f.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_else(|| {
                    if text == "No files found" {
                        vec![]
                    } else {
                        text.lines()
                            .filter(|l| !l.is_empty() && !l.starts_with('('))
                            .map(str::to_owned)
                            .collect()
                    }
                });
            (
                None,
                Some(ToolDetail::Search {
                    query: input["pattern"].as_str().unwrap_or("").to_owned(),
                    files: files
                        .into_iter()
                        .take(TOOL_DETAIL_LIMIT)
                        .map(|path| SearchFile { path, hits: 1 })
                        .collect(),
                }),
            )
        }
        "Grep" => {
            let mut files: Vec<SearchFile> = Vec::new();
            let mode = input["output_mode"]
                .as_str()
                .unwrap_or("files_with_matches");
            let content = structured["content"].as_str().unwrap_or(text);
            let known = structured["filenames"]
                .as_array()
                .map(|f| f.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            for line in content.lines() {
                let (path, hits) = match mode {
                    // "path:count"
                    "count" => match line.rsplit_once(':') {
                        Some((path, n)) => match n.trim().parse() {
                            Ok(n) => (path, n),
                            Err(_) => continue,
                        },
                        None => continue,
                    },
                    // "path:line:text"; attributed only to files the tool reported.
                    "content" => match line.split_once(':') {
                        Some((path, _)) if known.contains(&path) => (path, 1),
                        _ => continue,
                    },
                    _ => continue,
                };
                if let Some(file) = files.iter_mut().find(|f| f.path == path) {
                    file.hits += hits;
                } else if files.len() < TOOL_DETAIL_LIMIT {
                    files.push(SearchFile {
                        path: path.to_owned(),
                        hits,
                    });
                }
            }
            (
                None,
                Some(ToolDetail::Search {
                    query: input["pattern"].as_str().unwrap_or("").to_owned(),
                    files,
                }),
            )
        }
        "Bash" if input["command"].as_str().is_some_and(is_test_command) => (
            None,
            parse_test_summary(structured["stdout"].as_str().unwrap_or(text)),
        ),
        "WebFetch" => (
            None,
            structured["code"]
                .as_u64()
                .and_then(|c| u16::try_from(c).ok())
                .map(|status| ToolDetail::Fetch {
                    status: Some(status),
                    title: None,
                }),
        ),
        _ => (None, None),
    }
}

/// Context occupancy of the latest top-level request. Subagent messages have their own context.
fn context(snapshot: &mut Snapshot, session: &str, value: &Value) {
    let usage = &value["message"]["usage"];
    if !value["parent_tool_use_id"].is_null() {
        return;
    }
    let Some(input) = usage["input_tokens"].as_u64() else {
        return;
    };
    let tokens = input
        .saturating_add(usage["cache_read_input_tokens"].as_u64().unwrap_or(0))
        .saturating_add(usage["cache_creation_input_tokens"].as_u64().unwrap_or(0));
    if let Some(worker) = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session)
        .and_then(|s| s.worker.as_mut())
    {
        worker.context_tokens = Some(tokens);
    }
}

/// The reporting model's window; ambiguous multi-model results are left unset.
fn context_window(snapshot: &mut Snapshot, session: &str, usage: &Value) {
    let Some(models) = usage.as_object() else {
        return;
    };
    let Some(worker) = snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == session)
        .and_then(|s| s.worker.as_mut())
    else {
        return;
    };
    let entry = worker
        .model
        .as_deref()
        .and_then(|model| {
            models
                .iter()
                .find(|(name, _)| name.as_str() == model || name.starts_with(model))
        })
        .or_else(|| (models.len() == 1).then(|| models.iter().next()).flatten());
    if let Some(window) = entry
        .and_then(|(_, usage)| usage["contextWindow"].as_u64())
        .filter(|w| *w > 0)
    {
        worker.context_window = Some(window);
    }
}
