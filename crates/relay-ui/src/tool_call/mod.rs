//! Tool calls, three openings, compact groups, and shared code presentation.
mod body;
mod code;
mod micro;
mod model;
use crate::prelude::*;
use crate::tokens::{Signal, ToolKind, ground, px, status};
pub use body::{WaitLanes, WaitLanesProps};
pub use code::*;
pub use model::*;
use mosaic::prelude::*;
use std::rc::Rc;

pub type OnOpening = Rc<dyn Fn(Opening)>;
/// Application-owned approval policy. No action is performed inside relay-ui.
#[derive(Clone, Default)]
pub struct ToolActions {
    pub approve: Option<OnPress>,
    pub always: Option<OnPress>,
    pub reject: Option<OnPress>,
}

#[component]
pub fn ToolCallRow(
    data: Live<ToolCallView>,
    on_opening: OnOpening,
    #[prop(default = ToolActions::default())] actions: ToolActions,
) -> Element {
    let host = view! {
        col height:min-content min-width:0px {}
    };
    let previous = Rc::new(std::cell::RefCell::new(None::<Element>));
    let focused = Rc::new(std::cell::Cell::new(false));
    let (before, keep) = (previous.clone(), focused.clone());
    host.switch(
        move || {
            let value = data.get();
            keep.set(before.borrow().as_ref().is_some_and(Element::is_focused));
            value
        },
        move |parent, vm| {
            let (row, control) = call(vm.clone(), on_opening.clone(), actions.clone());
            parent.adopt(&row);
            if focused.get() {
                control.focus();
            }
            *previous.borrow_mut() = Some(control);
        },
    );
    host
}
fn call(vm: ToolCallView, on_opening: OnOpening, actions: ToolActions) -> (Element, Element) {
    let colour = vm.kind.token();
    let active = matches!(vm.status, AgentStatus::Working | AgentStatus::Waiting);
    let edge = match vm.status {
        AgentStatus::Working => status.live,
        AgentStatus::Waiting => status.needs_you,
        _ => colour,
    };
    let embedded = opening_indicator(vm.opening);
    let opening = vm.opening;
    let target = match &vm.detail {
        Some(ToolDetail::Message { target, .. }) => {
            vm.target.clone().or_else(|| target.clone()).map(|target| {
                if target.starts_with('→') {
                    target
                } else {
                    format!("→ {target}")
                }
            })
        }
        Some(ToolDetail::Spawn { child, .. }) => vm.target.clone().or_else(|| child.clone()),
        _ => vm.target.clone(),
    }
    .unwrap_or_default();
    let header = view! {
        stack height:{px(36.0)}px shrink:0 clip {
            if active {
                Wash
                    signal:(if vm.status == AgentStatus::Waiting { Signal::NeedsYou } else { Signal::Live })
                    opacity:0.24
            }
            row align:center {
                row width:1fr min-width:0px align:center gap:{px(8.0)}px
                    pad:(left:{px(11.0)}px right:{px(8.0)}px) {
                    StatusSquare status:(vm.status)
                    ToolIcon kind:(vm.kind)
                    Label text:(vm.verb.clone()) role:(TextRole::Label) weight:600
                        tone:(Tone::Colour(colour))
                    col width:1fr min-width:0px height:min-content {
                        Ellipsis text:(target) role:(TextRole::Label)
                    }
                    micro::micrographic(&vm)
                }
                micro::status_cell(&vm)
                { parent.adopt(&embedded); }
            }
        }
    };
    // Hover and press tint the row in the tool's own colour, faintly; read
    // inside the bindings so a theme switch recolours them.
    let tint = move |alpha: f32| {
        let c = mosaic::core::theme::color(colour);
        c.alpha(crate::tokens::srgb_alpha(
            c,
            mosaic::core::theme::color(ground.panel),
            alpha,
        ))
    };
    let control = view! {
        button @click:{on_opening(opening.next())} width:fill height:{px(36.0)}px shrink:0
            radius:0px pad:0px fill:(Color::TRANSPARENT) label:(opening.label())
            hover { fill:{tint(0.08)} } pressed { fill:{tint(0.14)} }
            focused { stroke:+(width:1px color:ground.ink offset:-1px) } {
            { parent.adopt(&header); }
        }
    };
    let content = view! {
        col height:min-content min-width:0px {}
    };
    content.adopt(&control);
    if vm.opening != Opening::Collapsed {
        content.adopt(&body::body(&vm, actions));
    }
    let frame = if vm.kind == ToolKind::Approval {
        view! {
            CutCorner cut:(Cut::Small) edge:(edge) edge-opacity:0.35 {
                { parent.adopt(&content); }
            }
        }
    } else {
        let frame = view! {
            col height:min-content min-width:0px fill:ground.panel {
                { parent.adopt(&content); }
            }
        };
        frame.visual_dyn(move || {
            let c = mosaic::core::theme::color(edge);
            let alpha =
                crate::tokens::srgb_alpha(c, mosaic::core::theme::color(ground.panel), 0.35);
            mosaic::widgets::Visual::new()
                .fill(mosaic::core::theme::color(ground.panel))
                .stroke(StrokeSpec::new(px(1.0), c.alpha(alpha)))
        });
        frame
    };
    (frame, control)
}
fn opening_indicator(opening: Opening) -> Element {
    view! {
        row width:{px(36.0)}px shrink:0 {
            { for i in 0..3 { parent.adopt(&view! {
                el width:1fr height:fill
                    fill:{if i < opening.marks() { mosaic::core::theme::color(ground.outline) } else { Color::TRANSPARENT }}
                    stroke:(width:{px(1.0)} color:ground.hairline edges:left) {}
            }); } }
        }
    }
}
/// A tool icon from the shared SVG scheme, coloured by kind.
#[component]
pub fn ToolIcon(kind: ToolKind) -> Element {
    let host = view! {
        el width:{px(14.0)}px height:{px(14.0)}px shrink:0 font-color:(kind.token()) {}
    };
    let glyph = mosaic::widgets::icon_dyn(
        &host,
        move || tool_icon(kind),
        mosaic::widgets::IconStyle::default(),
    );
    glyph.style_dyn(|| Style::default().size(px(14.0), px(14.0)));
    host
}
fn tool_icon(kind: ToolKind) -> SvgToken {
    use crate::icons::call;
    match kind {
        ToolKind::Read => call.read,
        ToolKind::Edit => call.edit,
        ToolKind::Create => call.create,
        ToolKind::Search => call.search,
        ToolKind::Run => call.run,
        ToolKind::Test => call.test,
        ToolKind::Fetch => call.fetch,
        ToolKind::Plan => call.plan,
        ToolKind::Spawn => call.spawn,
        ToolKind::Message => call.message,
        ToolKind::Wait => call.wait,
        ToolKind::Approval => call.approval,
        ToolKind::Result => call.result,
        ToolKind::Other => call.other,
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct CompactAction {
    pub status: AgentStatus,
    pub verb: String,
    pub target: Option<String>,
    pub info: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ActionGroupView {
    pub actions: Vec<CompactAction>,
    pub expanded: bool,
}
crate::value::live_value!(ActionGroupView);
impl ActionGroupView {
    pub fn all_done(&self) -> bool {
        !self.actions.is_empty() && self.actions.iter().all(|a| a.status == AgentStatus::Done)
    }
    pub fn stays_open(&self) -> bool {
        self.expanded || self.actions.iter().any(|a| a.status == AgentStatus::Failed)
    }
    pub fn caption(&self) -> String {
        format!(
            "{} actions{}",
            self.actions.len(),
            if self.all_done() { " · all done" } else { "" }
        )
    }
}
#[component]
pub fn ActionGroup(data: Live<ActionGroupView>, on_toggle: OnPress) -> Element {
    let host = view! {
        col height:min-content stroke:(width:{px(1.0)} color:ground.hairline) {}
    };
    host.switch(move || data.get(), move |parent, vm| {
              let toggle = on_toggle.clone();
              let expanded = vm.stays_open();
              let header = view! {
                  button @click:{toggle()} width:fill height:{px(34.0)}px align:center justify:start
                      fill:(Color::TRANSPARENT) radius:0px
                      pad:(horizontal:{px(12.0)}px vertical:0px) label:(vm.caption())
                      stroke:(width:{px(if expanded { 1.0 } else { 0.0 })} color:ground.hairline edges:bottom) {
                      Label
                          text:(format!("{} {}", if vm.stays_open() { "⌄" } else { "›" }, vm.caption()))
                          role:(TextRole::Label) weight:500
                  }
              }; parent.adopt(&header);
              if vm.stays_open() { for a in vm.actions.clone() { parent.adopt(&view! {
                  row height:{px(30.0)}px align:center gap:{px(10.0)}px
                      pad:(horizontal:{px(14.0)}px vertical:0px) {
                      StatusSquare status:(a.status)
                      col width:{px(68.0)}px shrink:0 {
                          Label text:(a.verb) role:(TextRole::Label) tone:(Tone::Muted)
                      }
                      col width:1fr min-width:0px height:min-content {
                          Ellipsis text:(a.target.unwrap_or_default()) role:(TextRole::Label)
                      }
                      Label text:(a.info.unwrap_or_default()) role:(TextRole::Caption)
                          tone:(Tone::Muted)
                  }
              }); } }
    });
    host
}
