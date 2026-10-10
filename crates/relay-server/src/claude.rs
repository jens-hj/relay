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
    cmd.arg(match mode {
        ApprovalMode::Automatic => "auto",
        ApprovalMode::Ask => "default",
        ApprovalMode::Unrestricted => "bypassPermissions",
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
        cmd.arg("--settings").arg(json!({"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":true,"filesystem":{"allowWrite":roots},"network":{"allowAllUnixSockets":false}}}).to_string());
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
pub(super) async fn execute(
    workspace: &Workspace,
    session: &str,
    run: &str,
    input: Vec<Value>,
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
    let mut sent = false;
    let mut thread = previous.map(str::to_owned);
    let mut current_messages = std::collections::HashMap::new();
    let mut completed = false;
    let mut state: Option<String> = None;
    let mut tasks = std::collections::HashSet::new();
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
                    workspace.update_run(session, run, |s| {
                        for (index, block) in blocks.iter().enumerate() {
                            if block["type"] == "tool_result" {
                                let text = if let Some(text) = block["content"].as_str() {
                                    text.to_owned()
                                } else {
                                    block["content"].to_string()
                                };
                                let id = format!(
                                    "tool-{}-{index}",
                                    block["tool_use_id"].as_str().unwrap_or("result")
                                );
                                message(s, session, run, &id, "command_execution", &text, false);
                            }
                        }
                        Ok(())
                    })?;
                }
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
                    workspace.update_run(session, run, |s| {
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
                                Some("tool_use") => message(
                                    s,
                                    session,
                                    run,
                                    &key,
                                    "command_execution",
                                    &format!(
                                        "{}\n{}",
                                        block["name"].as_str().unwrap_or("Tool"),
                                        block["input"]
                                    ),
                                    false,
                                ),
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
                    }
                    Ok(())
                })?;
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
