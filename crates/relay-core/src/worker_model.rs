//! Which harness, model and effort a delegator's workers may use. Relay
//! enforces the policy in `spawn_agent` and states it in director guidance.
use crate::Harness;
use serde::{Deserialize, Serialize};

/// Effort names Relay can order, lowest first. Each harness offers a subset.
pub const EFFORT_LEVELS: [&str; 8] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

/// Position of a known effort name in [`EFFORT_LEVELS`].
pub fn effort_rank(effort: &str) -> Option<usize> {
    EFFORT_LEVELS.iter().position(|level| *level == effort)
}

/// A spawn request's optional harness, model and effort.
pub type SpawnRequest = (Option<Harness>, Option<String>, Option<String>);

/// Longest selection rationale a spawn request may give, in characters as
/// JSON Schema `maxLength` counts them.
pub const MAX_RATIONALE_LEN: usize = 1000;

/// One model of one harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChoice {
    pub harness: Harness,
    pub model: String,
}

impl ModelChoice {
    pub fn label(&self) -> String {
        format!("{} {}", self.harness.label(), self.model)
    }
}

/// The Worker Model setting of a delegator's profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerModelPolicy {
    /// Every worker uses exactly this harness, model and effort. No effort
    /// means the model's default effort, and spawn calls may not set one.
    Fixed {
        harness: Harness,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
    },
    /// The delegator picks per task within the optional allowlist and cap.
    DelegatorChooses {
        /// Empty allows any model of any connected harness.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_models: Vec<ModelChoice>,
        /// A name from [`EFFORT_LEVELS`]; none leaves effort uncapped.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_effort: Option<String>,
        /// Match model strength and effort to task difficulty.
        #[serde(default = "enabled")]
        model_capability: bool,
        /// Prefer harnesses and models with more remaining usage quota.
        #[serde(default)]
        available_usage: bool,
    },
}

fn enabled() -> bool {
    true
}

impl Default for WorkerModelPolicy {
    /// Available usage starts off: usage windows are per harness, absent until
    /// a harness reports them, and trading model strength for quota is a
    /// choice the user should opt into.
    fn default() -> Self {
        Self::DelegatorChooses {
            allowed_models: vec![],
            max_effort: None,
            model_capability: true,
            available_usage: false,
        }
    }
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && !model.eq_ignore_ascii_case("default")
        && model.len() <= 256
        && model.trim() == model
        && !model.chars().any(char::is_control)
}

fn valid_effort(effort: &str) -> bool {
    !effort.is_empty()
        && effort.len() <= 64
        && effort.trim() == effort
        && !effort.chars().any(char::is_control)
}

impl WorkerModelPolicy {
    pub fn is_fixed(&self) -> bool {
        matches!(self, Self::Fixed { .. })
    }

