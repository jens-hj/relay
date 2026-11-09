//! Name-only optimistic edits; profile replacements retain their global revision contract.
use crate::*;
use serde::{Deserialize, Serialize};

pub const CONFIG_RENAME_BODY_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfigRenameGuard {
    pub agent_config_id: String,
    pub name: String,
    pub automatic_name: bool,
}
impl AgentConfigRenameGuard {
    pub fn capture(config: &AgentConfig) -> Self {
        Self {
            agent_config_id: config.id.clone(),
            name: config.name.clone(),
            automatic_name: config.automatic_name,
        }
    }
    pub fn renamed(&self, input: &str) -> Result<Self, String> {
        let (name, automatic_name) = normalized_agent_config_name(input)?;
        Ok(Self {
            name,
            automatic_name,
            ..self.clone()
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentConfigRenameError {
    #[error("Agent name changed. Load the latest name before renaming again.")]
    Changed,
    #[error("{0}")]
    Invalid(String),
}

/// Preserve the existing explicit/automatic name rules, including whitespace-only input.
pub fn normalized_agent_config_name(input: &str) -> Result<(String, bool), String> {
    if !input.trim().is_empty() && input.len() > 100 {
        return Err("Agent name must be nonempty and at most 100 bytes".into());
    }
    let automatic = input.trim().is_empty();
    Ok((
        if automatic {
            "New agent".into()
        } else {
            input.trim().into()
        },
        automatic,
    ))
}

impl Snapshot {
    pub fn validate_agent_config_rename(
        &self,
        id: &str,
        name: &str,
        expected: &AgentConfigRenameGuard,
    ) -> Result<(), AgentConfigRenameError> {
        let invalid = AgentConfigRenameError::Invalid;
        if id.is_empty()
            || id.len() > CONFIG_RENAME_BODY_LIMIT
            || expected.name.len() > CONFIG_RENAME_BODY_LIMIT
            || name.len() > CONFIG_RENAME_BODY_LIMIT
        {
            return Err(invalid(
                "Rename input exceeds the command size limit".into(),
            ));
        }
        if expected.agent_config_id != id {
            return Err(invalid("Rename guard belongs to another agent".into()));
        }
        normalized_agent_config_name(name).map_err(invalid)?;
        let config = self
            .agent_configs
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| invalid("Director not found".into()))?;
        // Retain current reference/profile validation, without replacing any of that state.
        let profile = self.effective_profile(config).map_err(invalid)?;
        self.validate_profile(&config.project_id, &profile)
            .map_err(invalid)?;
        if AgentConfigRenameGuard::capture(config) != *expected {
            return Err(AgentConfigRenameError::Changed);
        }
        Ok(())
    }

    pub(crate) fn set_agent_config_name(&mut self, id: &str, name: String, automatic: bool) {
        let config = self
            .agent_configs
            .iter_mut()
            .find(|c| c.id == id)
            .expect("validated agent config");
        config.name = name.clone();
        config.automatic_name = automatic;
        for session in self
            .sessions
            .iter_mut()
            .filter(|s| s.agent_config_id == id && s.role == SessionRole::Root)
        {
            session.automatic_name = automatic;
            if !automatic {
                session.title = name.clone();
            } else if let Some(message) = self.messages.iter().find(|m| {
                m.session_id == session.id && matches!(m.kind.as_str(), "user" | "prompt")
            }) {
                session.title = message
                    .body
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(snapshot: &Snapshot, name: &str) -> Command {
        let config = &snapshot.agent_configs[0];
        Command::RenameAgentConfig {
            agent_config_id: config.id.clone(),
            name: name.into(),
            expected: AgentConfigRenameGuard::capture(config),
        }
    }
    #[test]
    fn rename_contract_requires_complete_guard_and_preserves_legacy_bytes() {
        let legacy = r#"{"request_id":"00000000-0000-4000-8000-000000000001","expected_revision":0,"command":{"type":"update_agent_config","agent_config_id":"config","name":"Name","overrides":{}}}"#;
        let parsed: CommandEnvelope = serde_json::from_str(legacy).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), legacy);
        let snapshot = demo_snapshot(AgentProfile::default());
        let value = serde_json::to_value(command(&snapshot, "Name")).unwrap();
        let round: Command = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(round).unwrap(), value);
        for key in ["agent_config_id", "name", "automatic_name"] {
            let mut bad = value.clone();
            bad["expected"].as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<Command>(bad).is_err());
        }
        for bad in [serde_json::Value::Null, serde_json::json!({})] {
            let mut value = value.clone();
            value["expected"] = bad;
            assert!(serde_json::from_value::<Command>(value).is_err());
        }
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("expected");
        assert!(serde_json::from_value::<Command>(missing).is_err());
        let mut extra = value;
        extra["expected"]["unexpected"] = true.into();
        assert!(serde_json::from_value::<Command>(extra).is_err());
    }
    #[test]
    fn rename_preserves_profile_runtime_and_matches_existing_root_title_semantics() {
        let mut initial = demo_snapshot(AgentProfile::default());
        initial.agent_configs[0].draft_selection.model = Some("picked-model".into());
        for name in ["  Renamed  ", "", "   ", &"é".repeat(50), &"\0".repeat(100)] {
            let mut narrow = initial.clone();
            let mut legacy = initial.clone();
            narrow.apply(command(&initial, name), "request", 0).unwrap();
            legacy
                .apply(
                    Command::UpdateAgentConfig {
                        agent_config_id: initial.agent_configs[0].id.clone(),
                        name: name.into(),
                        overrides: initial.agent_configs[0].overrides.clone(),
                    },
                    "request",
                    0,
                )
                .unwrap();
            assert_eq!(narrow, legacy);
            for child in initial
                .sessions
                .iter()
                .filter(|s| s.role != SessionRole::Root)
            {
                assert_eq!(
                    narrow.sessions.iter().find(|s| s.id == child.id).unwrap(),
                    child
                );
            }
            assert_eq!(
                narrow.agent_configs[0].draft_selection,
                initial.agent_configs[0].draft_selection
            );
        }
    }
    #[test]
    fn rename_rejects_changed_wrong_missing_target_and_current_reference_failures_atomically() {
        let initial = demo_snapshot(AgentProfile::default());
        let cmd = command(&initial, "New");
        for mode in 0..6 {
            let mut s = initial.clone();
            let mut c = cmd.clone();
            match mode {
                0 => s.agent_configs[0].name.push('x'),
                1 => s.agent_configs[0].automatic_name = !s.agent_configs[0].automatic_name,
                2 => {
                    if let Command::RenameAgentConfig {
                        agent_config_id, ..
                    } = &mut c
                    {
                        *agent_config_id = s.agent_configs[1].id.clone()
                    }
                }
                3 => {
                    s.agent_configs.remove(0);
                }
                4 => s.agent_configs[0].template = "missing".into(),
                _ => s.projects.clear(),
            }
            let before = s.clone();
            assert!(s.apply(c, "request", 0).is_err());
            assert_eq!(s, before);
        }
    }
    #[test]
    fn rename_envelope_stays_small_with_large_valid_policy_and_escaped_legal_names() {
        let base = AgentProfile {
            worker_models: WorkerModelPolicy::DelegatorChooses {
                allowed_models: (0..64)
                    .map(|i| ModelChoice {
                        harness: Harness::Codex,
                        model: format!("model-{i:02}-{}", "m".repeat(247)),
                    })
                    .collect(),
                max_effort: None,
                model_capability: true,
                available_usage: false,
            },
            ..Default::default()
        };
        let mut s = demo_snapshot(base);
        let mut ids = vec![];
        for i in 0..256 {
            let mut issue = s.issues[0].clone();
            issue.id = format!("00000000-0000-4000-8000-{i:012x}");
            ids.push(issue.id.clone());
            s.issues.push(issue);
        }
        s.projects[0].defaults.scope = AgentScope::Issues { issue_ids: ids };
        s.agent_configs[0].id = "agent-config-00000000-0000-4000-8000-000000000001".into();
        s.revision = u64::MAX - 1;
        s.agent_configs[0].name = "\0".repeat(100);
        let envelope = CommandEnvelope {
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            expected_revision: s.revision,
            command: command(&s, &"\0".repeat(100)),
        };
        let bytes = serde_json::to_vec(&envelope).unwrap().len();
        assert!(bytes < 2048);
        s.apply(envelope.command, "request", 0).unwrap();
        eprintln!(
            "rename_envelope_bytes={bytes} limit_bytes={CONFIG_RENAME_BODY_LIMIT} policy_models=64 scope_issues=256"
        );
    }
    #[test]
    fn rename_variant_is_fail_closed_on_legacy_server_contract() {
        #[derive(serde::Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum LegacyCommand {
            UpdateAgentConfig {
                agent_config_id: String,
                name: String,
                overrides: ProfileOverrides,
            },
        }
        let old:LegacyCommand=serde_json::from_str(r#"{"type":"update_agent_config","agent_config_id":"config","name":"Name","overrides":{}}"#).unwrap();
        let LegacyCommand::UpdateAgentConfig {
            agent_config_id,
            name,
            overrides,
        } = old;
        assert_eq!(agent_config_id, "config");
        assert_eq!(name, "Name");
        assert_eq!(overrides, ProfileOverrides::default());
        let snapshot = demo_snapshot(AgentProfile::default());
        assert!(
            serde_json::from_value::<LegacyCommand>(
                serde_json::to_value(command(&snapshot, "New")).unwrap()
            )
            .is_err()
        );
        assert!(normalized_agent_config_name(&"é".repeat(51)).is_err());
    }
}
