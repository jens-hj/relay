//! The opt-in tiny Claude message that starts the next five-hour usage window
//! after a reported one expires, so an idle workspace does not wait for the
//! next real turn. The setting and last outcome are shared with clients. The
//! schedule is a pure function of the snapshot and the server's attempt
//! record, so every server process and every test derives the same plan.
//!
//! Anthropic documents the five-hour and weekly limits but not exactly when a
//! window begins, so this reduces idle gaps on a best-effort basis. It adds no
//! quota and guarantees nothing about when the service starts a window. It
//! only ever acts on a reset time Claude has reported: with none known, it
//! waits for a real turn to report one.
use crate::{Harness, Snapshot, UsageLimit, UsageWindow, WorkerStatus};
use serde::{Deserialize, Serialize};

/// Seconds after a reported reset before the message is sent, so the
/// provider's clock is past the boundary.
pub const WINDOW_START_GRACE_SECS: u64 = 5;
/// Attempts per expired window that provably delivered nothing.
pub const WINDOW_START_MAX_ATTEMPTS: u32 = 3;

/// Seconds to wait after the given number of failed attempts: 5 min, then 15 min.
pub fn window_start_backoff(failed_attempts: u32) -> u64 {
    300 * 3u64.pow(failed_attempts.saturating_sub(1).min(4))
}

/// Why a window-start message did not go through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStartFailure {
    /// Claude Code is not signed in on the server.
    SignedOut,
    /// Claude Code is signed in, but not verifiably with a first-party
    /// subscription (for example an API key or another provider), so a
    /// message could be billed as API usage.
    NotSubscription,
    /// Administrator policy can run hooks, which no launch option can switch off
    /// and which Relay must not override, or Relay cannot tell that it cannot.
    ManagedHooks,
    /// The weekly limit is used up.
    WeeklyLimit,
    /// The account's model list has no canonical Haiku model.
    ModelUnavailable,
    /// Claude Code could not reach the service.
    Offline,
    TimedOut,
    /// The server stopped before the attempt finished.
    Interrupted,
    /// The installed Claude Code cannot run the message.
    Unsupported,
    Failed,
}

impl WindowStartFailure {
    pub fn label(self) -> &'static str {
        match self {
            Self::SignedOut => "Claude Code is signed out",
            Self::NotSubscription => "Claude Code is not signed in with a subscription",
            Self::ManagedHooks => "Managed settings may run hooks",
            Self::WeeklyLimit => "Weekly limit reached",
            Self::ModelUnavailable => "No canonical Haiku model available",
            Self::Offline => "Claude Code could not connect",
            Self::TimedOut => "Timed out",
            Self::Interrupted => "Interrupted",
            Self::Unsupported => "Claude Code cannot run the message",
            Self::Failed => "Claude Code reported an error",
        }
    }

    /// Whether another attempt can succeed without the owner changing something.
    /// This only matters when nothing was handed to Claude Code, or the service
    /// itself reported a rejection; after hand-off no failure kind proves that
    /// nothing was delivered.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Offline | Self::TimedOut | Self::Interrupted | Self::Failed
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WindowStartOutcome {
    /// The message went through on this model.
    Sent { model: String },
    /// Claude had real work after the window ended, so no message was needed.
    NotNeeded,
    /// Nothing was sent, or nothing provably was.
    Failed {
        reason: WindowStartFailure,
        /// Attempts made for this window so far.
        attempts: u32,
    },
    /// The message was handed to Claude Code and may have been delivered, but
    /// no answer was read. Nothing more is sent for this window until Claude
    /// reports a new one, or the owner turns the setting off and on.
    Unconfirmed { reason: WindowStartFailure },
}

/// The window-start setting and its latest status. Off unless the workspace
/// owner enables it. Only the owner's explicit choice authorizes these
/// background model calls; it is separate from any agent's Implement
/// permission, which never applies to them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStart {
    #[serde(default)]
    pub enabled: bool,
    /// Unix seconds when the setting was last turned on. Turning it off and
    /// on again starts a fresh set of attempts.
    #[serde(default)]
    pub enabled_at: Option<u64>,
    /// Unix seconds.
    #[serde(default)]
    pub last_attempt_at: Option<u64>,
    #[serde(default)]
    pub last_outcome: Option<WindowStartOutcome>,
    /// Unix seconds of the next scheduled attempt. Absent while nothing is
    /// scheduled, for example while Claude runs a turn or no window end is
    /// known.
    #[serde(default)]
    pub next_due_at: Option<u64>,
}

