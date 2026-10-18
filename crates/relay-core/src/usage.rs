//! Harness rate-limit windows and the per-run usage ledger.
use crate::{Harness, Snapshot};
use serde::{Deserialize, Serialize};

/// Ledger entries older than this many seconds are pruned on insert.
pub const USAGE_LEDGER_RETENTION_SECS: u64 = 8 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindow {
    FiveHour,
    Weekly,
}

impl UsageWindow {
    pub fn key(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::Weekly => "weekly",
        }
    }

    /// Nearest named window for a reported window length.
    pub fn from_minutes(minutes: u32) -> Self {
        if minutes >= 24 * 60 {
            Self::Weekly
        } else {
            Self::FiveHour
        }
    }
}

/// The latest harness-reported occupancy of one rate-limit window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageLimit {
    /// `{harness}-{window}`, stable for record diffing.
    pub id: String,
    pub harness: Harness,
    pub window: UsageWindow,
    /// Always finite and within 0..=100; see [`UsageLimit::new`].
    pub used_percent: f32,
    pub window_minutes: u32,
    /// Unix seconds.
    #[serde(default)]
    pub resets_at: Option<u64>,
    /// Unix seconds.
    pub observed_at: u64,
}

// `used_percent` is never NaN: it is sanitized by `new` and on insertion.
impl Eq for UsageLimit {}

impl UsageLimit {
    pub fn id_for(harness: Harness, window: UsageWindow) -> String {
        let harness = match harness {
            Harness::Codex => "codex",
            Harness::ClaudeCode => "claude_code",
        };
        format!("{harness}-{}", window.key())
    }

    pub fn new(
        harness: Harness,
        window: UsageWindow,
        used_percent: f64,
        window_minutes: u32,
        resets_at: Option<u64>,
        observed_at: u64,
    ) -> Self {
        Self {
            id: Self::id_for(harness, window),
            harness,
            window,
            used_percent: sanitize_percent(used_percent as f32),
            window_minutes,
            resets_at,
            observed_at,
        }
    }
}

fn sanitize_percent(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// Measured token usage of one completed run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    pub id: String,
    pub session_id: String,
    pub harness: Harness,
    #[serde(default)]
    pub model: Option<String>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// Unix seconds.
    pub completed_at: u64,
}

impl Snapshot {
    /// Replace the window with the same id. Returns whether anything changed.
    pub fn record_usage_limit(&mut self, mut limit: UsageLimit) -> bool {
        limit.used_percent = sanitize_percent(limit.used_percent);
        limit.id = UsageLimit::id_for(limit.harness, limit.window);
        match self.usage_limits.iter_mut().find(|l| l.id == limit.id) {
            Some(existing) if *existing == limit => false,
            Some(existing) => {
                *existing = limit;
                true
            }
            None => {
                self.usage_limits.push(limit);
                self.usage_limits.sort_by(|a, b| a.id.cmp(&b.id));
                true
            }
        }
    }

    /// Append a ledger entry and drop entries outside the retention window.
    pub fn record_usage(&mut self, entry: UsageEntry, now: u64) {
        let cutoff = now.saturating_sub(USAGE_LEDGER_RETENTION_SECS);
        self.usage_ledger.retain(|e| e.completed_at >= cutoff);
        if !self.usage_ledger.iter().any(|e| e.id == entry.id) {
            self.usage_ledger.push(entry);
        }
    }
}

/// One harness's remaining quota as of a moment, for delegators weighing
/// available usage. Built from the same windows and ledger as the usage panel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UsageRemaining {
    pub harness: Harness,
    /// Percent left in each reported window that has not reset since.
    pub five_hour_left_percent: Option<u8>,
    pub weekly_left_percent: Option<u8>,
    /// Unix seconds.
    pub five_hour_resets_at: Option<u64>,
    pub weekly_resets_at: Option<u64>,
    /// Tokens per model in the last week, heaviest first.
    pub models_this_week: Vec<ModelTokens>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelTokens {
    pub model: String,
    pub tokens: u64,
}

const WEEK_SECS: u64 = 7 * 24 * 60 * 60;

