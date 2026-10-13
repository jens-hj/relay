//! Harness-independent persistence for a tool's input and lifecycle.
use relay_core::*;
use serde_json::Value;

pub(super) fn bounded(value: &str) -> String {
    let mut end = value.len().min(TEXT_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub(super) fn target(input: &Value) -> String {
    [
        "command",
        "file_path",
        "path",
        "pattern",
        "query",
        "url",
        "description",
    ]
    .into_iter()
    .find_map(|key| input[key].as_str())
    .map(bounded)
    .unwrap_or_default()
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A new running call observed now. Details and exit codes are filled in on completion.
pub(super) fn call(
    run: &str,
    name: &str,
    kind: ToolKind,
    input: String,
    target: String,
) -> ToolCall {
    ToolCall {
        run_id: run.into(),
        name: name.into(),
        kind,
        state: ToolState::Running,
        input,
        target,
        directory: None,
        exit_code: None,
        started_ms: Some(now_ms()),
        finished_ms: None,
        detail: None,
    }
}

/// Record a terminal state once, preserving the first observed finish time.
pub(super) fn finish(message: &mut Message, state: ToolState) {
    if let Some(tool) = &mut message.tool {
        tool.state = state;
        tool.finished_ms.get_or_insert_with(now_ms);
    }
}

pub(super) fn start(
    snapshot: &mut Snapshot,
    session: &str,
    id: String,
    author: &str,
    tool: ToolCall,
) {
    if let Some(message) = snapshot.messages.iter_mut().find(|m| m.id == id) {
        // A repeated assistant/item event must not erase a result already seen.
        if message
            .tool
            .as_ref()
            .is_none_or(|t| t.state == ToolState::Running)
        {
            let mut tool = tool;
            if let Some(old) = &message.tool {
                tool.started_ms = old.started_ms.or(tool.started_ms);
                tool.detail = tool.detail.or_else(|| old.detail.clone());
            }
            message.tool = Some(tool);
        }
        return;
    }
    snapshot.messages.push(Message {
        id,
        session_id: session.into(),
        author: author.into(),
        kind: "tool_call".into(),
        body: String::new(),
        parts: vec![],
        tool: Some(tool),
    });
}

pub(super) fn output(message: &mut Message, output: &str, append: bool) {
    let next = if append {
        format!("{}{output}", message.body)
    } else {
        output.to_owned()
    };
    // Keep any prefix that another client may have used for a reply anchor.
    if next.starts_with(&message.body) {
        message.body = bounded(&next);
    }
}
