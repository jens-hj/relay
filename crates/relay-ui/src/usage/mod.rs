//! Usage limits: one resting row per harness at the sidebar foot, opening
//! into a forecast with the burn rate, model mix, heaviest agents, quick
//! stats and a suggestion.

pub mod forecast;
#[cfg(test)]
mod tests;
mod view;

pub use view::{
    ForecastBar, ForecastBarProps, UsageBar, UsageBarProps, UsageDetail, UsageDetailProps,
    UsageFoot, UsageFootProps, UsageRow, UsageRowProps,
};

use crate::model::{Harness, ModelHue};
use std::rc::Rc;
use std::time::Duration;

crate::value::live_value!(Vec<HarnessUsage>);

/// One rolling limit window: usage so far and how far through it we are.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageWindow {
    /// Percent of the limit used.
    pub used: f32,
    /// Time since the window started; the elapsed tick and the forecast
    /// need it, and are hidden without it.
    pub elapsed: Option<Duration>,
    /// Length of the window (5 hours, a week).
    pub length: Duration,
    /// When it resets, as the caller words it ("resets in 3h 00m",
    /// "resets Mon 09:00"); hidden when empty.
    pub resets: String,
}

impl UsageWindow {
    pub fn new(used: f32, length: Duration) -> Self {
        UsageWindow {
            used,
            elapsed: None,
            length,
            resets: String::new(),
        }
    }

    pub fn elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed = Some(elapsed);
        self
    }

    pub fn resets(mut self, text: impl Into<String>) -> Self {
        self.resets = text.into();
        self
    }

    /// Fraction of the window that has passed, 0..1.
    pub fn elapsed_fraction(&self) -> Option<f32> {
        let elapsed = self.elapsed?;
        (!self.length.is_zero())
            .then(|| (elapsed.as_secs_f32() / self.length.as_secs_f32()).clamp(0.0, 1.0))
    }

    /// The projection to reset, when the elapsed time is known.
    pub fn forecast(&self) -> Option<forecast::Forecast> {
        let elapsed = self.elapsed?;
        forecast::forecast(self.used, elapsed, self.length.saturating_sub(elapsed))
    }

    /// The burn-rate unit: per hour for windows under a day, per day
    /// otherwise.
    pub fn rate_unit(&self) -> (Duration, &'static str) {
        if self.length < Duration::from_secs(86_400) {
            (Duration::from_secs(3600), "%/h")
        } else {
            (Duration::from_secs(86_400), "%/day")
        }
    }
}

/// A model's share of usage.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelShare {
    pub name: String,
    pub hue: ModelHue,
    /// Percent of usage.
    pub share: f32,
}

/// One concrete move, shown in the detail's ruled footer with an Apply
/// cell: `subject` bold, then `text` ("Flaky tests" → "Sonnet lands the
/// session").
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub subject: String,
    pub text: String,
}

/// The opened detail of one harness. Every part is optional and hidden
/// when empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageInsight {
    /// Model mix this week.
    pub model_mix: Vec<ModelShare>,
    /// The agents that used most of the limit: name and percent.
    pub heaviest: Vec<(String, f32)>,
    /// Quick stats as label and value ("peak hour", "14–15h").
    pub stats: Vec<(String, String)>,
    pub suggestion: Option<Suggestion>,
}

/// Usage of one connected harness.
#[derive(Clone, Debug, PartialEq)]
pub struct HarnessUsage {
    pub harness: Harness,
    /// The rolling session window ("5h").
    pub session: Option<UsageWindow>,
    /// The weekly limit ("wk").
    pub week: Option<UsageWindow>,
    pub insight: UsageInsight,
}

impl HarnessUsage {
    /// "Claude Code: session 62 percent, week 48 percent".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(session) = &self.session {
            parts.push(format!("session {:.0} percent", session.used));
        }
        if let Some(week) = &self.week {
            parts.push(format!("week {:.0} percent", week.used));
        }
        if parts.is_empty() {
            self.harness.name().to_string()
        } else {
            format!("{}: {}", self.harness.name(), parts.join(", "))
        }
    }
}

/// Called with the harness whose suggestion was applied.
pub type OnApply = Rc<dyn Fn(Harness)>;
