//! Harness, model and effort identity.

use relay_core::{HarnessCatalog, HarnessModel, HarnessSelection, Session};
use relay_ui::effort::{EFFORT_NAMES, TIER_NAMES};
use relay_ui::prelude::{Effort, Harness, ModelHue, ModelIdentity};
use std::collections::BTreeMap;

pub fn harness(harness: relay_core::Harness) -> Harness {
    match harness {
        relay_core::Harness::Codex => Harness::Codex,
        relay_core::Harness::ClaudeCode => Harness::ClaudeCode,
    }
}

/// The display identity of a reported model id: Claude Opus, Sonnet and
/// Haiku and named GPT families take version-independent hues, and
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
    let named_hue = ["astra", "luna", "terra", "sol"]
        .into_iter()
        .find_map(|name| {
            family_of(name).map(|index| {
                (
                    name,
                    index,
                    match name {
                        "astra" => ModelHue::Astra,
                        "luna" => ModelHue::Luna,
                        "terra" => ModelHue::Terra,
                        _ => ModelHue::Sol,
                    },
                )
            })
        });
    let claude = ["opus", "sonnet", "haiku", "fable"]
        .into_iter()
        .find_map(|name| family_of(name).map(|index| (name, index)));
    let (family, vendor, hue, skip) = if let Some((name, index)) = claude {
        let hue = match name {
            "opus" => ModelHue::Opus,
            "sonnet" => ModelHue::Sonnet,
            "haiku" => ModelHue::Haiku,
            _ => ModelHue::Neutral,
        };
        (capitalize(name), "CLAUDE", hue, vec![index])
    } else if words.first().is_some_and(|word| word.starts_with("gpt")) {
        // "gpt5" spells its version inside the family word.
        let first = words[0];
        let rest = first.trim_start_matches("gpt");
        let rest = std::iter::once(rest)
            .filter(|rest| !rest.is_empty())
            .chain(words[1..].iter().copied())
            .collect::<Vec<_>>();
        let variants = variant_name(&rest);
        let family = if variants.is_empty() {
            "GPT".into()
        } else {
            variants
        };
        return ModelIdentity::new(
            family,
            number(&rest),
            named_hue.map_or(ModelHue::Sol, |(_, _, hue)| hue),
        )
        .vendor("GPT");
    } else if let Some((name, index, hue)) = named_hue {
        (capitalize(name), "GPT", hue, vec![index])
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
    let variants = variant_name(&rest);
    let family = if variants.is_empty() {
        family
    } else {
        format!("{family} {variants}")
    };
    ModelIdentity::new(family, number(&rest), hue).vendor(vendor)
}

