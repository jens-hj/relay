//! Shared Labelism primitives: square, rule-framed controls and readouts
//! whose every mark is a control or a reading.

use crate::styles::*;
use crate::theme::*;
use mosaic::core::theme::color;
use mosaic::prelude::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

pub type Select = Rc<dyn Fn(usize)>;

/// A single-choice selector whose square indicator slides to the selected
/// option. Options are radio buttons labelled "`name`: option".
#[component]
pub fn SlidingSegments(
    name: String,
    options: Vec<String>,
    index: Derived<usize>,
    select: Select,
    attention_slot: Option<usize>,
    cell_width: f32,
    disabled: Derived<bool>,
) -> Element {
    let count = options.len().max(1);
    let focus: Rc<RefCell<BTreeMap<usize, Element>>> = Rc::default();
    let width = State::new(cell_width * count as f32);
    let group = State::new(name.clone());
    let is_attention = move || attention_slot == Some(index.get());
    view! {
        stack width:{px(cell_width*count as f32)}px max-width:100% height:{px(34.0)}px shrink:0
            @layout:{move |rect:Rect|width.set(rect.size.width)} fill:surface.panel
            stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) label:{group.get()} {
            el nohit width:{width.get()/count as f32}px height:fill
                fill:{color(if is_attention() {attention.fill} else {ink.inverse})}
                translate:(x:{width.get()/count as f32*index.get() as f32}px)
                transition:(translate:ease(140.0)) {}
            row {
                let focus=focus.clone();
                let select=select.clone();
                for (slot, label) in options.into_iter().enumerate() {
                    let option_name=State::new(label);
                    let choose=select.clone();
                    button #relay.tree-control @click:{choose(slot);} width:1fr height:fill
                        role:radio disabled:{disabled.get()}
                        label:{format!("{}: {}",group.get(),option_name.get())}
                        stroke:(width:{px(if slot==0 {0.0} else {1.0})} color:rule.hair edges:left)
                        font-color:{color(if index.get()!=slot {ink.fg} else if is_attention() {attention.on} else {ink.on_inverse})} {
                        text text-wrap:none font-weight:{if index.get()==slot {700} else {400}}
                            font-color:{color(if index.get()!=slot {ink.fg} else if is_attention() {attention.on} else {ink.on_inverse})}
                            {option_name.get()}
                    } as option
                    {let semantic_option=option.clone();Effect::new(move || {semantic_option.toggled(index.get()==slot);});}
                    {let keys=focus.clone();let choose=select.clone();keys.borrow_mut().insert(slot,option.clone());let cleanup=keys.clone();on_cleanup(move ||{cleanup.borrow_mut().remove(&slot);});option.on_key(move |event,ctx| {if matches!(event.kind,KeyEventKind::Down{..}) {let next=match event.key {Key::ArrowRight|Key::ArrowDown=>(index.get_untracked()+1)%count,Key::ArrowLeft|Key::ArrowUp=>(index.get_untracked()+count-1)%count,Key::Home=>0,Key::End=>count-1,_=>return};choose(next);let target=keys.borrow().get(&next).cloned();if let Some(target)=target{target.focus();}ctx.stop_propagation();}});}
                }
            }
        }
    }
}

/// A director's identifier: a diamond, filled while the director has active
/// workers. The same mark heads the director profile.
#[component]
pub fn DirectorMark(
    size: f32,
    active: Derived<bool>,
    #[prop(default = Derived::new(|| false))] inverse: Derived<bool>,
) -> Element {
    let tone = move || {
        color(if inverse.get() {
            ink.on_inverse
        } else {
            ink.fg
        })
    };
    view! {
        stack nohit width:{px(size * 1.42)}px height:{px(size * 1.42)}px shrink:0 align:center
            justify:center {
            el width:{px(size)}px height:{px(size)}px rotate:{std::f32::consts::FRAC_PI_4}
                fill:{if active.get() {tone()} else {Color::TRANSPARENT}}
                stroke:(width:{px(if size > 12.0 {2.0} else {1.0})} color:{tone()} offset:{px(-0.5)}) {}
        }
    }
}

/// What a session or issue is doing, as shown by a status glyph and word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Ready,
    Queued,
    Running,
    Waiting,
    Completed,
    Failed,
    Stopped,
    Interrupted,
    Fixture,
    Unavailable,
}

impl RunState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready to scope",
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Waiting => "Waiting for approval",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
            Self::Interrupted => "Interrupted",
            Self::Fixture => "Fixture",
            Self::Unavailable => "Status unavailable",
        }
    }

    /// Text-grade color for the status word.
    pub fn text_color(self) -> ColorToken {
        match self {
            Self::Running => run.text,
            Self::Waiting => attention.text,
            Self::Failed => status.danger,
            _ => ink.muted,
        }
    }
}