impl WindowStart {
    /// Omitted from snapshots until the owner touches the setting.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn set_enabled(&mut self, enabled: bool, now: u64) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.next_due_at = None;
        if enabled {
            self.enabled_at = Some(now);
        }
    }
}

/// What the server's durable attempt record holds for one window key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowClaim {
    pub attempts: u32,
    /// Sent, given up or waiting for the owner; no further attempts.
    pub settled: bool,
    /// The message was handed to Claude Code and has not been shown to be
    /// undelivered, so it may have reached the service.
    pub dispatched: bool,
    /// Unix seconds before which no retry happens.
    pub retry_at: u64,
    /// Unix seconds until which another attempt may still be running.
    pub lease_until: u64,
}

/// Whether a window-start message is due, before any attempt record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowStartEligibility {
    Off,
    /// Nothing to send yet. Look again at `until`, or when the snapshot
    /// changes if there is no time.
    Wait { until: Option<u64> },
    /// Real Claude work started a window after the reported one ended.
    NotNeeded,
    /// The reported window ended and nothing else started one; `key` names it.
    Due { key: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowStartPlan {
    Off,
    Wait { until: Option<u64> },
    NotNeeded,
    /// An earlier message for this window may have been delivered; none is
    /// sent until Claude reports a different window.
    Unconfirmed,
    /// Send one message for this attempt key.
    Send { key: String },
}

fn claude_limit(snapshot: &Snapshot, window: UsageWindow) -> Option<&UsageLimit> {
    snapshot
        .usage_limits
        .iter()
        .find(|l| l.harness == Harness::ClaudeCode && l.window == window)
}

/// The key under which attempts for an ended window are deduplicated. It
/// names the reported reset and the opt-in, so each is attempted once per
/// opt-in however many projects, processes or restarts see it. Absent while
/// the setting is off, no reset is known, or the reported window is open.
pub fn window_start_key(snapshot: &Snapshot, now: u64) -> Option<String> {
    let start = &snapshot.claude_window_start;
    if !start.enabled {
        return None;
    }
    let reset = claude_limit(snapshot, UsageWindow::FiveHour).and_then(|l| l.resets_at)?;
    if now < reset.saturating_add(WINDOW_START_GRACE_SECS) {
        return None;
    }
    Some(format!("reset:{reset}:{}", start.enabled_at.unwrap_or(0)))
}

/// Whether any genuine Claude turn is queued or running. Fixture agents never run.
pub fn claude_turn_running(snapshot: &Snapshot) -> bool {
    snapshot.sessions.iter().any(|s| {
        !s.fixture
            && s.worker.as_ref().is_some_and(|w| {
                w.harness == Harness::ClaudeCode
                    && matches!(w.status, WorkerStatus::Queued | WorkerStatus::Running)
            })
    })
}

/// Whether a window-start message is due at `now` (Unix seconds), judged only
/// from the snapshot. The server evaluates this again, from the stored
/// snapshot, when it claims an attempt and immediately before dispatching it.
pub fn window_start_eligibility(snapshot: &Snapshot, now: u64) -> WindowStartEligibility {
    if !snapshot.claude_window_start.enabled {
        return WindowStartEligibility::Off;
    }
    // Only a reported reset is acted on. With none, a real turn has to
    // report one first.
    let Some(reset) = claude_limit(snapshot, UsageWindow::FiveHour).and_then(|l| l.resets_at)
    else {
        return WindowStartEligibility::Wait { until: None };
    };
    let Some(key) = window_start_key(snapshot, now) else {
        // The reported window is still open: look again when it ends.
        return WindowStartEligibility::Wait {
            until: Some(reset.saturating_add(WINDOW_START_GRACE_SECS)),
        };
    };
    // Genuine turns start and report windows themselves.
    if claude_turn_running(snapshot) {
        return WindowStartEligibility::Wait { until: None };
    }
    // Real work since the window ended means a window is already running.
    if snapshot
        .usage_ledger
        .iter()
        .any(|e| e.harness == Harness::ClaudeCode && e.completed_at >= reset)
    {
        return WindowStartEligibility::NotNeeded;
    }
    // A used-up weekly limit rejects the message; wait for its reset.
    if let Some(weekly) = claude_limit(snapshot, UsageWindow::Weekly)
        && weekly.used_percent >= 100.0
        && let Some(until) = weekly.resets_at.filter(|r| *r > now)
    {
        return WindowStartEligibility::Wait { until: Some(until) };
    }
    WindowStartEligibility::Due { key }
}

/// Whether the message for `key` is still due at `now`.
pub fn window_start_due(snapshot: &Snapshot, now: u64, key: &str) -> bool {
    matches!(
        window_start_eligibility(snapshot, now),
        WindowStartEligibility::Due { key: due } if due == key
    )
}

/// The next step for the window-start message at `now` (Unix seconds).
/// `claim` is the durable record for the due key, if any.
pub fn plan_window_start(
    snapshot: &Snapshot,
    now: u64,
    claim: Option<WindowClaim>,
) -> WindowStartPlan {
    let key = match window_start_eligibility(snapshot, now) {
        WindowStartEligibility::Off => return WindowStartPlan::Off,
        WindowStartEligibility::Wait { until } => return WindowStartPlan::Wait { until },
        WindowStartEligibility::NotNeeded => return WindowStartPlan::NotNeeded,
        WindowStartEligibility::Due { key } => key,
    };
    if let Some(claim) = claim {
        if claim.dispatched && !claim.settled {
            return WindowStartPlan::Unconfirmed;
        }
        if claim.settled || claim.attempts >= WINDOW_START_MAX_ATTEMPTS {
            return WindowStartPlan::Wait { until: None };
        }
        let until = claim.retry_at.max(claim.lease_until);
        if until > now {
            return WindowStartPlan::Wait { until: Some(until) };
        }
    }
    WindowStartPlan::Send { key }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentProfile, UsageEntry, demo_orchestration, demo_snapshot};

    const NOW: u64 = 1_000_000;

    fn enabled(opt_in: u64) -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.claude_window_start.set_enabled(true, opt_in);
        snapshot
    }

    fn limit(snapshot: &mut Snapshot, window: UsageWindow, percent: f64, reset: Option<u64>) {
        snapshot.record_usage_limit(UsageLimit::new(
            Harness::ClaudeCode,
            window,
            percent,
            if window == UsageWindow::FiveHour {
                300
            } else {
                10080
            },
            reset,
            NOW,
        ));
    }

    fn real_work(snapshot: &mut Snapshot, at: u64) {
        snapshot.record_usage(
            UsageEntry {
                id: format!("run-{at}"),
                session_id: "session".into(),
                harness: Harness::ClaudeCode,
                model: None,
                input_tokens: 1,
                cached_input_tokens: 0,
                output_tokens: 1,
                completed_at: at,
            },
            NOW,
        );
    }

    /// An enabled snapshot whose reported five-hour window ended 100 s ago.
    fn expired() -> Snapshot {
        let mut snapshot = enabled(10);
        limit(&mut snapshot, UsageWindow::FiveHour, 40.0, Some(NOW - 100));
        snapshot
    }

    #[test]
    fn the_setting_is_off_by_default_and_hidden_until_touched() {
        let snapshot = Snapshot::default();
        assert!(!snapshot.claude_window_start.enabled);
        assert!(snapshot.claude_window_start.is_default());
        assert_eq!(plan_window_start(&snapshot, NOW, None), WindowStartPlan::Off);
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json.get("claude_window_start").is_none());
        // Snapshots stored before the setting existed load with it off.
        let loaded: Snapshot = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.claude_window_start, WindowStart::default());
    }

    #[test]
    fn nothing_is_sent_while_the_reported_window_is_open_then_one_message_after_it_ends() {
        let mut snapshot = enabled(10);
        limit(&mut snapshot, UsageWindow::FiveHour, 40.0, Some(NOW + 600));
        let due = NOW + 600 + WINDOW_START_GRACE_SECS;
        assert_eq!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Wait { until: Some(due) }
        );
        assert_eq!(
            plan_window_start(&snapshot, due - 1, None),
            WindowStartPlan::Wait { until: Some(due) }
        );
        assert_eq!(
            plan_window_start(&snapshot, due, None),
            WindowStartPlan::Send {
                key: format!("reset:{}:10", NOW + 600)
            }
        );
        assert!(window_start_due(&snapshot, due, &format!("reset:{}:10", NOW + 600)));
    }

    #[test]
    fn a_new_reported_window_changes_the_key_and_ends_the_old_attempt() {
        let mut snapshot = expired();
        let old = window_start_key(&snapshot, NOW).unwrap();
        assert!(window_start_due(&snapshot, NOW, &old));
        // Real work reported a new window after the expiry.
        limit(&mut snapshot, UsageWindow::FiveHour, 2.0, Some(NOW + 5 * 3600));
        assert_eq!(window_start_key(&snapshot, NOW), None);
        assert!(!window_start_due(&snapshot, NOW, &old));
        assert!(matches!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Wait { until: Some(_) }
        ));
        // When that window ends it is a different attempt.
        let later = window_start_key(&snapshot, NOW + 5 * 3600 + 10).unwrap();
        assert_ne!(old, later);
    }

    #[test]
    fn real_work_after_the_expiry_means_no_message_is_needed() {
        let mut snapshot = expired();
        real_work(&mut snapshot, NOW - 200);
        // Work before the window ended does not count.
        assert!(matches!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Send { .. }
        ));
        real_work(&mut snapshot, NOW - 50);
        assert_eq!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::NotNeeded
        );
        let key = window_start_key(&snapshot, NOW).unwrap();
        assert!(!window_start_due(&snapshot, NOW, &key));
    }

    #[test]
    fn a_running_claude_turn_defers_the_message() {
        let mut snapshot = demo_snapshot(AgentProfile::default());
        demo_orchestration(&mut snapshot);
        snapshot.claude_window_start.set_enabled(true, 10);
        limit(&mut snapshot, UsageWindow::FiveHour, 40.0, Some(NOW - 100));
        let key = window_start_key(&snapshot, NOW).unwrap();
        // Fixture agents never run.
        assert!(!claude_turn_running(&snapshot));
        assert!(window_start_due(&snapshot, NOW, &key));
        for session in &mut snapshot.sessions {
            session.fixture = false;
        }
        assert!(claude_turn_running(&snapshot));
        assert_eq!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Wait { until: None }
        );
        assert!(!window_start_due(&snapshot, NOW, &key));
        for session in &mut snapshot.sessions {
            if let Some(worker) = &mut session.worker {
                worker.status = WorkerStatus::Completed;
            }
        }
        assert!(window_start_due(&snapshot, NOW, &key));
    }

    #[test]
    fn with_no_reported_reset_nothing_is_sent_and_a_real_turn_is_awaited() {
        let snapshot = enabled(10);
        assert_eq!(window_start_key(&snapshot, NOW), None);
        assert_eq!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Wait { until: None }
        );
        // A limit without a reset time is just as unknown.
        let mut unknown = enabled(10);
        limit(&mut unknown, UsageWindow::FiveHour, 10.0, None);
        assert_eq!(
            plan_window_start(&unknown, NOW, None),
            WindowStartPlan::Wait { until: None }
        );
        // So is a weekly window alone.
        let mut weekly = enabled(10);
        limit(&mut weekly, UsageWindow::Weekly, 10.0, Some(NOW - 100));
        assert_eq!(
            plan_window_start(&weekly, NOW, None),
            WindowStartPlan::Wait { until: None }
        );
    }

    #[test]
    fn turning_the_setting_off_and_on_is_a_new_opt_in_with_a_new_key() {
        let mut snapshot = expired();
        let first = window_start_key(&snapshot, NOW).unwrap();
        snapshot.claude_window_start.set_enabled(false, NOW);
        snapshot.claude_window_start.set_enabled(true, NOW + 50);
        let second = window_start_key(&snapshot, NOW + 50).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn retries_back_off_and_stop_after_the_attempt_limit() {
        let snapshot = expired();
        let claim = |attempts, retry_at, lease_until, settled| {
            Some(WindowClaim {
                attempts,
                settled,
                dispatched: false,
                retry_at,
                lease_until,
            })
        };
        // Another process holds the attempt.
        assert_eq!(
            plan_window_start(&snapshot, NOW, claim(1, 0, NOW + 90, false)),
            WindowStartPlan::Wait {
                until: Some(NOW + 90)
            }
        );
        // Backing off after a failure.
        assert_eq!(
            plan_window_start(&snapshot, NOW, claim(1, NOW + 300, 0, false)),
            WindowStartPlan::Wait {
                until: Some(NOW + 300)
            }
        );
        assert!(matches!(
            plan_window_start(&snapshot, NOW + 300, claim(1, NOW + 300, 0, false)),
            WindowStartPlan::Send { .. }
        ));
        // Out of attempts, or settled: no more for this window.
        assert_eq!(
            plan_window_start(
                &snapshot,
                NOW + 9_999,
                claim(WINDOW_START_MAX_ATTEMPTS, 0, 0, false)
            ),
            WindowStartPlan::Wait { until: None }
        );
        assert_eq!(
            plan_window_start(&snapshot, NOW + 9_999, claim(1, 0, 0, true)),
            WindowStartPlan::Wait { until: None }
        );
        assert_eq!(window_start_backoff(1), 300);
        assert_eq!(window_start_backoff(2), 900);
    }

    #[test]
    fn a_possibly_delivered_message_is_never_retried_until_a_new_window_is_reported() {
        let mut snapshot = expired();
        let ambiguous = Some(WindowClaim {
            attempts: 1,
            settled: false,
            dispatched: true,
            retry_at: NOW - 1,
            lease_until: 0,
        });
        // Long after the lease and the backoff, it is still not resent.
        assert_eq!(
            plan_window_start(&snapshot, NOW + 99_999, ambiguous),
            WindowStartPlan::Unconfirmed
        );
        // A fresh reported reset reconciles it: the old key is no longer due.
        limit(&mut snapshot, UsageWindow::FiveHour, 1.0, Some(NOW + 99_999 + 600));
        assert!(matches!(
            plan_window_start(&snapshot, NOW + 99_999, ambiguous),
            WindowStartPlan::Wait { until: Some(_) }
        ));
    }

    #[test]
    fn an_exhausted_weekly_limit_waits_for_its_reset_without_a_message() {
        let mut snapshot = enabled(10);
        limit(&mut snapshot, UsageWindow::FiveHour, 100.0, Some(NOW - 100));
        limit(&mut snapshot, UsageWindow::Weekly, 100.0, Some(NOW + 86_400));
        assert_eq!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Wait {
                until: Some(NOW + 86_400)
            }
        );
        // A weekly window that already reset no longer blocks.
        limit(&mut snapshot, UsageWindow::Weekly, 100.0, Some(NOW - 1));
        assert!(matches!(
            plan_window_start(&snapshot, NOW, None),
            WindowStartPlan::Send { .. }
        ));
    }

    #[test]
    fn disabling_stops_scheduling_and_keeps_the_last_outcome() {
        let mut snapshot = expired();
        snapshot.claude_window_start.next_due_at = Some(NOW);
        snapshot.claude_window_start.last_outcome = Some(WindowStartOutcome::Sent {
            model: "claude-haiku".into(),
        });
        snapshot.claude_window_start.set_enabled(false, NOW);
        assert_eq!(window_start_key(&snapshot, NOW), None);
        assert_eq!(plan_window_start(&snapshot, NOW, None), WindowStartPlan::Off);
        assert_eq!(snapshot.claude_window_start.next_due_at, None);
        assert!(snapshot.claude_window_start.last_outcome.is_some());
    }

    #[test]
    fn failures_are_retryable_only_when_waiting_can_help() {
        for reason in [
            WindowStartFailure::SignedOut,
            WindowStartFailure::NotSubscription,
            WindowStartFailure::ManagedHooks,
            WindowStartFailure::WeeklyLimit,
            WindowStartFailure::ModelUnavailable,
            WindowStartFailure::Unsupported,
        ] {
            assert!(!reason.retryable(), "{reason:?}");
        }
        for reason in [
            WindowStartFailure::Offline,
            WindowStartFailure::TimedOut,
            WindowStartFailure::Interrupted,
            WindowStartFailure::Failed,
        ] {
            assert!(reason.retryable(), "{reason:?}");
        }
    }
}
