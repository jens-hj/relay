//! Harness, model and effort identity.

use relay_core::{HarnessCatalog, HarnessModel, HarnessSelection, Session};
use relay_ui::effort::EFFORT_NAMES;
use relay_ui::prelude::{Effort, Harness, ModelHue, ModelIdentity};
use std::collections::BTreeMap;

pub fn harness(harness: relay_core::Harness) -> Harness {
    match harness {
        relay_core::Harness::Codex => Harness::Codex,
        relay_core::Harness::ClaudeCode => Harness::ClaudeCode,
    }
}

/// The display identity of a reported model id: Claude Opus, Sonnet and
/// Haiku take their family hues, GPT, Codex and Sol models the Sol hue, and
/// anything else stays neutral under its own name.
pub fn model_identity(id: &str) -> ModelIdentity {
    // "sonnet[1m]" names a context variant, not a version.
    let lower = id
        .split('[')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(['-', '_', ' ', '/'])
        .filter(|word| !word.is_empty())
        // Release dates and moving aliases are not versions.
        .filter(|word| !(word.len() >= 6 && word.chars().all(|c| c.is_ascii_digit())))
        .filter(|word| *word != "latest")
        .collect();
    let family_of = |name: &str| words.iter().position(|word| *word == name);
    let claude = ["opus", "sonnet", "haiku"]
        .into_iter()
        .find_map(|name| family_of(name).map(|index| (name, index)));
    let (family, vendor, hue, skip) = if let Some((name, index)) = claude {
        let hue = match name {
            "opus" => ModelHue::Opus,
            "sonnet" => ModelHue::Sonnet,
            _ => ModelHue::Haiku,
        };
        (capitalize(name), "CLAUDE", hue, vec![index])
    } else if let Some(index) = family_of("sol") {
        ("Sol".to_string(), "GPT", ModelHue::Sol, vec![index])
    } else if words.first().is_some_and(|word| word.starts_with("gpt")) {
        // "gpt5" spells its version inside the family word.
        let first = words[0];
        let rest = first.trim_start_matches("gpt");
        let mut identity = ModelIdentity::new("GPT", "", ModelHue::Sol).vendor("GPT");
        identity.version = version(
            &std::iter::once(rest)
                .filter(|rest| !rest.is_empty())
                .chain(words[1..].iter().copied())
                .collect::<Vec<_>>(),
        );
        return identity;
    } else if let Some(index) = family_of("codex") {
        ("Codex".to_string(), "GPT", ModelHue::Sol, vec![index])
    } else {
        return ModelIdentity::new(id.trim(), "", ModelHue::Neutral);
    };
    let rest: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(index, word)| !skip.contains(index) && **word != "claude")
        .map(|(_, word)| *word)
        .collect();
    ModelIdentity::new(family, version(&rest), hue).vendor(vendor)
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// "4", "5" → "4.5"; "5.1", "codex", "max" → "5.1 codex max".
fn version(words: &[&str]) -> String {
    let numbers: Vec<&str> = words
        .iter()
        .copied()
        .filter(|word| word.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .collect();
    let others: Vec<&str> = words
        .iter()
        .copied()
        .filter(|word| !word.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .collect();
    std::iter::once(numbers.join("."))
        .filter(|numbers| !numbers.is_empty())
        .chain(others.into_iter().map(str::to_string))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The ordinary efforts a model offers and whether it offers max.
fn offered(model: &HarnessModel) -> (Vec<&str>, bool) {
    let ordinary = model
        .efforts
        .iter()
        .map(String::as_str)
        .filter(|effort| *effort != "max")
        .collect();
    (ordinary, model.efforts.iter().any(|effort| effort == "max"))
}

/// The effort track of `selection` on a catalog model: one step per offered
/// effort except max, and the max diamond only when max is offered. Hidden
/// when the model offers no efforts, no effort is selected or defaulted, or
/// the offered names are not consecutive steps the track can name.
pub fn effort(selection: &HarnessSelection, model: &HarnessModel) -> Option<Effort> {
    let (ordinary, has_max) = offered(model);
    if ordinary.is_empty() && !has_max {
        return None;
    }
    let first = match ordinary.first() {
        Some(name) => EFFORT_NAMES.iter().position(|known| known == name)?,
        None => 0,
    };
    if EFFORT_NAMES.get(first..first + ordinary.len()) != Some(&ordinary[..]) {
        return None;
    }
    let current = selection
        .effort
        .as_deref()
        .or(model.default_effort.as_deref())?;
    let levels = ordinary.len() as u8;
    let effort = if current == "max" && has_max {
        Effort::new(levels, levels.saturating_sub(1)).with_max(true)
    } else {
        let step = ordinary.iter().position(|name| *name == current)? as u8;
        let effort = Effort::new(levels, step);
        if has_max {
            effort.with_max(false)
        } else {
            effort
        }
    };
    Some(effort.starting_at(first as u8))
}

/// The effort name a track step selects on `model`.
pub fn effort_choice(model: &HarnessModel, step: u8) -> Option<String> {
    let (ordinary, has_max) = offered(model);
    let step = usize::from(step);
    if step < ordinary.len() {
        Some(ordinary[step].to_string())
    } else if step == ordinary.len() && has_max {
        Some("max".into())
    } else {
        None
    }
}

/// The model a session reports, or else the one it selected, with its
/// effort when the session's catalog describes that model.
pub fn session_model(
    session: &Session,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> (Option<ModelIdentity>, Option<Effort>) {
    let selected = session.selection.model.as_deref();
    let reported = session.worker.as_ref().and_then(|w| w.model.as_deref());
    let catalog_model = catalogs
        .get(&session.id)
        .and_then(|catalog| catalog.as_ref().ok())
        .and_then(|catalog| {
            [selected, reported]
                .into_iter()
                .flatten()
                .find_map(|id| catalog.models.iter().find(|m| m.id == id))
        });
    let identity = reported.or(selected).map(model_identity);
    let effort = catalog_model.and_then(|model| effort(&session.selection, model));
    (identity, effort)
}