impl UsageRemaining {
    /// "Codex: 62% of the 5-hour window left (resets in 2h 05m), …".
    pub fn describe(&self, now: u64) -> String {
        let window = |name: &str, left: Option<u8>, resets: Option<u64>| match left {
            None => format!("{name} window not reported"),
            Some(left) => match resets {
                Some(at) => format!(
                    "{left}% of the {name} window left (resets in {})",
                    duration(at.saturating_sub(now))
                ),
                None => format!("{left}% of the {name} window left"),
            },
        };
        let mut text = format!(
            "{}: {}, {}",
            self.harness.label(),
            window(
                "5-hour",
                self.five_hour_left_percent,
                self.five_hour_resets_at
            ),
            window("weekly", self.weekly_left_percent, self.weekly_resets_at),
        );
        if !self.models_this_week.is_empty() {
            text.push_str("; tokens this week: ");
            text.push_str(
                &self
                    .models_this_week
                    .iter()
                    .map(|m| format!("{} {}", m.model, m.tokens))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        text
    }
}

/// "2d 4h", "3h 05m" or "12m".
fn duration(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

impl Snapshot {
    /// Remaining usage of each harness in `harnesses`, as of `now` (Unix seconds).
    pub fn usage_remaining(&self, harnesses: &[Harness], now: u64) -> Vec<UsageRemaining> {
        harnesses
            .iter()
            .map(|&harness| {
                let current = |window: UsageWindow| {
                    self.usage_limits
                        .iter()
                        .find(|l| l.harness == harness && l.window == window)
                        .filter(|l| l.resets_at.is_none_or(|at| at > now))
                };
                let left = |limit: Option<&UsageLimit>| {
                    limit.map(|l| (100.0 - l.used_percent).round().clamp(0.0, 100.0) as u8)
                };
                let (five, week) = (current(UsageWindow::FiveHour), current(UsageWindow::Weekly));
                let mut models: Vec<ModelTokens> = vec![];
                for entry in self
                    .usage_ledger
                    .iter()
                    .filter(|e| e.harness == harness && e.completed_at + WEEK_SECS >= now)
                {
                    let Some(model) = &entry.model else { continue };
                    let tokens = entry.input_tokens + entry.output_tokens;
                    match models.iter_mut().find(|m| &m.model == model) {
                        Some(existing) => existing.tokens += tokens,
                        None => models.push(ModelTokens {
                            model: model.clone(),
                            tokens,
                        }),
                    }
                }
                models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.model.cmp(&b.model)));
                UsageRemaining {
                    harness,
                    five_hour_left_percent: left(five),
                    weekly_left_percent: left(week),
                    five_hour_resets_at: five.and_then(|l| l.resets_at),
                    weekly_resets_at: week.and_then(|l| l.resets_at),
                    models_this_week: models,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_usage_reports_current_windows_and_weekly_model_tokens() {
        let mut snapshot = Snapshot::default();
        let now = 1_000_000;
        snapshot.record_usage_limit(UsageLimit::new(
            Harness::Codex,
            UsageWindow::FiveHour,
            38.0,
            300,
            Some(now + 2 * 3600 + 5 * 60),
            now - 10,
        ));
        // A window that reset since it was observed no longer describes usage.
        snapshot.record_usage_limit(UsageLimit::new(
            Harness::Codex,
            UsageWindow::Weekly,
            90.0,
            10080,
            Some(now - 1),
            now - 100,
        ));
        for (id, model, tokens, at) in [
            ("a", "gpt-big", 700, now - 5),
            ("b", "gpt-small", 100, now - 5),
            ("c", "gpt-big", 300, now - 6),
            ("d", "gpt-small", 9_000, now - WEEK_SECS - 1),
        ] {
            snapshot.record_usage(
                UsageEntry {
                    model: Some(model.into()),
                    output_tokens: tokens,
                    input_tokens: 0,
                    ..entry(id, at)
                },
                now,
            );
        }
        let remaining = snapshot.usage_remaining(&[Harness::Codex, Harness::ClaudeCode], now);
        assert_eq!(remaining[0].five_hour_left_percent, Some(62));
        assert_eq!(remaining[0].weekly_left_percent, None);
        assert_eq!(
            remaining[0].models_this_week,
            vec![
                ModelTokens {
                    model: "gpt-big".into(),
                    tokens: 1000
                },
                ModelTokens {
                    model: "gpt-small".into(),
                    tokens: 100
                },
            ]
        );
        assert_eq!(
            remaining[0].describe(now),
            "Codex: 62% of the 5-hour window left (resets in 2h 05m), weekly window not reported; tokens this week: gpt-big 1000, gpt-small 100"
        );
        assert_eq!(
            remaining[1].describe(now),
            "Claude Code: 5-hour window not reported, weekly window not reported"
        );
    }

    fn entry(id: &str, completed_at: u64) -> UsageEntry {
        UsageEntry {
            id: id.into(),
            session_id: "s".into(),
            harness: Harness::Codex,
            model: None,
            input_tokens: 1,
            cached_input_tokens: 0,
            output_tokens: 1,
            completed_at,
        }
    }

    #[test]
    fn ledger_prunes_old_entries_on_insert() {
        let mut snapshot = Snapshot::default();
        let now = 100 * 24 * 60 * 60;
        snapshot.record_usage(entry("old", now - USAGE_LEDGER_RETENTION_SECS - 1), now - 1);
        snapshot.record_usage(entry("edge", now - USAGE_LEDGER_RETENTION_SECS), now - 1);
        snapshot.record_usage(entry("new", now), now);
        snapshot.record_usage(entry("new", now), now);
        assert_eq!(
            snapshot
                .usage_ledger
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["edge", "new"]
        );
    }

    #[test]
    fn limits_upsert_by_window_and_sanitize() {
        let mut snapshot = Snapshot::default();
        assert!(snapshot.record_usage_limit(UsageLimit::new(
            Harness::Codex,
            UsageWindow::Weekly,
            f64::NAN,
            10080,
            None,
            1
        )));
        assert!(snapshot.record_usage_limit(UsageLimit::new(
            Harness::Codex,
            UsageWindow::FiveHour,
            140.0,
            300,
            Some(9),
            1
        )));
        let same = snapshot.usage_limits[0].clone();
        assert!(!snapshot.record_usage_limit(same));
        assert_eq!(snapshot.usage_limits[0].id, "codex-five_hour");
        assert_eq!(snapshot.usage_limits[0].used_percent, 100.0);
        assert_eq!(snapshot.usage_limits[1].used_percent, 0.0);
        assert_eq!(UsageWindow::from_minutes(300), UsageWindow::FiveHour);
        assert_eq!(UsageWindow::from_minutes(10080), UsageWindow::Weekly);
    }
}
