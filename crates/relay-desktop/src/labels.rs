//! Shared Labelism primitives: square, rule-framed controls and readouts
//! whose every mark is a control or a reading.

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
    attention: Option<usize>,
    cell_width: f32,
    disabled: Derived<bool>,
) -> Element {
    let count = options.len().max(1);
    let focus: Rc<RefCell<BTreeMap<usize, Element>>> = Rc::default();
    let width = State::new(cell_width * count as f32);
    let group = State::new(name.clone());
    let is_attention = move || attention == Some(index.get());
    view! {
        stack width:{px(cell_width*count as f32)}px max-width:100% height:{px(34.0)}px shrink:0
            @layout:{move |rect:Rect|width.set(rect.size.width)} fill:surface
            stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)}) label:{group.get()} {
            el nohit width:{width.get()/count as f32}px height:fill
                fill:{color(if is_attention() {attention_fill} else {inverse})}
                translate:(x:{width.get()/count as f32*index.get() as f32}px)
                transition:(translate:ease(140.0)) {}
            row {
                let focus=focus.clone();
                let select=select.clone();
                for (slot, label) in options.into_iter().enumerate() {
                    let option_name=State::new(label);
                    let choose=select.clone();
                    button #tree-control @click:{choose(slot);} width:1fr height:fill role:radio
                        disabled:{disabled.get()}
                        label:{format!("{}: {}",group.get(),option_name.get())}
                        stroke:(width:{px(if slot==0 {0.0} else {1.0})} color:edge edges:left)
                        font-color:{color(if index.get()!=slot {ink} else if is_attention() {on_attention} else {on_inverse})} {
                        text text-wrap:none font-weight:{if index.get()==slot {700} else {400}}
                            font-color:{color(if index.get()!=slot {ink} else if is_attention() {on_attention} else {on_inverse})}
                            {option_name.get()}
                    } as option
                    {let semantic_option=option.clone();Effect::new(move || {semantic_option.toggled(index.get()==slot);});}
                    {let keys=focus.clone();let choose=select.clone();keys.borrow_mut().insert(slot,option.clone());let cleanup=keys.clone();on_cleanup(move ||{cleanup.borrow_mut().remove(&slot);});option.on_key(move |event,ctx| {if matches!(event.kind,KeyEventKind::Down{..}) {let next=match event.key {Key::ArrowRight|Key::ArrowDown=>(index.get_untracked()+1)%count,Key::ArrowLeft|Key::ArrowUp=>(index.get_untracked()+count-1)%count,Key::Home=>0,Key::End=>count-1,_=>return};choose(next);let target=keys.borrow().get(&next).cloned();if let Some(target)=target{target.focus();}ctx.stop_propagation();}});}
                }
            }
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
            Self::Running => run_text,
            Self::Waiting => attention_text,
            Self::Failed => danger,
            _ => muted,
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
pub fn StatusGlyph(state: Derived<RunState>) -> Element {
    let size = 11.0;
    let frame = move || match state.get() {
        RunState::Running => run_text,
        RunState::Waiting => attention_text,
        RunState::Failed => danger,
        RunState::Completed => ink,
        _ => rule,
    };
    view! {
        stack nohit width:{px(size)}px height:{px(size)}px shrink:0 align:center justify:center
            fill:{if state.get() == RunState::Completed {color(ink)} else {Color::TRANSPARENT}}
            stroke:(width:{px(1.0)} color:{color(frame())} offset:{px(-0.5)}) {
            if state.get() == RunState::Running {
                col {
                    el height:1fr {}
                    el height:1fr fill:run-text {}
                }
            }
            if state.get() == RunState::Waiting {
                el width:{px(5.0)}px height:{px(5.0)}px fill:attention-text {}
            }
            if state.get() == RunState::Fixture {
                el width:{px(3.0)}px height:{px(3.0)}px fill:muted {}
            }
            if state.get() == RunState::Stopped {
                el width:{px(7.0)}px height:{px(2.0)}px fill:ink {}
            }
            if state.get() == RunState::Interrupted {
                el width:{px(1.5)}px height:{px(12.0)}px fill:ink
                    rotate:{std::f32::consts::FRAC_PI_4} {}
            }
            if state.get() == RunState::Failed {
                el width:{px(1.5)}px height:{px(9.0)}px fill:danger
                    rotate:{std::f32::consts::FRAC_PI_4} {}
                el width:{px(1.5)}px height:{px(9.0)}px fill:danger
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
    [tint_lilac, tint_sky, tint_mint, tint_sand][(hash % 4) as usize]
}

/// An issue label: a tinted square tag that always shows its word.
#[component]
pub fn Tag(text: String) -> Element {
    let tint = label_tint(&text);
    view! {
        row width:max-content height:{px(20.0)}px shrink:0 align:center
            pad:(horizontal:{px(6.0)}px vertical:0px) fill:{color(tint)} {
            text text-wrap:none font-size:{px(11.0)}px font-color:ink (text.clone())
        }
    }
}

/// A caps key above a value.
#[component]
pub fn Readout(key: String, value: Derived<String>) -> Element {
    view! {
        col height:min-content min-width:0px gap:{px(3.0)}px {
            text text-wrap:none font-size:{px(11.0)}px font-color:muted text-transform:uppercase
                letter-spacing:{px(0.6)}px (key.clone())
            text font-size:{px(13.0)}px font-color:{color(ink)} {value.get()}
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
                    fill:{if cell < running.get() {color(run_fill)} else if cell < active.get() {color(muted)} else {Color::TRANSPARENT}}
                    stroke:(width:{px(1.0)} color:{color(if cell < running.get() {run_text} else {rule})} offset:{px(-0.5)}) {}
            }
            if limit.get() > 16 {
                text text-wrap:none font-size:{px(11.0)}px font-color:{color(muted)}
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
/// uncached input, cached input and output.
#[component]
pub fn UsageReadout(usage: relay_core::TokenUsage) -> Element {
    let split = UsageSplit::new(&usage);
    let input = State::new(grouped(usage.input_tokens));
    let cached = State::new(grouped(usage.cached_input_tokens));
    let output = State::new(grouped(usage.output_tokens));
    let fractions = split.fractions();
    let reported = fractions.is_some();
    let [uncached, cached_part, output_part] = fractions.unwrap_or([0.0; 3]);
    view! {
        col height:min-content gap:{px(8.0)}px label:"Latest turn usage" {
            text font-size:{px(11.0)}px font-color:muted text-transform:uppercase
                letter-spacing:{px(0.6)}px "Latest reported turn · measured"
            grid cols:(1fr 1fr 1fr) height:min-content gap:{px(12.0)}px {
                Readout key:("Input · includes cached".to_string())
                    value:(Derived::new(move || input.get()))
                Readout key:("Cached · subset of input".to_string())
                    value:(Derived::new(move || cached.get()))
                Readout key:("Output".to_string()) value:(Derived::new(move || output.get()))
            }
            if reported {
                row height:{px(12.0)}px stroke:(width:{px(1.0)} color:rule offset:{px(-1.0)})
                    label:"Usage meter" {
                    el width:{uncached}fr fill:tint-sky {}
                    el width:{cached_part}fr fill:tint-lilac {}
                    el width:{output_part}fr fill:tint-sand {}
                }
                row height:min-content gap:{px(12.0)}px {
                    for (name, tint) in [("Uncached input", tint_sky), ("Cached input", tint_lilac), ("Output", tint_sand)] {
                        row width:max-content height:min-content align:center gap:{px(5.0)}px {
                            el width:{px(9.0)}px height:{px(9.0)}px fill:{color(tint)}
                                stroke:(width:{px(1.0)} color:rule offset:{px(-0.5)}) {}
                            text text-wrap:none font-size:{px(11.0)}px font-color:muted (name)
                        }
                    }
                }
            } else {
                text font-size:{px(12.0)}px font-color:muted
                    "No tokens reported for the latest turn"
            }
            if split.clamped {
                text font-size:{px(12.0)}px font-color:warning
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
