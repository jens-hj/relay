use crate::Harness;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    Automatic,
    Ask,
    Unrestricted,
}
impl ApprovalMode {
    pub const ALL: [Self; 3] = [Self::Automatic, Self::Ask, Self::Unrestricted];
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Ask => "Ask",
            Self::Unrestricted => "Unrestricted Access",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSettings {
    #[serde(default)]
    pub approval: ApprovalMode,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBinding {
    pub repository: String,
    pub owner: String,
    pub number: u64,
    /// Absolute checkout path on the server, never the desktop machine.
    pub checkout: String,
}
impl ProjectBinding {
    pub fn project_id(&self) -> String {
        format!(
            "github-project:{}:{}:{}",
            self.owner, self.number, self.repository
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessInstallation {
    pub harness: Harness,
    pub executable: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessStatus {
    pub harness: Harness,
    pub executable: String,
    pub version: Option<String>,
    pub state: String,
    pub detail: String,
    pub checked_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPermission {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub tool: String,
    pub description: String,
    /// None while the harness request is pending. Terminal states survive reconnect.
    pub decision: Option<bool>,
    #[serde(default)]
    pub expired: bool,
}
