//! Recorded tool activity: older transcripts' tool messages gain typed calls.
use relay_core::{Message, ToolKind, ToolState};

/// Old transcripts lack lifecycle metadata. Give them a typed presentation
/// while explicitly leaving their status unknown; never infer success from text.
pub(crate) fn with_legacy_tool(mut message: Message) -> Message {
    if message.tool.is_some()
        || !matches!(message.kind.as_str(), "command_execution" | "file_change")
    {
        return message;
    }
    let (name, kind, input, target, output, code) =
        if let Some(command) = message.body.strip_prefix("Command: ") {
            let mut lines = command.splitn(3, '\n');
            let command = lines.next().unwrap_or("").to_owned();
            let code = lines
                .next()
                .and_then(|s| s.strip_prefix("Exit: "))
                .and_then(|s| s.parse::<i64>().ok());
            (
                "commandExecution".into(),
                ToolKind::Run,
                command.clone(),
                command,
                lines.next().unwrap_or("").into(),
                code,
            )
        } else if message.kind == "file_change" {
            (
                "fileChange".into(),
                ToolKind::Edit,
                message.body.clone(),
                "Recorded file changes".into(),
                message.body.clone(),
                None,
            )
        } else if let Some((name, input)) = message.body.split_once('\n')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(input)
            && value.is_object()
        {
            let target = [
                "command",
                "file_path",
                "path",
                "pattern",
                "query",
                "url",
                "description",
            ]
            .into_iter()
            .find_map(|key| value[key].as_str())
            .unwrap_or(name);
            (
                name.into(),
                ToolKind::from_name(name),
                input.into(),
                target.into(),
                String::new(),
                None,
            )
        } else {
            (
                "Recorded tool".into(),
                ToolKind::Other,
                String::new(),
                "Recorded tool output".into(),
                message.body.clone(),
                None,
            )
        };
    message.tool = Some(relay_core::ToolCall {
        run_id: String::new(),
        name,
        kind,
        state: ToolState::Unknown,
        input,
        target,
        directory: None,
        exit_code: code,
        started_ms: None,
        finished_ms: None,
        detail: None,
    });
    message.body = output;
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_activity_uses_recorded_type_without_inventing_status() {
        let message = Message {
            id: "old".into(),
            session_id: "session".into(),
            author: "Codex".into(),
            kind: "command_execution".into(),
            body: "Command: cargo test\nExit: 0\ntests passed".into(),
            parts: vec![],
            tool: None,
        };
        let shown = with_legacy_tool(message.clone());
        assert_eq!(shown.tool.as_ref().unwrap().kind, ToolKind::Run);
        assert_eq!(shown.tool.as_ref().unwrap().state, ToolState::Unknown);
        assert_eq!(shown.tool.as_ref().unwrap().target, "cargo test");
        assert_eq!(shown.body, "tests passed");
        assert_eq!(message.body, "Command: cargo test\nExit: 0\ntests passed");
    }
}
