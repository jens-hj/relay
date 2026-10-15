//! The session header: path and serif title over a live wash, the context
//! gauge, and the control block flush right.

#[cfg(test)]
mod tests;

use crate::context::{ContextGauge, ContextGaugeProps, ContextView};
use crate::marks::{Mark, mark, sized_text};
use crate::prelude::*;
use crate::tokens::{Signal, ground, px, srgb_alpha};
use crate::tree::{StatusTally, tally_label};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::widgets::Visual;

/// "Working", "Waiting", … for a status cell.
pub fn status_title(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "Idle",
        AgentStatus::Working => "Working",
        AgentStatus::Waiting => "Waiting",
        AgentStatus::Failed => "Failed",
        AgentStatus::Queued => "Queued",
        AgentStatus::Done => "Done",
    }
}

/// "Working. 4 agents below: 1 done, 1 waiting, 1 working, 1 queued. Show
/// in tree".
pub fn status_cell_label(status: AgentStatus, below: &[AgentStatus]) -> String {
    if below.is_empty() {
        return status_title(status).to_string();
    }
    let mut tally = StatusTally::default();
    for each in below {
        tally.add(*each);
    }
    let agents = if below.len() == 1 { "agent" } else { "agents" };
    format!(
        "{}. {} {agents} below: {}. Show in tree",
        status_title(status),
        below.len(),
        tally_label(&tally)
    )
}

pub(crate) fn header_cell(
    label: String,
    on_press: OnPress,
    wide: bool,
    stop: bool,
    content: Element,
) -> Element {
    let pad = if wide { 22.0 } else { 18.0 };
    let cell = view! {
        button @click:{on_press()} width:max-content height:fill min-width:{px(64.0)}px shrink:0
            radius:0px pad:(horizontal:{px(pad)}px vertical:0px) label:(label)
            font-color:ground.ink {
            {
                parent.adopt(&content);
            }
        }
    };
    let interaction = cell.interaction();
    cell.visual_dyn(move || {
        let wash = if stop {
            let red = color(Signal::Failed.token());
            red.alpha(srgb_alpha(red, color(ground.page), 0.08))
        } else {
            color(ground.hover)
        };
        let fill = if interaction.pressed() {
            if stop { wash } else { color(ground.pressed) }
        } else if interaction.hovered() {
            wash
        } else {
            Color::TRANSPARENT
        };
        let mut visual = Visual::new()
            .fill(fill)
            .stroke(StrokeSpec::new(px(1.0), color(ground.rule)).edges(StrokeEdges::LEFT));
        if interaction.focus_visible() {
            visual = visual.stroke(StrokeSpec::new(px(1.0), color(ground.ink)).offset(-px(1.0)));
        }
        visual
    });
    cell
}

pub(crate) fn column(children: Vec<Element>) -> Element {
    let host = view! {
        col width:max-content height:min-content gap:{px(8.0)}px align:start {}
    };
    for child in children {
        host.adopt(&child);
    }
    host
}

/// The header's control block: full-height ruled cells flush right. The
/// status cell shows the session's state with one square per agent below
/// and "n below" (pressing it shows them in the tree); Pause (P); Stop (⇧S)
/// with a red square and a red hover tint. A cell without a handler is not
/// shown.
#[component]
pub fn ControlBlock(
    /// The session's own state.
    status: AgentStatus,
    /// The agents below it, in tree order.
    #[prop(optional)]
    below: Vec<AgentStatus>,
    #[prop(optional)] on_below: Option<OnPress>,
    #[prop(optional)] on_pause: Option<OnPress>,
    /// The queue is held: the pause cell offers Resume.
    #[prop(default = false)]
    paused: bool,
    #[prop(optional)] on_stop: Option<OnPress>,
) -> Element {
    let block = view! {
        row width:max-content shrink:0 {}
    };
    let squares = view! {
        row width:max-content height:min-content gap:{px(3.0)}px align:center {}
    };
    for each in &below {
        squares.adopt(&view! {
            StatusSquare status:(*each)
        });
    }
    let head = view! {
        row width:max-content height:min-content gap:{px(8.0)}px align:center {
            StatusSquare status:(status)
            text font-family:monospace font-size:{px(12.0)}px font-weight:600 (status_title(status))
        }
    };
    let mut parts = vec![head];
    if !below.is_empty() {
        let count = format!("{} below", below.len());
        let line = view! {
            row width:max-content height:min-content gap:{px(6.0)}px align:center {}
        };
        line.adopt(&squares);
        line.adopt(&sized_text(count.into(), 11.0, 400, ground.muted));
        parts.push(line);
    }
    let label = status_cell_label(status, &below);
    let status_cell = match on_below {
        Some(press) => header_cell(label, press, true, false, column(parts)),
        None => {
            let cell = view! {
                col width:max-content min-width:{px(64.0)}px shrink:0 justify:center
                    pad:(horizontal:{px(22.0)}px vertical:0px) label:(label)
                    stroke:(width:{px(1.0)} color:ground.rule edges:left) {}
            };
            cell.adopt(&column(parts));
            cell
        }
    };
    block.adopt(&status_cell);
    if let Some(pause) = on_pause {
        let (glyph, word) = if paused {
            (Mark::Play, "Resume")
        } else {
            (Mark::Pause, "Pause")
        };
        let content = column(vec![
            mark(glyph, 14.0, ground.ink),
            sized_text(word.to_string().into(), 12.0, 500, ground.ink),
            view! {
                Kbd keys:"P"
            },
        ]);
        block.adopt(&header_cell(word.into(), pause, false, false, content));
    }
    if let Some(stop) = on_stop {
        let content = column(vec![
            mark(Mark::Stop, 14.0, Signal::Failed.token()),
            sized_text("Stop".to_string().into(), 12.0, 500, ground.ink),
            view! {
                Kbd keys:"⇧S"
            },
        ]);
        block.adopt(&header_cell("Stop".into(), stop, false, true, content));
    }
    block
}

