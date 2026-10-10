//! Structured, persisted tool activity shared by the harness adapters and chat.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Terminal,
    Read,
    Edit,
    Search,
    Web,
    Agent,
    Other,
}

impl ToolKind {
    pub fn from_name(name: &str) -> Self {
        let normalized = name.to_ascii_lowercase();
        let name = normalized.rsplit("__").next().unwrap_or(&normalized);
        match name {
            "bash" | "shell" | "exec_command" | "write_stdin" | "commandexecution" => {
                Self::Terminal
            }
            "read" | "read_file" => Self::Read,
            "edit" | "write" | "multiedit" | "apply_patch" | "filechange" => Self::Edit,
            "grep" | "glob" | "search" | "filesearch" => Self::Search,
            "websearch" | "webfetch" | "web_search" | "web.run" => Self::Web,
            "agent" | "task" | "collabagenttoolcall" => Self::Agent,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    Unknown,
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub run_id: String,
    pub name: String,
    pub kind: ToolKind,
    pub state: ToolState,
    /// Original JSON input, or the command for a native terminal item.
    pub input: String,
    pub target: String,
    pub directory: Option<String>,
    pub exit_code: Option<i64>,
}

/// A turn can end before individual tools publish results. Preserve uncertainty
/// rather than leaving an old call running or claiming that it succeeded.
pub fn interrupt_tools(snapshot: &mut crate::Snapshot, session: &str, run: Option<&str>) -> bool {
    let mut changed = false;
    for message in &mut snapshot.messages {
        if message.session_id == session
            && let Some(tool) = &mut message.tool
            && run.is_none_or(|run| tool.run_id == run)
            && tool.state == ToolState::Running
        {
            tool.state = ToolState::Interrupted;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_messages_and_structured_activity_round_trip() {
        let legacy: crate::Message = serde_json::from_str(r#"{"id":"m","session_id":"s","author":"Codex","kind":"command_execution","body":"old output"}"#).unwrap();
        assert!(legacy.tool.is_none());
        let mut message = legacy;
        message.tool = Some(ToolCall {
            run_id: "r".into(),
            name: "Bash".into(),
            kind: ToolKind::Terminal,
            state: ToolState::Failed,
            input: "false".into(),
            target: "false".into(),
            directory: Some("/workspace".into()),
            exit_code: Some(1),
        });
        assert_eq!(
            serde_json::from_str::<crate::Message>(&serde_json::to_string(&message).unwrap())
                .unwrap(),
            message
        );
    }
}
