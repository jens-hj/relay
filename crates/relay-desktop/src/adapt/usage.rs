//! Harness rate-limit windows and the weekly usage ledger.

use super::identity::{harness, model_identity};
use relay_core::{Harness, Snapshot, UsageLimit, UsageWindow as Window};
use relay_ui::usage::{HarnessUsage, ModelShare, UsageInsight, UsageWindow};
use std::collections::BTreeMap;
use std::time::Duration;

const WEEK: u64 = 7 * 24 * 60 * 60;

/// "3h 05m", "2d 4h" or "12m".
fn remaining(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

/// One reported window as of `now`. An observation from a window that has
/// since reset no longer describes current usage, so it is dropped.
fn window(limit: &UsageLimit, now: u64) -> Option<UsageWindow> {
    let length = u64::from(limit.window_minutes) * 60;
    if length == 0 {
        return None;
    }
    let mut window = UsageWindow::new(limit.used_percent, Duration::from_secs(length));
    if let Some(resets) = limit.resets_at {
        if resets <= now {
            return None;
        }
        let left = resets - now;
        window = window
            .elapsed(Duration::from_secs(length.saturating_sub(left)))
            .resets(format!("resets in {}", remaining(left)));
    }
    Some(window)
}

fn tokens(count: u64) -> String {
    relay_ui::context::tokens(count)
}

/// Model mix, heaviest agents and run counts from the ledger's last week.
fn insight(snapshot: &Snapshot, which: Harness, now: u64) -> UsageInsight {
    let entries: Vec<_> = snapshot
        .usage_ledger
        .iter()
        .filter(|e| e.harness == which && e.completed_at + WEEK >= now)
        .collect();
    let spent = |e: &&relay_core::UsageEntry| e.input_tokens + e.output_tokens;
    let total: u64 = entries.iter().map(spent).sum();
    if total == 0 {
        return UsageInsight::default();
    }
    let share = |part: u64| (part as f64 * 100.0 / total as f64) as f32;
    let mut models: BTreeMap<String, u64> = BTreeMap::new();
    let mut sessions: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in &entries {
        if let Some(model) = &entry.model {
            *models.entry(model.clone()).or_default() += spent(entry);
        }
        *sessions.entry(entry.session_id.as_str()).or_default() += spent(entry);
    }
    let mut model_mix: Vec<ModelShare> = models
        .into_iter()
        .map(|(id, used)| {
            let identity = model_identity(&id);
            ModelShare {
                name: identity.short_name(),
                hue: identity.hue,
                share: share(used),
            }
        })
        .collect();
    model_mix.sort_by(|a, b| b.share.total_cmp(&a.share));
    let mut heaviest: Vec<(String, f32)> = sessions
        .into_iter()
        .map(|(id, used)| {
            let name = snapshot
                .sessions
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| id.to_string());
            (name, share(used))
        })
        .collect();
    heaviest.sort_by(|a, b| b.1.total_cmp(&a.1));
    heaviest.truncate(3);
    let cached: u64 = entries.iter().map(|e| e.cached_input_tokens).sum();
    UsageInsight {
        model_mix,
        heaviest,
        stats: vec![
            ("runs".into(), entries.len().to_string()),
            ("tokens".into(), tokens(total)),
            ("cached".into(), tokens(cached)),
        ],
        suggestion: None,
    }
}

/// Usage of every harness with a reported limit window, in harness order.
/// `now` is Unix seconds.
pub fn harness_usage(snapshot: &Snapshot, now: u64) -> Vec<HarnessUsage> {
    [Harness::Codex, Harness::ClaudeCode]
        .into_iter()
        .filter_map(|which| {
            let find = |kind: Window| {
                snapshot
                    .usage_limits
                    .iter()
                    .find(|l| l.harness == which && l.window == kind)
                    .and_then(|l| window(l, now))
            };
            let (session, week) = (find(Window::FiveHour), find(Window::Weekly));
            if session.is_none() && week.is_none() {
                return None;
            }
            Some(HarnessUsage {
                harness: harness(which),
                session,
                week,
                insight: insight(snapshot, which, now),
            })
        })
        .collect()
}
