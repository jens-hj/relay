//! Agent state in words and marks, and the small readouts views share.

use mosaic::core::theme::color;
use mosaic::prelude::*;
use relay_ui::prelude::{
    AgentStatus, Label, LabelProps, StatusSquare, StatusSquareProps, TextRole, Tone,
};
use relay_ui::tokens::{ground, px};

/// The word for a state; `None` is an issue no agent has worked on yet.
pub(crate) fn status_word(status: Option<AgentStatus>) -> &'static str {
    match status {
        Some(AgentStatus::Working) => "Working",
        Some(AgentStatus::Waiting) => "Waiting for approval",
        Some(AgentStatus::Failed) => "Failed",
        Some(AgentStatus::Queued) => "Queued",
        Some(AgentStatus::Done) => "Done",
        None => "Ready to scope",
    }
}

/// The colour of a state's word: its signal while it needs attention or
/// failed, muted otherwise.
pub(crate) fn status_tone(status: Option<AgentStatus>) -> ColorToken {
    match status {
        Some(status @ (AgentStatus::Working | AgentStatus::Waiting | AgentStatus::Failed)) => {
            status.token()
        }
        _ => ground.muted,
    }
}

/// A status square, or nothing without a state.
#[component]
pub(crate) fn StatusMark(status: Derived<Option<AgentStatus>>) -> Element {
    let host = view! {
        row width:max-content height:min-content shrink:0 {}
    };
    host.switch(
        move || status.get(),
        |parent, status| {
            if let Some(status) = *status {
                parent.adopt(&view! {
                    StatusSquare status:(status)
                });
            }
        },
    );
    host
}

/// A status square beside its word.
#[component]
pub(crate) fn StatusLine(status: Derived<Option<AgentStatus>>) -> Element {
    view! {
        row width:max-content height:min-content shrink:0 align:center gap:{px(7.0)}px {
            StatusMark status:(status)
            row width:max-content height:min-content font-size:{px(11.0)}px
                font-color:{color(status_tone(status.get()))} {
                text text-wrap:none {status_word(status.get())}
            }
        }
    }
}

/// A muted caps key above its value.
#[component]
pub(crate) fn Readout(key: String, value: Derived<String>) -> Element {
    view! {
        col height:min-content min-width:0px gap:{px(3.0)}px {
            Label text:(key.clone()) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                caps:true
            Label text:(value) role:(TextRole::Body)
        }
    }
}

pub(crate) fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Latest-turn usage split for display. Adapters normalize
/// `input_tokens` to include cached input, so cached is a subset of input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct UsageSplit {
    pub uncached: u64,
    pub cached: u64,
    pub output: u64,
    /// The reported cached count exceeded input and was clamped for the meter.
    pub clamped: bool,
}

impl UsageSplit {
    pub fn new(usage: &relay_core::TokenUsage) -> Self {
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        Self {
            uncached: usage.input_tokens - cached,
            cached,
            output: usage.output_tokens,
            clamped: usage.cached_input_tokens > usage.input_tokens,
        }
    }

    /// Meter fractions for uncached input, cached input and output; `None`
    /// when nothing was reported.
    pub fn fractions(&self) -> Option<[f32; 3]> {
        let parts = [self.uncached, self.cached, self.output];
        let total: u128 = parts.iter().map(|&p| u128::from(p)).sum();
        (total > 0)
            .then(|| parts.map(|p| (u128::from(p) as f64 / total as f64).clamp(0.0, 1.0) as f32))
    }
}

/// Measured usage of the latest reported turn: raw counts plus a meter of
/// uncached input, cached input and output.
#[component]
pub(crate) fn UsageReadout(usage: Derived<Option<relay_core::TokenUsage>>) -> Element {
    let split = Derived::new(move || usage.get().map(|u| UsageSplit::new(&u)));
    let fractions = Derived::new(move || split.get().and_then(|s| s.fractions()));
    let raw = move |pick: fn(&relay_core::TokenUsage) -> u64| {
        Derived::new(move || {
            usage
                .get()
                .map(|u| grouped(pick(&u)))
                .unwrap_or_else(|| "—".into())
        })
    };
    let input = raw(|u| u.input_tokens);
    let cached = raw(|u| u.cached_input_tokens);
    let output = raw(|u| u.output_tokens);
    let share = move |i: usize| fractions.get().map(|f| f[i]).unwrap_or(0.0);
    view! {
        col height:min-content gap:{px(8.0)}px label:"Latest turn usage" {
            text font-size:{px(11.0)}px font-color:ground.muted text-transform:uppercase
                letter-spacing:{px(0.6)}px "Latest reported turn · measured"
            grid cols:(1fr 1fr 1fr) height:min-content gap:{px(12.0)}px {
                Readout key:("Input · includes cached".to_string()) value:(input)
                Readout key:("Cached · subset of input".to_string()) value:(cached)
                Readout key:("Output".to_string()) value:(output)
            }
            if fractions.get().is_some() {
                row height:{px(12.0)}px gap:{px(1.0)}px fill:ground.panel
                    stroke:(width:{px(1.0)} color:ground.rule offset:{px(-1.0)})
                    label:"Usage meter" {
                    el width:{share(0)}fr height:fill fill:ground.ink {}
                    el width:{share(1)}fr height:fill fill:ground.muted {}
                    el width:{share(2)}fr height:fill fill:ground.rule_strong {}
                }
                row height:min-content gap:{px(12.0)}px {
                    for (name, swatch) in [("Uncached input", ground.ink), ("Cached input", ground.muted), ("Output", ground.rule_strong)] {
                        row width:max-content height:min-content align:center gap:{px(5.0)}px {
                            el width:{px(9.0)}px height:{px(9.0)}px fill:{color(swatch)}
                                stroke:(width:{px(1.0)} color:ground.rule offset:{px(-0.5)}) {}
                            text text-wrap:none font-size:{px(11.0)}px font-color:ground.muted
                                (name)
                        }
                    }
                }
            } else {
                text font-size:{px(12.0)}px font-color:ground.muted
                    "No tokens reported for the latest turn"
            }
            if split.get().is_some_and(|s| s.clamped) {
                text font-size:{px(12.0)}px font-color:{color(relay_ui::tokens::status.needs_you)}
                    "Reported cached tokens exceed input; meter clamped"
            }
        }
    }
}

/// A harness check as a status mark: ready is done, a sign-in or version
/// problem needs you, a failed check failed, and anything else is pending.
pub(crate) fn harness_state(state: &str) -> Option<AgentStatus> {
    Some(match state {
        "ready" => AgentStatus::Done,
        "signed_out" | "incompatible" => AgentStatus::Waiting,
        "unknown" | "error" | "failed" => AgentStatus::Failed,
        _ => AgentStatus::Queued,
    })
}
