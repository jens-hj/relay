use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    #[default]
    Codex,
    ClaudeCode,
}

impl Harness {
    pub const ALL: [Self; 2] = [Self::Codex, Self::ClaudeCode];
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentScope {
    #[default]
    Project,
    Issues {
        issue_ids: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Plan,
    Delegate,
    Implement,
    Verify,
    Review,
    Merge,
    Deploy,
}

impl Task {
    pub const ALL: [Self; 7] = [
        Self::Plan,
        Self::Delegate,
        Self::Implement,
        Self::Verify,
        Self::Review,
        Self::Merge,
        Self::Deploy,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Delegate => "Delegate",
            Self::Implement => "Implement",
            Self::Verify => "Verify",
            Self::Review => "Review",
            Self::Merge => "Merge",
            Self::Deploy => "Deploy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Deny,
    Ask,
    Allow,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Deny => "Deny",
            Self::Ask => "Ask",
            Self::Allow => "Allow",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Deny => Self::Ask,
            Self::Ask => Self::Allow,
            Self::Allow => Self::Deny,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    /// Empty only for legacy profiles that have not selected a model yet.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub execution: crate::ExecutionSettings,
    pub harness: Harness,
    pub scope: AgentScope,
    pub responsibilities: Vec<Task>,
    pub completion: Vec<Task>,
    /// Zero pauses delegation. The upper bound prevents accidental unbounded fan-out.
    pub max_workers: u16,
    pub permissions: BTreeMap<Task, Permission>,
    /// The harness, model and effort this agent's workers may use.
    #[serde(default)]
    pub worker_models: crate::WorkerModelPolicy,
}

impl Default for AgentProfile {
    fn default() -> Self {
        Self::from_toml(include_str!("../../../profiles/default.toml"))
            .expect("bundled default profile is valid")
    }
}

impl AgentProfile {
    pub fn model_selection(&self) -> crate::HarnessSelection {
        crate::HarnessSelection {
            model: (!self.model.is_empty()).then(|| self.model.clone()),
            ..Default::default()
        }
    }
    pub fn worker(base: &Self) -> Self {
        let mut profile = base.clone();
        profile.responsibilities = vec![Task::Implement, Task::Verify];
        profile.completion = vec![Task::Verify];
        profile.permissions.insert(Task::Delegate, Permission::Deny);
        profile
    }

    pub fn can_delegate(&self) -> bool {
        self.max_workers > 0 && self.permissions[&Task::Delegate] != Permission::Deny
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.model.eq_ignore_ascii_case("default")
            || self.model.len() > 256
            || self.model.trim() != self.model
            || self.model.chars().any(char::is_control)
        {
            return Err("Choose a valid model identifier".into());
        }
        if self.max_workers > 64 {
            return Err("Worker limit must be between 0 and 64".into());
        }
        self.worker_models.validate()?;
        if self.responsibilities.is_empty() {
            return Err("Select at least one responsibility".into());
        }
        if self.completion.is_empty() {
            return Err("Select at least one completion requirement".into());
        }
        for list in [&self.responsibilities, &self.completion] {
            for (index, item) in list.iter().enumerate() {
                if list[..index].contains(item) {
                    return Err("Profile steps must not repeat".into());
                }
            }
        }
        if Task::ALL
            .iter()
            .any(|task| !self.permissions.contains_key(task))
        {
            return Err("Declare a permission for every action".into());
        }
        if let AgentScope::Issues { issue_ids } = &self.scope {
            if issue_ids.is_empty() {
                return Err("Issue scope requires at least one issue".into());
            }
            for (index, id) in issue_ids.iter().enumerate() {
                if id.trim().is_empty() || issue_ids[..index].contains(id) {
                    return Err("Scope issues must be nonempty and unique".into());
                }
            }
        }
        Ok(())
    }
    pub fn from_toml(source: &str) -> Result<Self, String> {
        let profile: Self = toml::from_str(source).map_err(|e| e.to_string())?;
        profile.validate()?;
        Ok(profile)
    }
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("profile is TOML serializable")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub profile: AgentProfile,
}

/// Missing fields stay inherited. Setting a field replaces that field as a whole.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::ExecutionSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<Harness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<AgentScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibilities: Option<Vec<Task>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<Vec<Task>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_workers: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<BTreeMap<Task, Permission>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_models: Option<crate::WorkerModelPolicy>,
}

impl ProfileOverrides {
    pub fn between(base: &AgentProfile, profile: &AgentProfile) -> Self {
        Self {
            model: (base.model != profile.model
                || (base.harness != profile.harness && !profile.model.is_empty()))
            .then(|| profile.model.clone()),
            execution: (base.execution != profile.execution).then(|| profile.execution.clone()),
            harness: (base.harness != profile.harness).then_some(profile.harness),
            scope: (base.scope != profile.scope).then(|| profile.scope.clone()),
            responsibilities: (base.responsibilities != profile.responsibilities)
                .then(|| profile.responsibilities.clone()),
            completion: (base.completion != profile.completion).then(|| profile.completion.clone()),
            max_workers: (base.max_workers != profile.max_workers).then_some(profile.max_workers),
            permissions: (base.permissions != profile.permissions)
                .then(|| profile.permissions.clone()),
            worker_models: (base.worker_models != profile.worker_models)
                .then(|| profile.worker_models.clone()),
        }
    }
    pub fn resolve(&self, base: &AgentProfile) -> AgentProfile {
        AgentProfile {
            model: self.model.clone().unwrap_or_else(|| {
                if self.harness.is_some_and(|h| h != base.harness) {
                    String::new()
                } else {
                    base.model.clone()
                }
            }),
            execution: self
                .execution
                .clone()
                .unwrap_or_else(|| base.execution.clone()),
            harness: self.harness.unwrap_or(base.harness),
            scope: self.scope.clone().unwrap_or_else(|| base.scope.clone()),
            responsibilities: self
                .responsibilities
                .clone()
                .unwrap_or_else(|| base.responsibilities.clone()),
            completion: self
                .completion
                .clone()
                .unwrap_or_else(|| base.completion.clone()),
            max_workers: self.max_workers.unwrap_or(base.max_workers),
            permissions: self
                .permissions
                .clone()
                .unwrap_or_else(|| base.permissions.clone()),
            worker_models: self
                .worker_models
                .clone()
                .unwrap_or_else(|| base.worker_models.clone()),
        }
    }
    pub fn from_toml(source: &str) -> Result<Self, String> {
        toml::from_str(source).map_err(|e| e.to_string())
    }
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("overrides are TOML serializable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_choice_round_trips_in_profiles_and_harness_changes_require_a_new_choice() {
        let base = AgentProfile {
            model: "codex-model".into(),
            ..AgentProfile::default()
        };
        let mut profile = base.clone();
        profile.harness = Harness::ClaudeCode;
        profile.model = "claude-model".into();
        let overrides = ProfileOverrides::between(&base, &profile);
        assert_eq!(overrides.resolve(&base), profile);
        assert_eq!(
            ProfileOverrides::from_toml(&overrides.to_toml()).unwrap(),
            overrides
        );
        assert_eq!(
            AgentProfile::from_toml(&profile.to_toml()).unwrap(),
            profile
        );
        assert_eq!(
            profile.model_selection().model.as_deref(),
            Some("claude-model")
        );
        assert!(
            ProfileOverrides {
                harness: Some(Harness::ClaudeCode),
                ..Default::default()
            }
            .resolve(&base)
            .model
            .is_empty()
        );
        let mut implicit = base.clone();
        implicit.model = "default".into();
        assert!(implicit.validate().is_err());
        let mut legacy = serde_json::to_value(&base).unwrap();
        legacy.as_object_mut().unwrap().remove("model");
        assert!(
            serde_json::from_value::<AgentProfile>(legacy)
                .unwrap()
                .model
                .is_empty()
        );
    }
    #[test]
    fn worker_presets_and_instance_overrides_keep_delegation_explicit() {
        let director = AgentProfile::default();
        assert!(director.can_delegate());
        let worker = AgentProfile::worker(&director);
        assert!(!worker.can_delegate());
        assert!(worker.responsibilities.contains(&Task::Implement));
        let overrides = ProfileOverrides::between(&director, &worker);
        assert_eq!(overrides.resolve(&director), worker);
        assert!(overrides.harness.is_none());
        assert!(overrides.execution.is_none());
    }

    #[test]
    fn manually_configured_child_profiles_are_shared_by_client_and_server() {
        let mut snapshot = crate::demo_snapshot(AgentProfile::default());
        let parent = snapshot.sessions[0].clone();
        let mut child = parent.clone();
        child.id = "child".into();
        child.parent_session_id = Some(parent.id.clone());
        let mut worker = AgentProfile::worker(&AgentProfile::default());
        worker.harness = Harness::ClaudeCode;
        worker.execution.approval = crate::ApprovalMode::Ask;
        child.profile = Some(worker.clone());
        snapshot.sessions.push(child.clone());
        assert_eq!(snapshot.session_profile(&child).unwrap(), worker);
        let mut descendant = child.clone();
        descendant.id = "descendant".into();
        descendant.parent_session_id = Some(child.id);
        descendant.profile = None;
        assert_eq!(snapshot.session_profile(&descendant).unwrap(), worker);
    }

    #[test]
    fn explicit_child_profiles_may_only_narrow_the_worker_model_policy() {
        let mut snapshot = crate::demo_snapshot(AgentProfile::default());
        let parent_profile = AgentProfile {
            worker_models: crate::WorkerModelPolicy::DelegatorChooses {
                allowed_models: vec![crate::ModelChoice {
                    harness: Harness::Codex,
                    model: "gpt-light".into(),
                }],
                max_effort: Some("high".into()),
                model_capability: true,
                available_usage: false,
            },
            ..AgentProfile::default()
        };
        snapshot.sessions[0].profile = Some(parent_profile.clone());
        let parent = snapshot.sessions[0].clone();
        let mut child = AgentProfile::worker(&parent_profile);
        assert!(snapshot.validate_child_profile(&parent, &child).is_ok());
        child.worker_models = crate::WorkerModelPolicy::Fixed {
            harness: Harness::Codex,
            model: "gpt-light".into(),
            effort: Some("low".into()),
        };
        assert!(snapshot.validate_child_profile(&parent, &child).is_ok());
        child.worker_models = crate::WorkerModelPolicy::default();
        assert!(
            snapshot
                .validate_child_profile(&parent, &child)
                .unwrap_err()
                .contains("cannot widen")
        );
    }

    #[test]
    fn named_profiles_persist_and_reject_duplicate_or_reserved_names() {
        let mut snapshot = crate::demo_snapshot(AgentProfile::default());
        let command = crate::Command::SaveAgentTemplate {
            project_id: snapshot.projects[0].id.clone(),
            name: "Reviewer".into(),
            profile: AgentProfile {
                model: "review-model".into(),
                ..AgentProfile::worker(&AgentProfile::default())
            },
        };
        snapshot.apply(command.clone(), "named", 0).unwrap();
        let stored: crate::Snapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(stored.agent_templates[0].name, "Reviewer");
        snapshot
            .apply(
                crate::Command::CreateAgentConfig {
                    project_id: snapshot.projects[0].id.clone(),
                    name: "Review agent".into(),
                    template: "profile-named".into(),
                    overrides: ProfileOverrides::default(),
                },
                "agent",
                0,
            )
            .unwrap();
        let config = snapshot.agent_configs.last().unwrap().clone();
        assert!(!snapshot.effective_profile(&config).unwrap().can_delegate());
        snapshot.projects[0]
            .defaults
            .permissions
            .insert(Task::Delegate, Permission::Allow);
        snapshot.projects[0].defaults.harness = Harness::ClaudeCode;
        assert_eq!(
            snapshot.effective_profile(&config).unwrap().harness,
            Harness::Codex
        );
        assert!(snapshot.apply(command, "duplicate", 0).is_err());
        assert!(
            snapshot
                .apply(
                    crate::Command::SaveAgentTemplate {
                        project_id: snapshot.projects[0].id.clone(),
                        name: "Worker".into(),
                        profile: AgentProfile::default()
                    },
                    "reserved",
                    0
                )
                .is_err()
        );
    }
    #[test]
    fn inherited_fields_follow_defaults_and_explicit_fields_stay_fixed() {
        let mut defaults = AgentProfile::default();
        let overrides = ProfileOverrides {
            max_workers: Some(2),
            ..Default::default()
        };
        defaults.max_workers = 8;
        defaults.harness = Harness::ClaudeCode;
        let resolved = overrides.resolve(&defaults);
        assert_eq!(resolved.max_workers, 2);
        assert_eq!(resolved.harness, Harness::ClaudeCode);
        assert_eq!(ProfileOverrides::default().resolve(&defaults), defaults);
    }
    #[test]
    fn worker_model_policy_is_an_inheritable_validated_profile_field() {
        let base = AgentProfile::default();
        assert_eq!(base.worker_models, crate::WorkerModelPolicy::default());
        let mut profile = base.clone();
        profile.worker_models = crate::WorkerModelPolicy::Fixed {
            harness: Harness::ClaudeCode,
            model: "claude-sonnet".into(),
            effort: Some("high".into()),
        };
        let overrides = ProfileOverrides::between(&base, &profile);
        assert_eq!(overrides.worker_models, Some(profile.worker_models.clone()));
        assert_eq!(overrides.resolve(&base), profile);
        assert_eq!(
            ProfileOverrides::from_toml(&overrides.to_toml()).unwrap(),
            overrides
        );
        assert_eq!(
            AgentProfile::from_toml(&profile.to_toml()).unwrap(),
            profile
        );
        assert!(ProfileOverrides::default().worker_models.is_none());
        assert!(
            !ProfileOverrides::default()
                .to_toml()
                .contains("worker_models")
        );
        // Workers keep their delegator's policy.
        assert_eq!(
            AgentProfile::worker(&profile).worker_models,
            profile.worker_models
        );
        profile.worker_models = crate::WorkerModelPolicy::DelegatorChooses {
            allowed_models: vec![],
            max_effort: Some("enormous".into()),
            model_capability: true,
            available_usage: true,
        };
        assert!(profile.validate().is_err());
        // Profiles stored before the policy existed load with the default.
        let mut legacy = serde_json::to_value(&base).unwrap();
        legacy.as_object_mut().unwrap().remove("worker_models");
        assert_eq!(
            serde_json::from_value::<AgentProfile>(legacy)
                .unwrap()
                .worker_models,
            crate::WorkerModelPolicy::default()
        );
    }

    #[test]
    fn toml_roundtrips_and_rejects_typos_or_invalid_limits() {
        let profile = AgentProfile::default();
        assert_eq!(
            AgentProfile::from_toml(&profile.to_toml()).unwrap(),
            profile
        );
        assert!(AgentProfile::from_toml(&(profile.to_toml() + "\n[unknown]\nx = 1\n")).is_err());
        assert!(
            AgentProfile::from_toml(
                &profile
                    .to_toml()
                    .replace("max_workers = 4", "max_workers = 65")
            )
            .is_err()
        );
        let overrides = ProfileOverrides {
            harness: Some(Harness::ClaudeCode),
            ..Default::default()
        };
        assert_eq!(
            ProfileOverrides::from_toml(&overrides.to_toml()).unwrap(),
            overrides
        );
    }
}