pub fn pending_permission(snapshot: &relay_core::Snapshot, session_id: &str) -> bool {
    snapshot
        .tool_permissions
        .iter()
        .any(|p| p.session_id == session_id && p.decision.is_none() && !p.expired)
}

pub fn session_state(snapshot: &relay_core::Snapshot, session: &relay_core::Session) -> RunState {
    use relay_core::WorkerStatus;
    if session.fixture {
        return RunState::Fixture;
    }
    if pending_permission(snapshot, &session.id) {
        return RunState::Waiting;
    }
    match session.worker.as_ref().map(|w| &w.status) {
        Some(WorkerStatus::Queued) => RunState::Queued,
        Some(WorkerStatus::Running) => RunState::Running,
        Some(WorkerStatus::Completed) => RunState::Completed,
        Some(WorkerStatus::Failed) => RunState::Failed,
        Some(WorkerStatus::Stopped) => RunState::Stopped,
        Some(WorkerStatus::Interrupted) => RunState::Interrupted,
        None => RunState::Unavailable,
    }
}

/// An issue's state from its linked sessions: anything waiting on the user,
/// then active work, then the outcome of the most recently created worker.
/// Sessions carry no timestamps; snapshot order is creation order.
pub fn issue_status(snapshot: &relay_core::Snapshot, sessions: &[relay_core::Session]) -> RunState {
    let states: Vec<RunState> = sessions
        .iter()
        .map(|s| session_state(snapshot, s))
        .collect();
    for active in [RunState::Waiting, RunState::Running, RunState::Queued] {
        if states.contains(&active) {
            return active;
        }
    }
    sessions
        .iter()
        .zip(&states)
        .rev()
        .find(|(session, _)| session.worker.is_some() || session.fixture)
        .map(|(_, state)| *state)
        .unwrap_or(RunState::Ready)
}

/// A small square whose fill encodes a run state. Always paired with the
/// state's word, either visibly or in the owning control's description.
#[component]
pub fn StatusGlyph(
    state: Derived<RunState>,
    /// Draw for an inverse (selected) row: every mark uses the inverse text
    /// color, so the shape alone carries the state there.
    #[prop(default = Derived::new(|| false))]
    inverse: Derived<bool>,
) -> Element {
    let size = 11.0;
    let tone = move |token: ColorToken| color(if inverse.get() { ink.on_inverse } else { token });
    let frame = move || match state.get() {
        RunState::Running => run.text,
        RunState::Waiting => attention.text,
        RunState::Failed => status.danger,
        RunState::Completed => ink.fg,
        _ => rule.line,
    };
    view! {
        stack nohit width:{px(size)}px height:{px(size)}px shrink:0 align:center justify:center
            fill:{if state.get() == RunState::Completed {tone(ink.fg)} else {Color::TRANSPARENT}}
            stroke:(width:{px(1.0)} color:{tone(frame())} offset:{px(-0.5)}) {
            if state.get() == RunState::Running {
                col {
                    el width:fill height:1fr {}
                    el width:fill height:1fr fill:{tone(run.text)} {}
                }
            }
            if state.get() == RunState::Waiting {
                el width:{px(5.0)}px height:{px(5.0)}px fill:{tone(attention.text)} {}
            }
            if state.get() == RunState::Fixture {
                el width:{px(3.0)}px height:{px(3.0)}px fill:{tone(ink.muted)} {}
            }
            if state.get() == RunState::Stopped {
                el width:{px(7.0)}px height:{px(2.0)}px fill:{tone(ink.fg)} {}
            }
            if state.get() == RunState::Interrupted {
                el width:{px(1.5)}px height:{px(12.0)}px fill:{tone(ink.fg)}
                    rotate:{std::f32::consts::FRAC_PI_4} {}
            }
            if state.get() == RunState::Failed {
                el width:{px(1.5)}px height:{px(9.0)}px fill:{tone(status.danger)}
                    rotate:{std::f32::consts::FRAC_PI_4} {}
                el width:{px(1.5)}px height:{px(9.0)}px fill:{tone(status.danger)}
                    rotate:{-std::f32::consts::FRAC_PI_4} {}
            }
        }
    }
}

/// The tint for an issue label: stable for a given label text.
pub fn label_tint(label: &str) -> ColorToken {
    // FNV-1a keeps the mapping stable across runs and platforms.
    let hash = label.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    [tint.lilac, tint.sky, tint.mint, tint.sand][(hash % 4) as usize]
}

/// An issue label: a tinted square tag that always shows its word.
#[component]
pub fn Tag(text: String) -> Element {
    let swatch = label_tint(&text);
    view! {
        row width:max-content height:{px(20.0)}px shrink:0 align:center
            pad:(horizontal:{px(6.0)}px vertical:0px) fill:{color(swatch)} {
            text text-wrap:none font-size:{px(11.0)}px font-color:ink.fg (text.clone())
        }
    }
}

