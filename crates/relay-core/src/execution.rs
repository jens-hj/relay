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
/// Whether a project's agents may run `nix develop --command` through Relay
/// on the server, outside their harness sandbox. The command can execute any
/// code in the workspace with the server user's access, so this is execution
/// authority, not a sandbox: the project owner opts in, and each agent also
/// needs its Verify and Implement permissions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NixCommands {
    #[default]
    Off,
    Ask,
    Allow,
}
impl NixCommands {
    pub const ALL: [Self; 3] = [Self::Off, Self::Ask, Self::Allow];
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Ask => "Ask",
            Self::Allow => "Allow",
        }
    }
    pub fn is_off(&self) -> bool {
        *self == Self::Off
    }
    /// An agent's permission to run Nix commands: Deny when the project has
    /// them off, otherwise the strictest of this setting and the agent's
    /// Verify and Implement permissions. Allow therefore needs all three.
    pub fn permission(
        self,
        permissions: &std::collections::BTreeMap<crate::Task, crate::Permission>,
    ) -> crate::Permission {
        use crate::{Permission, Task};
        let strictness = |p: Permission| match p {
            Permission::Deny => 0,
            Permission::Ask => 1,
            Permission::Allow => 2,
        };
        let setting = match self {
            Self::Off => Permission::Deny,
            Self::Ask => Permission::Ask,
            Self::Allow => Permission::Allow,
        };
        [Task::Verify, Task::Implement]
            .iter()
            .map(|task| permissions.get(task).copied().unwrap_or(Permission::Deny))
            .fold(setting, |strictest, p| {
                if strictness(p) < strictness(strictest) {
                    p
                } else {
                    strictest
                }
            })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Permission, Project, Task};

    #[test]
    fn nix_commands_take_the_strictest_of_the_setting_verify_and_implement() {
        use Permission::{Allow, Ask, Deny};
        let permissions = |verify, implement| {
            [(Task::Verify, verify), (Task::Implement, implement)]
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        // Off denies whatever the profile allows.
        assert_eq!(NixCommands::Off.permission(&permissions(Allow, Allow)), Deny);
        // Allow needs the owner's Allow and both profile permissions.
        assert_eq!(NixCommands::Allow.permission(&permissions(Allow, Allow)), Allow);
        assert_eq!(NixCommands::Allow.permission(&permissions(Ask, Allow)), Ask);
        assert_eq!(NixCommands::Allow.permission(&permissions(Allow, Ask)), Ask);
        assert_eq!(NixCommands::Allow.permission(&permissions(Deny, Allow)), Deny);
        assert_eq!(NixCommands::Allow.permission(&permissions(Allow, Deny)), Deny);
        assert_eq!(NixCommands::Ask.permission(&permissions(Allow, Allow)), Ask);
        assert_eq!(NixCommands::Ask.permission(&permissions(Allow, Deny)), Deny);
        // A missing permission counts as Deny.
        assert_eq!(
            NixCommands::Allow.permission(&std::collections::BTreeMap::new()),
            Deny
        );
    }

    #[test]
    fn projects_without_the_setting_load_off_and_serialize_unchanged() {
        let project = crate::demo_snapshot(crate::AgentProfile::default()).projects[0].clone();
        let json = serde_json::to_value(&project).unwrap();
        assert!(json.get("nix_commands").is_none());
        let loaded: Project = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(loaded.nix_commands, NixCommands::Off);
        let enabled = Project {
            nix_commands: NixCommands::Ask,
            ..project
        };
        let json = serde_json::to_value(&enabled).unwrap();
        assert_eq!(json["nix_commands"], "ask");
        let loaded: Project = serde_json::from_value(json).unwrap();
        assert_eq!(loaded, enabled);
    }
}