fn variant_name(words: &[&str]) -> String {
    words
        .iter()
        .filter(|word| !word.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map(|word| capitalize(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The version a family is shown with: its numbers, "4", "5" → "4.5" and
/// "5.1" → "5.1". Variant words are kept in the display family.
fn number(words: &[&str]) -> String {
    words
        .iter()
        .copied()
        .filter(|word| !word.is_empty() && word.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .collect::<Vec<_>>()
        .join(".")
}

/// How a model's offered efforts map onto the track, in catalog order:
/// ordinary steps (a consecutive run of [`EFFORT_NAMES`] starting at
/// `first`), then top steps (a prefix of [`TIER_NAMES`]: max, ultra) drawn
/// as diamonds. `None` when the names do not fit that scale, so the track
/// would misname a step.
struct Scale {
    first: u8,
    levels: u8,
    tiers: u8,
}

fn scale(model: &HarnessModel) -> Option<Scale> {
    let names: Vec<&str> = model.efforts.iter().map(String::as_str).collect();
    let levels = names
        .iter()
        .take_while(|name| EFFORT_NAMES.contains(name))
        .count();
    let (ordinary, top) = names.split_at(levels);
    let first = match ordinary.first() {
        Some(name) => EFFORT_NAMES.iter().position(|known| known == name)?,
        None => 0,
    };
    if EFFORT_NAMES.get(first..first + levels) != Some(ordinary)
        || TIER_NAMES.get(..top.len()) != Some(top)
        || names.is_empty()
    {
        return None;
    }
    Some(Scale {
        first: first as u8,
        levels: levels as u8,
        tiers: top.len() as u8,
    })
}

/// The effort track of `selection` on a catalog model: one growing step per
/// ordinary effort, then a diamond per top effort (max, ultra) the model
/// offers. Shows the selected effort, or the model's default. Hidden when
/// the model offers no efforts, none is selected or defaulted, or the
/// offered names do not fit the track's scale.
pub fn effort(selection: &HarnessSelection, model: &HarnessModel) -> Option<Effort> {
    let scale = scale(model)?;
    let current = selection
        .effort
        .as_deref()
        .or(model.default_effort.as_deref())?;
    let step = model.efforts.iter().position(|name| name == current)? as u8;
    Some(
        Effort::new(scale.levels, 0)
            .starting_at(scale.first)
            .with_tiers(scale.tiers, step),
    )
}

/// The effort name a track step selects on `model`: steps follow the
/// catalog's order.
pub fn effort_choice(model: &HarnessModel, step: u8) -> Option<String> {
    scale(model)?;
    model.efforts.get(usize::from(step)).cloned()
}

/// The selected model, or the reported model when no selection is set, with its
/// effort when the session's catalog describes that model.
pub fn session_model(
    session: &Session,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> (Option<ModelIdentity>, Option<Effort>) {
    selection_model(
        &session.selection,
        session.worker.as_ref().and_then(|w| w.model.as_deref()),
        &session.id,
        session.worker.as_ref().map(|w| w.harness),
        catalogs,
    )
}

pub fn selection_model(
    selection: &HarnessSelection,
    reported: Option<&str>,
    session_id: &str,
    harness: Option<relay_core::Harness>,
    catalogs: &BTreeMap<String, Result<HarnessCatalog, String>>,
) -> (Option<ModelIdentity>, Option<Effort>) {
    let effective = selection.model.as_deref().or(reported);
    // Without a selected or reported model, the harness runs its default.
    let find = |catalog: &HarnessCatalog| catalog.model(effective).cloned();
    // The session's own catalog, else any loaded catalog of the same harness
    // that lists the model: a model's effort steps do not depend on the
    // session.
    let catalog_model = catalogs
        .get(session_id)
        .and_then(|catalog| catalog.as_ref().ok())
        .and_then(find)
        .or_else(|| {
            catalogs
                .values()
                .filter_map(|catalog| catalog.as_ref().ok())
                .filter(|catalog| harness.is_none_or(|h| catalog.harness == h))
                .find_map(find)
        });
    let identity = effective
        .or(catalog_model.as_ref().map(|model| model.id.as_str()))
        .map(|id| {
            let mut identity = model_identity(id);
            // Moving Claude aliases (e.g. "opus") omit the version from their ID,
            // but their catalogue display names carry it. An explicit version wins.
            if identity.version.is_empty() {
                let candidates = catalog_model
                    .as_ref()
                    .map(|model| model.name.as_str())
                    .into_iter()
                    .chain(reported);
                for candidate in candidates {
                    let resolved = model_identity(candidate);
                    if resolved.family == identity.family
                        && resolved.vendor == identity.vendor
                        && !resolved.version.is_empty()
                    {
                        identity.version = resolved.version;
                        break;
                    }
                }
            }
            identity
        });
    let effort = catalog_model.and_then(|model| effort(selection, &model));
    (identity, effort)
}

/// The track of a stored effort whose model no loaded catalog describes,
/// for surfaces that show effort only as a track (tree rows, spawn cards):
/// the low…xhigh and max scale both harnesses share, reaching down to
/// minimal or up to ultra when the effort names them.
pub fn stored_effort(selection: &HarnessSelection) -> Option<Effort> {
    let name = selection.effort.as_deref()?;
    let mut efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"]
        .map(String::from)
        .to_vec();
    match name {
        "minimal" => efforts.insert(0, name.into()),
        "ultra" => efforts.push(name.into()),
        _ => {}
    }
    effort(
        selection,
        &HarnessModel {
            efforts,
            ..Default::default()
        },
    )
}