/// A caps key above a value.
#[component]
pub fn Readout(key: String, value: Derived<String>) -> Element {
    view! {
        col height:min-content min-width:0px gap:{px(3.0)}px {
            text text-wrap:none font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
                letter-spacing:{px(0.6)}px (key.clone())
            text font-size:{px(13.0)}px font-color:{color(ink.fg)} {value.get()}
        }
    }
}

/// Worker capacity: one square slot per allowed worker (up to 16). Running
/// workers fill slots in the running color; queued workers occupy slots in a
/// neutral fill.
#[component]
pub fn SlotMeter(
    running: Derived<usize>,
    active: Derived<usize>,
    limit: Derived<usize>,
) -> Element {
    view! {
        row nohit width:max-content height:min-content shrink:0 align:center gap:{px(2.0)}px {
            for (_, slot) in {(0..limit.get().min(16)).map(|i| (i, i)).collect::<Vec<_>>()} {
                let cell = *slot;
                el width:{px(6.0)}px height:{px(11.0)}px shrink:0
                    fill:{if cell < running.get() {color(run.fill)} else if cell < active.get() {color(ink.muted)} else {Color::TRANSPARENT}}
                    stroke:(width:{px(1.0)} color:{color(if cell < running.get() {run.text} else {rule.line})} offset:{px(-0.5)}) {}
            }
            if limit.get() > 16 {
                text text-wrap:none font-size:{px(11.0)}px font-color:{color(ink.muted)}
                    {format!("+{}", limit.get() - 16)}
            }
        }
    }
}

/// Latest-turn usage split for display. Adapters normalize
/// `input_tokens` to include cached input, so cached is a subset of input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsageSplit {
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

pub fn grouped(value: u64) -> String {
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

/// Measured usage of the latest reported turn: raw counts plus a meter of
/// uncached input, cached input and output. Bound to the session's current
/// usage, so a new report for the same session updates it in place.
#[component]
pub fn UsageReadout(usage: Derived<Option<relay_core::TokenUsage>>) -> Element {
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
            text font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
                letter-spacing:{px(0.6)}px "Latest reported turn · measured"
            grid cols:(1fr 1fr 1fr) height:min-content gap:{px(12.0)}px {
                Readout key:("Input · includes cached".to_string()) value:(input)
                Readout key:("Cached · subset of input".to_string()) value:(cached)
                Readout key:("Output".to_string()) value:(output)
            }
            if fractions.get().is_some() {
                row height:{px(12.0)}px gap:{px(1.0)}px fill:surface.panel
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) label:"Usage meter" {
                    el width:{share(0)}fr height:fill fill:meter.input {}
                    el width:{share(1)}fr height:fill fill:meter.cached {}
                    el width:{share(2)}fr height:fill fill:meter.output {}
                }
                row height:min-content gap:{px(12.0)}px {
                    for (name, swatch) in [("Uncached input", meter.input), ("Cached input", meter.cached), ("Output", meter.output)] {
                        row width:max-content height:min-content align:center gap:{px(5.0)}px {
                            el width:{px(9.0)}px height:{px(9.0)}px fill:{color(swatch)}
                                stroke:(width:{px(1.0)} color:rule.line offset:{px(-0.5)}) {}
                            text text-wrap:none font-size:{px(11.0)}px font-color:ink.muted (name)
                        }
                    }
                }
            } else {
                text font-size:{px(12.0)}px font-color:ink.muted
                    "No tokens reported for the latest turn"
            }
            if split.get().is_some_and(|s| s.clamped) {
                text font-size:{px(12.0)}px font-color:status.warning
                    "Reported cached tokens exceed input; meter clamped"
            }
        }
    }
}

/// Running, occupied (running or queued) and allowed workers for a director.
pub fn director_capacity(
    snapshot: &relay_core::Snapshot,
    director_id: &str,
) -> (usize, usize, usize) {
    use relay_core::{SessionRole, WorkerStatus};
    let statuses: Vec<&WorkerStatus> = snapshot
        .sessions
        .iter()
        .filter(|s| s.director_id == director_id && s.role != SessionRole::Director)
        .filter_map(|s| s.worker.as_ref().map(|w| &w.status))
        .collect();
    let running = statuses
        .iter()
        .filter(|s| matches!(s, WorkerStatus::Running))
        .count();
    let active = statuses
        .iter()
        .filter(|s| matches!(s, WorkerStatus::Running | WorkerStatus::Queued))
        .count();
    let limit = snapshot
        .directors
        .iter()
        .find(|d| d.id == director_id)
        .and_then(|d| snapshot.effective_profile(d).ok())
        .map(|p| p.max_workers as usize)
        .unwrap_or(0);
    (running, active, limit)
}