/// The 88px session header: the path in muted mono and the session name in
/// the serif; while the session works, a model-colored dither fills the whole
/// height, reaching as far as the effort level; then the context gauge and
/// the control block flush right.
#[component]
pub fn SessionHeader(
    #[prop(optional)] model_hue: Option<crate::model::ModelHue>,
    /// Ancestor names, root first.
    #[prop(optional)]
    path: Vec<String>,
    title: UiText,
    /// The session's own state; working draws the live wash.
    status: Live<AgentStatus>,
    /// Sets the wash's reach; the reach of level 0 without it.
    #[prop(optional)]
    effort: Option<Effort>,
    #[prop(optional)] context: Option<ContextView>,
    #[prop(optional)] on_compact: Option<OnPress>,
    /// The agents below, for the status cell.
    #[prop(optional)]
    below: Vec<AgentStatus>,
    #[prop(optional)] on_below: Option<OnPress>,
    #[prop(optional)] on_pause: Option<OnPress>,
    /// The queue is held: the pause cell offers Resume.
    #[prop(default = false)]
    paused: bool,
    #[prop(optional)] on_stop: Option<OnPress>,
) -> Element {
    let level = effort.map_or(0, |effort| effort.level());
    let header = view! {
        stack height:{px(88.0)}px shrink:0 clip label:(title.get_untracked()) {}
    };
    let working = status.clone();
    let wash_host = view! {
        stack nohit {}
    };
    wash_host.show(
        move || working.get() == AgentStatus::Working,
        move |parent| {
            parent.adopt(&view! {
                Wash reach:(live_reach(level)) opacity:0.14 over:(ground.page)
                    colour:(model_hue.map(|hue| hue.token()))
            });
        },
    );
    header.adopt(&wash_host);
    let titles = view! {
        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px {}
    };
    if !path.is_empty() {
        titles.adopt(&view! {
            PathText segments:(path) trailing:true
        });
    }
    titles.adopt(&view! {
        text font-family:serif font-size:{px(30.0)}px line-height:{px(30.0)}px {title.get()}
    });
    let content = view! {
        row gap:{px(12.0)}px align:center pad:(left:{px(32.0)}px right:0px top:0px bottom:0px) {}
    };
    content.adopt(&titles);
    if let Some(context) = context {
        content.adopt(&view! {
            ContextGauge context:(context) on-compact:(on_compact)
        });
    }
    let controls = view! {
        row width:max-content shrink:0 margin:(left:{px(12.0)}px) {}
    };
    controls.switch(
        move || status.get(),
        move |parent, status| {
            let (below, on_below, on_pause, on_stop) = (
                below.clone(),
                on_below.clone(),
                on_pause.clone(),
                on_stop.clone(),
            );
            parent.adopt(&view! {
                ControlBlock status:(*status) below:(below) on-below:(on_below) on-pause:(on_pause)
                    paused:(paused) on-stop:(on_stop)
            });
        },
    );
    content.adopt(&controls);
    header.adopt(&content);
    header
}
