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

#[cfg(test)]
mod tests {
    use super::*;

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
