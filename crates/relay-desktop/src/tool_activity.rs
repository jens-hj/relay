//! Recorded tool activity: older transcripts' tool messages gain typed calls.
use relay_core::{Message, ToolState};

/// Old transcripts lack lifecycle metadata. Give them a typed presentation.
/// A recorded exit code is the outcome (zero completed, otherwise failed);
/// without one the status stays unknown. Never infer success from prose.
pub(crate) fn with_legacy_tool(mut message: Message) -> Message {
    // The same category the server counts this message under.
    let Some(kind) = relay_core::legacy_tool_kind(&message) else {
        return message;
    };
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
                kind,
                command.clone(),
                command,
                lines.next().unwrap_or("").into(),
                code,
            )
        } else if message.kind == "file_change" {
            (
                "fileChange".into(),
                kind,
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
                kind,
                input.into(),
                target.into(),
                String::new(),
                None,
            )
        } else {
            (
                "Recorded tool".into(),
                kind,
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
        state: match code {
            Some(0) => ToolState::Completed,
            Some(_) => ToolState::Failed,
            None => ToolState::Unknown,
        },
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
    use relay_core::ToolKind;

    #[test]
    fn legacy_activity_takes_its_status_from_the_recorded_exit_code() {
        let message = Message {
            id: "old".into(),
            session_id: "session".into(),
            author: "Codex".into(),
            sender: relay_core::Sender::User,
            kind: "command_execution".into(),
            body: "Command: cargo test\nExit: 0\ntests passed".into(),
            parts: vec![],
            tool: None,
        };
        let shown = with_legacy_tool(message.clone());
        assert_eq!(shown.tool.as_ref().unwrap().kind, ToolKind::Run);
        assert_eq!(shown.tool.as_ref().unwrap().state, ToolState::Completed);
        assert_eq!(shown.tool.as_ref().unwrap().target, "cargo test");
        assert_eq!(shown.body, "tests passed");
        assert_eq!(message.body, "Command: cargo test\nExit: 0\ntests passed");
        let failed = with_legacy_tool(Message {
            body: "Command: cargo test\nExit: 101\nfailures".into(),
            ..message.clone()
        });
        assert_eq!(failed.tool.as_ref().unwrap().state, ToolState::Failed);
        assert_eq!(failed.tool.as_ref().unwrap().exit_code, Some(101));
        // No recorded exit code: the outcome stays unknown, never guessed.
        let unknown = with_legacy_tool(Message {
            body: "Command: cargo test\ntests passed".into(),
            ..message.clone()
        });
        assert_eq!(unknown.tool.as_ref().unwrap().state, ToolState::Unknown);
        let change = with_legacy_tool(Message {
            kind: "file_change".into(),
            body: "src/lib.rs".into(),
            ..message
        });
        assert_eq!(change.tool.as_ref().unwrap().state, ToolState::Unknown);
    }
}