    /// Whether the delegator should weigh remaining usage.
    pub fn considers_usage(&self) -> bool {
        matches!(
            self,
            Self::DelegatorChooses {
                available_usage: true,
                ..
            }
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Fixed { model, effort, .. } => {
                if !valid_model(model) {
                    return Err("Choose a valid fixed worker model".into());
                }
                if effort.as_deref().is_some_and(|e| !valid_effort(e)) {
                    return Err("Choose a valid fixed worker effort".into());
                }
            }
            Self::DelegatorChooses {
                allowed_models,
                max_effort,
                ..
            } => {
                if allowed_models.len() > 64 {
                    return Err("Allow at most 64 worker models".into());
                }
                for (index, choice) in allowed_models.iter().enumerate() {
                    if !valid_model(&choice.model) {
                        return Err("Choose valid allowed worker models".into());
                    }
                    if allowed_models[..index].contains(choice) {
                        return Err("Allowed worker models must not repeat".into());
                    }
                }
                if max_effort
                    .as_deref()
                    .is_some_and(|e| effort_rank(e).is_none())
                {
                    return Err(format!(
                        "Maximum worker effort must be one of: {}",
                        EFFORT_LEVELS.join(", ")
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether `harness` may run `model` at all, ignoring effort.
    pub fn allows_model(&self, harness: Harness, model: &str) -> bool {
        match self {
            Self::Fixed {
                harness: fixed,
                model: fixed_model,
                ..
            } => *fixed == harness && fixed_model == model,
            Self::DelegatorChooses { allowed_models, .. } => {
                allowed_models.is_empty()
                    || allowed_models
                        .iter()
                        .any(|c| c.harness == harness && c.model == model)
            }
        }
    }

    /// Harnesses this policy names, or every harness when it names none.
    pub fn harnesses(&self) -> Vec<Harness> {
        match self {
            Self::Fixed { harness, .. } => vec![*harness],
            Self::DelegatorChooses { allowed_models, .. } if allowed_models.is_empty() => {
                Harness::ALL.to_vec()
            }
            Self::DelegatorChooses { allowed_models, .. } => Harness::ALL
                .into_iter()
                .filter(|h| allowed_models.iter().any(|c| c.harness == *h))
                .collect(),
        }
    }

    /// Named models for one harness; empty when any model is allowed.
    pub fn models_for(&self, harness: Harness) -> Vec<&str> {
        match self {
            Self::Fixed {
                harness: fixed,
                model,
                ..
            } => {
                if *fixed == harness {
                    vec![model.as_str()]
                } else {
                    vec![]
                }
            }
            Self::DelegatorChooses { allowed_models, .. } => allowed_models
                .iter()
                .filter(|c| c.harness == harness)
                .map(|c| c.model.as_str())
                .collect(),
        }
    }

    /// Effort names spawn_agent may pass: the fixed one, those at or below
    /// the cap, or none for an uncapped policy (any effort the model offers).
    pub fn efforts(&self) -> Option<Vec<&str>> {
        match self {
            Self::Fixed { effort, .. } => Some(effort.as_deref().into_iter().collect()),
            Self::DelegatorChooses {
                max_effort: Some(cap),
                ..
            } => {
                let cap = effort_rank(cap)?;
                Some(EFFORT_LEVELS[..=cap].to_vec())
            }
            Self::DelegatorChooses { .. } => None,
        }
    }

    /// A one-sentence statement of what workers may use, for errors and tools.
    pub fn allowed_summary(&self) -> String {
        match self {
            Self::Fixed {
                harness,
                model,
                effort,
            } => format!(
                "Workers must use {} {model} at {}.",
                harness.label(),
                effort
                    .as_deref()
                    .map_or("the model's default effort".to_string(), |e| format!(
                        "{e} effort"
                    ))
            ),
            Self::DelegatorChooses {
                allowed_models,
                max_effort,
                ..
            } => {
                let models = if allowed_models.is_empty() {
                    "Allowed models: any model of a connected harness".to_string()
                } else {
                    format!(
                        "Allowed models: {}",
                        allowed_models
                            .iter()
                            .map(ModelChoice::label)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let effort = max_effort
                    .as_deref()
                    .map_or("any effort".to_string(), |cap| {
                        format!("effort at most {cap}")
                    });
                format!("{models}; {effort}.")
            }
        }
    }

    /// Fill a fixed policy's values into a spawn request, rejecting any value
    /// that differs. Other policies pass the request through.
    pub fn resolve_request(
        &self,
        harness: Option<Harness>,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<SpawnRequest, String> {
        let Self::Fixed {
            harness: fixed_harness,
            model: fixed_model,
            effort: fixed_effort,
        } = self
        else {
            return Ok((harness, model, effort));
        };
        let mut wrong = vec![];
        if harness.is_some_and(|h| h != *fixed_harness) {
            wrong.push("harness");
        }
        if model.as_ref().is_some_and(|m| m != fixed_model) {
            wrong.push("model");
        }
        if effort.is_some() && effort != *fixed_effort {
            wrong.push("effort");
        }
        if !wrong.is_empty() {
            return Err(format!(
                "This profile fixes the worker model; the requested {} differs. {} Omit harness, model and effort or pass exactly these values.",
                wrong.join(", "),
                self.allowed_summary()
            ));
        }
        Ok((
            Some(*fixed_harness),
            Some(fixed_model.clone()),
            fixed_effort.clone(),
        ))
    }

    /// What a Delegator Chooses rationale should address, for tools and errors.
    pub fn rationale_hint(&self) -> String {
        let mut points = vec!["why the harness, model and effort fit the task"];
        if let Self::DelegatorChooses {
            model_capability,
            available_usage,
            ..
        } = self
        {
            if *model_capability {
                points.push("the capability the task needs");
            }
            if *available_usage {
                points.push("remaining usage");
            }
        }
        points.join(", ")
    }

    /// Under Delegator Chooses, a spawn request must name its harness, model
    /// and effort and give a rationale; effort is omitted only for a model
    /// whose discovered catalog offers none (`offers_effort` is `Some(false)`;
    /// `None` when the catalog is unknown). Fixed requests are already filled.
    pub fn check_selection(
        &self,
        (harness, model, effort): &SpawnRequest,
        rationale: Option<&str>,
        offers_effort: Option<bool>,
    ) -> Result<(), String> {
        if rationale.is_some_and(|r| r.chars().count() > MAX_RATIONALE_LEN) {
            return Err(format!(
                "Keep the selection rationale within {MAX_RATIONALE_LEN} characters"
            ));
        }
        if self.is_fixed() {
            return Ok(());
        }
        let mut missing = vec![];
        if harness.is_none() {
            missing.push("harness");
        }
        if model.is_none() {
            missing.push("model");
        }
        if effort.is_none() && offers_effort != Some(false) {
            missing.push("effort");
        }
        if rationale.is_none_or(|r| r.trim().is_empty()) {
            missing.push("rationale");
        }
        if missing.is_empty() {
            return Ok(());
        }
        Err(format!(
            "This profile has you choose each worker's model; pass {} explicitly. {} Effort may be omitted only for a model that offers no effort setting. The rationale briefly covers {}.",
            missing.join(", "),
            self.allowed_summary(),
            self.rationale_hint()
        ))
    }

    /// Check a worker's resolved choice. `default_effort` is the model's
    /// default when known; it applies when no effort was requested. With
    /// neither, the model has no effort setting ([`Self::check_selection`]
    /// requires an effort for any other model), so a cap does not restrict it.
    pub fn permits(
        &self,
        harness: Harness,
        model: &str,
        effort: Option<&str>,
        default_effort: Option<&str>,
    ) -> Result<(), String> {
        if !self.allows_model(harness, model) {
            return Err(format!(
                "{} {model} is not allowed for workers. {}",
                harness.label(),
                self.allowed_summary()
            ));
        }
        match self {
            Self::Fixed {
                effort: fixed_effort,
                ..
            } => {
                if effort != fixed_effort.as_deref() {
                    return Err(format!(
                        "The requested effort differs from the fixed worker model. {}",
                        self.allowed_summary()
                    ));
                }
            }
            Self::DelegatorChooses {
                max_effort: Some(cap),
                ..
            } => {
                let cap_rank = effort_rank(cap).unwrap_or(0);
                let allowed = EFFORT_LEVELS[..=cap_rank].join(", ");
                match effort.or(default_effort) {
                    None => {}
                    Some(chosen) if effort_rank(chosen).is_none_or(|r| r > cap_rank) => {
                        let source = if effort.is_some() {
                            "requested"
                        } else {
                            "model's default"
                        };
                        return Err(format!(
                            "The {source} effort {chosen} exceeds the worker cap of {cap}; pass one of: {allowed}. {}",
                            self.allowed_summary()
                        ));
                    }
                    Some(_) => {}
                }
            }
            Self::DelegatorChooses { .. } => {}
        }
        Ok(())
    }

    /// A child's policy may keep or tighten its parent's, never widen it.
    pub fn narrows(&self, parent: &Self) -> Result<(), String> {
        match parent {
            Self::Fixed { .. } => {
                if self != parent {
                    return Err(format!(
                        "A child cannot change its parent's fixed worker model. {}",
                        parent.allowed_summary()
                    ));
                }
            }
            Self::DelegatorChooses {
                allowed_models: parent_models,
                max_effort: parent_cap,
                ..
            } => {
                let within_cap = |effort: Option<&str>| match parent_cap {
                    None => true,
                    Some(cap) => effort
                        .and_then(effort_rank)
                        .is_some_and(|rank| effort_rank(cap).is_some_and(|cap| rank <= cap)),
                };
                let widen = |what: &str| {
                    Err(format!(
                        "A child cannot widen its parent's worker {what}. {}",
                        parent.allowed_summary()
                    ))
                };
                match self {
                    Self::Fixed {
                        harness,
                        model,
                        effort,
                    } => {
                        if !parent.allows_model(*harness, model) {
                            return widen("models");
                        }
                        if !within_cap(effort.as_deref()) {
                            return widen("effort cap");
                        }
                    }
                    Self::DelegatorChooses {
                        allowed_models,
                        max_effort,
                        ..
                    } => {
                        if !parent_models.is_empty()
                            && (allowed_models.is_empty()
                                || allowed_models.iter().any(|c| !parent_models.contains(c)))
                        {
                            return widen("models");
                        }
                        if !within_cap(max_effort.as_deref()) {
                            return widen("effort cap");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(harness: Harness, model: &str) -> ModelChoice {
        ModelChoice {
            harness,
            model: model.into(),
        }
    }

    fn chooses(models: Vec<ModelChoice>, cap: Option<&str>) -> WorkerModelPolicy {
        WorkerModelPolicy::DelegatorChooses {
            allowed_models: models,
            max_effort: cap.map(str::to_owned),
            model_capability: true,
            available_usage: false,
        }
    }

    fn fixed(effort: Option<&str>) -> WorkerModelPolicy {
        WorkerModelPolicy::Fixed {
            harness: Harness::ClaudeCode,
            model: "claude-sonnet".into(),
            effort: effort.map(str::to_owned),
        }
    }

    #[test]
    fn default_is_open_capability_driven_choice() {
        let policy = WorkerModelPolicy::default();
        assert_eq!(policy, chooses(vec![], None));
        assert!(!policy.considers_usage());
        assert!(policy.validate().is_ok());
        assert_eq!(policy.harnesses(), Harness::ALL.to_vec());
        assert_eq!(policy.efforts(), None);
        assert!(
            policy
                .permits(Harness::Codex, "anything", Some("ultra"), None)
                .is_ok()
        );
        assert_eq!(
            policy.resolve_request(None, Some("m".into()), None),
            Ok((None, Some("m".into()), None))
        );
    }

    #[test]
    fn validation_rejects_malformed_policies() {
        for invalid in [
            WorkerModelPolicy::Fixed {
                harness: Harness::Codex,
                model: "default".into(),
                effort: None,
            },
            WorkerModelPolicy::Fixed {
                harness: Harness::Codex,
                model: "m".into(),
                effort: Some(String::new()),
            },
            chooses(vec![], Some("extreme")),
            chooses(
                vec![choice(Harness::Codex, "m"), choice(Harness::Codex, "m")],
                None,
            ),
            chooses(vec![choice(Harness::Codex, " m")], None),
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
        assert!(fixed(Some("high")).validate().is_ok());
        assert!(
            chooses(vec![choice(Harness::Codex, "m")], Some("high"))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn fixed_mode_fills_requests_and_rejects_mismatches() {
        let policy = fixed(Some("high"));
        assert_eq!(
            policy.resolve_request(None, None, None),
            Ok((
                Some(Harness::ClaudeCode),
                Some("claude-sonnet".into()),
                Some("high".into())
            ))
        );
        assert!(
            policy
                .resolve_request(
                    Some(Harness::ClaudeCode),
                    Some("claude-sonnet".into()),
                    Some("high".into())
                )
                .is_ok()
        );
        let error = policy
            .resolve_request(Some(Harness::Codex), None, Some("low".into()))
            .unwrap_err();
        assert!(error.contains("harness, effort differs"), "{error}");
        assert!(error.contains("Claude Code claude-sonnet at high effort"));
        assert!(
            policy
                .resolve_request(None, Some("other".into()), None)
                .is_err()
        );
        // Without a fixed effort, workers use the model default.
        assert!(
            fixed(None)
                .resolve_request(None, None, Some("low".into()))
                .is_err()
        );
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-sonnet", Some("high"), None)
                .is_ok()
        );
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-sonnet", None, Some("high"))
                .is_err()
        );
    }

    #[test]
    fn allowlist_and_effort_cap_are_enforced_with_allowed_choices() {
        let policy = chooses(
            vec![
                choice(Harness::Codex, "gpt-fast"),
                choice(Harness::ClaudeCode, "claude-opus"),
            ],
            Some("high"),
        );
        let error = policy
            .permits(Harness::Codex, "claude-opus", Some("low"), None)
            .unwrap_err();
        assert!(
            error.contains("Codex claude-opus is not allowed"),
            "{error}"
        );
        assert!(error.contains("Codex gpt-fast, Claude Code claude-opus"));
        assert!(error.contains("effort at most high"));
        let error = policy
            .permits(Harness::ClaudeCode, "claude-opus", Some("max"), None)
            .unwrap_err();
        assert!(error.contains("requested effort max exceeds"), "{error}");
        assert!(error.contains("none, minimal, low, medium, high"));
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-opus", None, Some("xhigh"))
                .unwrap_err()
                .contains("model's default effort xhigh")
        );
        // A model with no effort setting has nothing for the cap to restrict.
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-opus", None, None)
                .is_ok()
        );
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-opus", Some("custom"), None)
                .is_err()
        );
        assert!(
            policy
                .permits(Harness::ClaudeCode, "claude-opus", None, Some("medium"))
                .is_ok()
        );
        assert_eq!(
            policy.harnesses(),
            vec![Harness::Codex, Harness::ClaudeCode]
        );
        assert_eq!(policy.models_for(Harness::Codex), vec!["gpt-fast"]);
        assert_eq!(
            policy.efforts().unwrap(),
            vec!["none", "minimal", "low", "medium", "high"]
        );
    }

    #[test]
    fn delegator_chooses_requires_an_explicit_explained_selection() {
        let policy = WorkerModelPolicy::DelegatorChooses {
            allowed_models: vec![],
            max_effort: None,
            model_capability: true,
            available_usage: true,
        };
        let full: SpawnRequest = (
            Some(Harness::Codex),
            Some("gpt".into()),
            Some("high".into()),
        );
        let why = Some("Cross-crate change; strong model with ample quota");
        assert!(policy.check_selection(&full, why, None).is_ok());

        let error = policy
            .check_selection(&(None, None, None), None, None)
            .unwrap_err();
        assert!(
            error.contains("pass harness, model, effort, rationale explicitly"),
            "{error}"
        );
        assert!(
            error.contains("any model of a connected harness"),
            "{error}"
        );
        assert!(
            error.contains("the capability the task needs, remaining usage"),
            "{error}"
        );
        let error = policy.check_selection(&full, Some("  "), None).unwrap_err();
        assert!(error.contains("pass rationale explicitly"), "{error}");
        let long = "x".repeat(MAX_RATIONALE_LEN + 1);
        assert!(
            policy
                .check_selection(&full, Some(long.as_str()), None)
                .unwrap_err()
                .contains("within 1000 characters")
        );
        // The limit counts characters, as the schema's maxLength does.
        let accented = "é".repeat(MAX_RATIONALE_LEN);
        assert!(
            policy
                .check_selection(&full, Some(accented.as_str()), None)
                .is_ok()
        );

        // Effort is required unless the model is known to offer none.
        let no_effort: SpawnRequest = (Some(Harness::Codex), Some("gpt".into()), None);
        for offers in [None, Some(true)] {
            assert!(
                policy
                    .check_selection(&no_effort, why, offers)
                    .unwrap_err()
                    .contains("pass effort explicitly")
            );
        }
        assert!(policy.check_selection(&no_effort, why, Some(false)).is_ok());

        // Without considerations the rationale need only cover task fit.
        let plain = WorkerModelPolicy::DelegatorChooses {
            allowed_models: vec![],
            max_effort: None,
            model_capability: false,
            available_usage: false,
        };
        assert_eq!(
            plain.rationale_hint(),
            "why the harness, model and effort fit the task"
        );

        // Fixed policies fill the request and need no rationale.
        let pinned = fixed(None);
        let filled = pinned.resolve_request(None, None, None).unwrap();
        assert!(pinned.check_selection(&filled, None, None).is_ok());
        assert!(
            pinned
                .check_selection(&filled, Some(long.as_str()), None)
                .is_err()
        );
    }

    #[test]
    fn children_may_only_narrow_their_parent() {
        let open = WorkerModelPolicy::default();
        let listed = chooses(
            vec![
                choice(Harness::Codex, "a"),
                choice(Harness::ClaudeCode, "b"),
            ],
            Some("high"),
        );
        assert!(listed.narrows(&open).is_ok());
        assert!(open.narrows(&listed).is_err());
        assert!(
            chooses(vec![choice(Harness::Codex, "a")], Some("low"))
                .narrows(&listed)
                .is_ok()
        );
        assert!(
            chooses(vec![choice(Harness::Codex, "c")], Some("low"))
                .narrows(&listed)
                .unwrap_err()
                .contains("widen its parent's worker models")
        );
        assert!(
            chooses(vec![choice(Harness::Codex, "a")], None)
                .narrows(&listed)
                .unwrap_err()
                .contains("effort cap")
        );
        assert!(
            chooses(vec![choice(Harness::Codex, "a")], Some("max"))
                .narrows(&listed)
                .is_err()
        );
        let pinned = WorkerModelPolicy::Fixed {
            harness: Harness::ClaudeCode,
            model: "b".into(),
            effort: Some("medium".into()),
        };
        assert!(pinned.narrows(&listed).is_ok());
        assert!(
            WorkerModelPolicy::Fixed {
                harness: Harness::ClaudeCode,
                model: "b".into(),
                effort: None,
            }
            .narrows(&listed)
            .is_err()
        );
        assert!(pinned.narrows(&pinned).is_ok());
        assert!(listed.narrows(&pinned).is_err());
        assert!(fixed(Some("low")).narrows(&pinned).is_err());
        // Considerations are guidance, not limits.
        let usage = WorkerModelPolicy::DelegatorChooses {
            allowed_models: vec![],
            max_effort: None,
            model_capability: false,
            available_usage: true,
        };
        assert!(usage.narrows(&open).is_ok());
    }

    #[test]
    fn policies_round_trip_through_json_and_toml() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Wrapper {
            worker_models: WorkerModelPolicy,
        }
        for policy in [
            WorkerModelPolicy::default(),
            fixed(Some("high")),
            fixed(None),
            chooses(vec![choice(Harness::Codex, "a")], Some("medium")),
        ] {
            let wrapper = Wrapper {
                worker_models: policy,
            };
            let toml = toml::to_string_pretty(&wrapper).unwrap();
            assert_eq!(toml::from_str::<Wrapper>(&toml).unwrap(), wrapper, "{toml}");
            let json = serde_json::to_string(&wrapper).unwrap();
            assert_eq!(serde_json::from_str::<Wrapper>(&json).unwrap(), wrapper);
        }
        let minimal: Wrapper =
            toml::from_str("[worker_models]\nmode = \"delegator_chooses\"\n").unwrap();
        assert_eq!(minimal.worker_models, WorkerModelPolicy::default());
        assert!(
            toml::from_str::<Wrapper>(
                "[worker_models]\nmode = \"fixed\"\nharness = \"codex\"\nmodel = \"m\"\ntypo = 1\n"
            )
            .is_err()
        );
    }
}
