//! The usage forecast: assuming the pace so far continues until the window
//! resets, where does usage land and when does it run out?
//!
//! With usage `p` (%), time elapsed `e` and time left `l`:
//! `rate = p / e`, `projected = p + rate · l`, `empty in = (100 − p) / rate`.

use std::time::Duration;

/// Above this projection the window runs out before it resets.
pub const RUNS_OUT_ABOVE: f32 = 100.0;
/// At or below this projection a quarter or more would go unused.
pub const FULL_STEAM_AT_MOST: f32 = 75.0;

/// What the projection means for the person using it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Projected above 100%: slow down.
    RunsOutEarly,
    /// Projected above 75% and up to 100%.
    OnPace,
    /// Projected at most 75%: there is room to spend.
    FullSteam,
}

impl Verdict {
    pub fn of(projected: f32) -> Self {
        if projected > RUNS_OUT_ABOVE {
            Verdict::RunsOutEarly
        } else if projected > FULL_STEAM_AT_MOST {
            Verdict::OnPace
        } else {
            Verdict::FullSteam
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Verdict::RunsOutEarly => "Runs out early",
            Verdict::OnPace => "On pace",
            Verdict::FullSteam => "Full steam ahead",
        }
    }
}

/// A projection of one usage window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    /// Usage so far, %.
    pub used: f32,
    /// Percent per second.
    pub rate: f32,
    /// Usage at reset if the pace holds, %.
    pub projected: f32,
    /// Time until 100% at this pace; `None` when nothing is being used.
    pub empty_in: Option<Duration>,
    /// Time left in the window.
    pub left: Duration,
    pub verdict: Verdict,
}

/// Projects `used` percent after `elapsed` of a window with `left` to go.
/// `None` before any time has passed, when no pace exists yet.
pub fn forecast(used: f32, elapsed: Duration, left: Duration) -> Option<Forecast> {
    let elapsed_s = elapsed.as_secs_f32();
    if elapsed_s <= 0.0 || !used.is_finite() {
        return None;
    }
    let used = used.max(0.0);
    let rate = used / elapsed_s;
    let projected = used + rate * left.as_secs_f32();
    let empty_in = (rate > 0.0).then(|| Duration::from_secs_f32(((100.0 - used) / rate).max(0.0)));
    Some(Forecast {
        used,
        rate,
        projected,
        empty_in,
        left,
        verdict: Verdict::of(projected),
    })
}

impl Forecast {
    /// The burn rate per `unit`, in percent.
    pub fn rate_per(&self, unit: Duration) -> f32 {
        self.rate * unit.as_secs_f32()
    }

    /// How long before the reset the window empties, when it does.
    pub fn short_by(&self) -> Option<Duration> {
        let empty = self.empty_in?;
        self.left.checked_sub(empty).filter(|gap| !gap.is_zero())
    }

    /// The detail line under the verdict: "empty in 1h 14m · 1h 46m before
    /// reset", "~16% spare at reset", or "~40% would go unused".
    pub fn detail(&self) -> String {
        match self.verdict {
            Verdict::RunsOutEarly => match (self.empty_in, self.short_by()) {
                (Some(empty), Some(short)) => format!(
                    "empty in {} · {} before reset",
                    format_duration(empty),
                    format_duration(short)
                ),
                (Some(empty), None) => format!("empty in {}", format_duration(empty)),
                _ => "runs out before reset".to_string(),
            },
            Verdict::OnPace => {
                format!("~{:.0}% spare at reset", (100.0 - self.projected).max(0.0))
            }
            Verdict::FullSteam => {
                format!("~{:.0}% would go unused", (100.0 - self.projected).max(0.0))
            }
        }
    }
}

/// "1h 14m", "45m", "2d 4h", rounded to the minute.
pub fn format_duration(duration: Duration) -> String {
    let minutes = (duration.as_secs_f64() / 60.0).round() as u64;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn the_guides_worked_example() {
        // 62% after 2h of a 5h session.
        let forecast = forecast(62.0, 2 * HOUR, 3 * HOUR).unwrap();
        assert!((forecast.rate_per(HOUR) - 31.0).abs() < 1e-3);
        assert!((forecast.projected - 155.0).abs() < 1e-3);
        assert_eq!(forecast.verdict, Verdict::RunsOutEarly);
        assert_eq!(format_duration(forecast.empty_in.unwrap()), "1h 14m");
        assert_eq!(format_duration(forecast.short_by().unwrap()), "1h 46m");
        assert_eq!(forecast.detail(), "empty in 1h 14m · 1h 46m before reset");
    }

    #[test]
    fn verdict_thresholds_are_100_and_75() {
        assert_eq!(Verdict::of(100.1), Verdict::RunsOutEarly);
        assert_eq!(Verdict::of(100.0), Verdict::OnPace);
        assert_eq!(Verdict::of(75.1), Verdict::OnPace);
        assert_eq!(Verdict::of(75.0), Verdict::FullSteam);
        assert_eq!(Verdict::of(0.0), Verdict::FullSteam);
    }

    #[test]
    fn the_board_week_is_on_pace() {
        // 48% at 57% of a week: 12.0%/day, ~16% spare.
        let week = 7 * 24 * HOUR;
        let elapsed = week.mul_f32(0.57);
        let forecast = forecast(48.0, elapsed, week - elapsed).unwrap();
        assert_eq!(forecast.verdict, Verdict::OnPace);
        assert_eq!(format!("{:.1}", forecast.rate_per(24 * HOUR)), "12.0");
        assert_eq!(forecast.detail(), "~16% spare at reset");
        let calm = super::forecast(20.0, 2 * HOUR, 3 * HOUR).unwrap();
        assert_eq!(calm.verdict, Verdict::FullSteam);
        assert_eq!(calm.detail(), "~50% would go unused");
    }

    #[test]
    fn no_pace_without_time_or_use() {
        assert!(forecast(10.0, Duration::ZERO, HOUR).is_none());
        let idle = forecast(0.0, HOUR, HOUR).unwrap();
        assert_eq!(idle.empty_in, None);
        assert_eq!(idle.verdict, Verdict::FullSteam);
        assert_eq!(format_duration(Duration::from_secs(45 * 60)), "45m");
        assert_eq!(format_duration(52 * HOUR), "2d 4h");
        assert_eq!(format_duration(3 * HOUR), "3h 00m");
    }
}
