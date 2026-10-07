use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    Codex,
    ClaudeCode,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectorScope {
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
pub struct DirectorProfile {
    pub harness: Harness,
    pub scope: DirectorScope,
    pub responsibilities: Vec<Task>,
    pub completion: Vec<Task>,
    /// Zero pauses delegation. The upper bound prevents accidental unbounded fan-out.
    pub max_workers: u16,
    pub permissions: BTreeMap<Task, Permission>,
}

impl Default for DirectorProfile {
    fn default() -> Self {
        Self::from_toml(include_str!("../../../profiles/default.toml"))
            .expect("bundled default profile is valid")
    }
}

impl DirectorProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_workers > 64 {
            return Err("Worker limit must be between 0 and 64".into());
        }
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
        if let DirectorScope::Issues { issue_ids } = &self.scope {
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

/// Missing fields stay inherited. Setting a field replaces that field as a whole.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<Harness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<DirectorScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibilities: Option<Vec<Task>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<Vec<Task>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_workers: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<BTreeMap<Task, Permission>>,
}

impl ProfileOverrides {
    pub fn resolve(&self, base: &DirectorProfile) -> DirectorProfile {
        DirectorProfile {
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
    fn inherited_fields_follow_defaults_and_explicit_fields_stay_fixed() {
        let mut defaults = DirectorProfile::default();
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
    fn toml_roundtrips_and_rejects_typos_or_invalid_limits() {
        let profile = DirectorProfile::default();
        assert_eq!(
            DirectorProfile::from_toml(&profile.to_toml()).unwrap(),
            profile
        );
        assert!(DirectorProfile::from_toml(&(profile.to_toml() + "\n[unknown]\nx = 1\n")).is_err());
        assert!(
            DirectorProfile::from_toml(
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
