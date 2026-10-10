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
