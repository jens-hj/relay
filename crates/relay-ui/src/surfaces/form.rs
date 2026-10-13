//! Form primitives in ruled cells: a text field, single-choice segments, a
//! stepper, a toggle and a select.

use crate::marks::{Mark, mark};
use crate::prelude::*;
use crate::tokens::{ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::widgets::{Role, Visual};
use std::rc::Rc;

/// Called with a chosen index.
pub type OnIndex = Rc<dyn Fn(usize)>;
/// Called with a new number.
pub type OnNumber = Rc<dyn Fn(i64)>;
/// Called with a new on/off value.
pub type OnSwitch = Rc<dyn Fn(bool)>;

crate::value::live_value!(i64);

/// The label cell every field row starts with.
fn label_cell(label: &str, width: f32) -> Element {
    let label = label.to_string();
    view! {
        row width:{px(width)}px shrink:0 align:center pad:(horizontal:{px(14.0)}px vertical:0px)
            stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
            Ellipsis text:(label) role:(TextRole::Label) tone:(Tone::Muted)
        }
    }
}

/// A labelled text field in a ruled 40px row. Pass `editor` to embed
/// another editing element (a code or buffer editor) in place of the plain
/// input.
#[component]
pub fn Field(
    label: String,
    value: State<String>,
    #[prop(optional)] placeholder: String,
    /// Width of the label cell in logical pixels before UI scale.
    #[prop(default = 140.0)]
    label_width: f32,
    #[prop(optional)] editor: Option<Element>,
) -> Element {
    let row = view! {
        row height:{px(40.0)}px shrink:0 align:center label:(label.clone())
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    row.adopt(&label_cell(&label, label_width));
    let input = match editor {
        Some(editor) => editor,
        None => view! {
            input width:1fr height:fill radius:0px pad:(horizontal:{px(14.0)}px vertical:0px)
                fill:(Color::TRANSPARENT) stroke:(width:0px color:ground.rule) font-family:monospace
                font-size:{px(12.5)}px font-color:ground.ink label:(label.clone())
                placeholder:(placeholder) focused { fill:ground.hover } value
        },
    };
    row.adopt(&input);
    row
}

/// Single choice as ruled cells; the chosen cell is highlighted in ink
/// over the selected ground, the rest muted.
#[component]
pub fn Segments(
    options: Vec<String>,
    selected: Live<usize>,
    on_select: OnIndex,
    /// Colours the chosen cell's text, for choices that carry a signal.
    #[prop(optional)]
    chosen_tone: Option<ColorToken>,
) -> Element {
    let row = view! {
        row width:max-content height:{px(32.0)}px shrink:0 role:list
            stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {}
    };
    for (index, option) in options.into_iter().enumerate() {
        let pick = on_select.clone();
        let shown_text = option.clone();
        let current = selected.clone();
        let looks = selected.clone();
        let cell = view! {
            button @click:{pick(index)} width:max-content height:fill radius:0px
                pad:(horizontal:{px(12.0)}px vertical:0px) label:(option.clone())
                font-family:monospace font-size:{px(11.5)}px font-weight:500 {
                text
                    {
                    let _ = current.get();
                    shown_text.clone()
                }
            }
        };
        let interaction = cell.interaction();
        cell.visual_dyn(move || {
            let chosen = looks.get() == index;
            let fill = if chosen {
                color(ground.selected)
            } else if interaction.pressed() {
                color(ground.pressed)
            } else if interaction.hovered() {
                color(ground.hover)
            } else {
                Color::TRANSPARENT
            };
            let mut visual = Visual::new().fill(fill);
            if index > 0 {
                visual = visual.stroke(
                    StrokeSpec::new(px(1.0), color(ground.hairline)).edges(StrokeEdges::LEFT),
                );
            }
            if interaction.focus_visible() {
                visual =
                    visual.stroke(StrokeSpec::new(px(1.0), color(ground.ink)).offset(-px(1.0)));
            }
            visual
        });
        let tone_of = selected.clone();
        cell.font_color_dyn(move || {
            if tone_of.get() == index {
                color(chosen_tone.unwrap_or(ground.ink))
            } else {
                color(ground.muted)
            }
        });
        cell.role(Role::Radio);
        row.adopt(&cell);
    }
    row
}

/// A number with − and + cells, clamped to `min..=max`.
#[component]
pub fn Stepper(
    value: Live<i64>,
    on_change: OnNumber,
    #[prop(default = i64::MIN)] min: i64,
    #[prop(default = i64::MAX)] max: i64,
    #[prop(default = 1)] step: i64,
    /// Accessible name of the value.
    #[prop(default = "Value".to_string())]
    label: String,
) -> Element {
    let (down, up) = (value.clone(), value.clone());
    let (lower, raise) = (on_change.clone(), on_change);
    let decrease: OnPress =
        Rc::new(move || lower((down.get_untracked().saturating_sub(step)).clamp(min, max)));
    let increase: OnPress =
        Rc::new(move || raise((up.get_untracked().saturating_add(step)).clamp(min, max)));
    let shown = value.clone();
    let named = value.clone();
    view! {
        row width:max-content height:{px(32.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) {
            row width:max-content {
                row width:{px(32.0)}px shrink:0 {
                    CellButton label:"Decrease" on-press:(decrease) {
                        text font-family:monospace font-size:{px(13.0)}px "−"
                    }
                }
                Cell rule:(Rule::Left) {
                    row width:{px(48.0)}px height:min-content justify:center role:label
                        label:{format!("{label} {}", named.get())} {
                        text font-family:monospace font-size:{px(12.5)}px font-weight:500
                            {shown.get().to_string()}
                    }
                }
                row width:{px(32.0)}px shrink:0 {
                    CellButton label:"Increase" on-press:(increase) rule:(Rule::Left) {
                        text font-family:monospace font-size:{px(13.0)}px "+"
                    }
                }
            }
        }
    }
}

/// An on/off switch: a square track with a square knob, ink when on.
#[component]
pub fn Toggle(label: String, on: Live<bool>, on_toggle: OnSwitch) -> Element {
    let flip = on.clone();
    let painted = on.clone();
    let track = view! {
        el width:{px(26.0)}px height:{px(14.0)}px shrink:0 nohit {}
    };
    {
        let target = track.clone();
        let watched = on.clone();
        Effect::new(move || {
            let _ = watched.get();
            target.paint_dirty();
        });
    }
    track.paint(move |ctx| {
        let r = ctx.rect;
        let unit = r.size.height / 14.0;
        let lit = painted.get_untracked();
        let edge = color(if lit { ground.ink } else { ground.outline });
        ctx.scene.shape(
            mosaic::render::Shape::new(r)
                .stroke(mosaic::render::Stroke::new(unit, edge).offset(-unit / 2.0)),
        );
        let x = if lit {
            r.origin.x + r.size.width - 12.0 * unit
        } else {
            r.origin.x + 2.0 * unit
        };
        let knob = Rect::from_xywh(x, r.origin.y + 2.0 * unit, 10.0 * unit, 10.0 * unit);
        let fill = color(if lit { ground.ink } else { ground.muted });
        ctx.scene.shape(mosaic::render::Shape::new(knob).fill(fill));
    });
    let text = label.clone();
    let host = view! {
        button @click:{on_toggle(!flip.get_untracked())} width:max-content height:{px(32.0)}px
            radius:0px pad:(horizontal:{px(4.0)}px vertical:0px) gap:{px(10.0)}px align:center
            fill:(Color::TRANSPARENT) label:(label) hover { fill:ground.hover }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
            {
                parent.adopt(&track);
            }
            text font-family:monospace font-size:{px(12.0)}px font-color:ground.ink (text)
        }
    };
    host.role(Role::Switch);
    {
        let target = host.clone();
        let state = on.clone();
        Effect::new(move || {
            target.toggled(state.get());
        });
    }
    host
}

/// One choice from a list: a ruled cell showing the current option and a
/// chevron, opening a popover list.
#[component]
pub fn Select(
    label: String,
    options: Vec<String>,
    selected: Live<usize>,
    on_select: OnIndex,
    /// Width in logical pixels before UI scale.
    #[prop(default = 200.0)]
    width: f32,
) -> Element {
    let open = State::new(false);
    let shown = selected.clone();
    let names = options.clone();
    let content: PopoverContent = {
        let options = options.clone();
        let pick = on_select.clone();
        let current = selected.clone();
        Rc::new(move || {
            let list = view! {
                col width:{px(width)}px height:min-content role:list {}
            };
            for (index, option) in options.iter().cloned().enumerate() {
                let pick = pick.clone();
                let press: OnPress = Rc::new(move || {
                    pick(index);
                    open.set(false);
                });
                let chosen = current.get_untracked() == index;
                let tone = if chosen { Tone::Ink } else { Tone::Muted };
                list.adopt(&view! {
                    col height:{px(32.0)}px shrink:0 {
                        CellButton label:(option.clone()) on-press:(press) rule:(Rule::Top) {
                            row height:min-content {
                                Label text:(option.clone()) role:(TextRole::Label) tone:(tone)
                                    weight:500
                            }
                        }
                    }
                });
            }
            list
        })
    };
    let toggle = move || open.set(!open.get_untracked());
    let host = view! {
        stack width:{px(width)}px height:{px(32.0)}px shrink:0 {
            button @click:{toggle()} width:fill height:fill radius:0px
                pad:(horizontal:{px(12.0)}px vertical:0px) gap:{px(8.0)}px align:center
                fill:(Color::TRANSPARENT) label:(label.clone())
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)})
                hover { fill:ground.hover }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                row width:1fr min-width:0px height:min-content {
                    text font-family:monospace font-size:{px(12.0)}px font-color:ground.ink
                        {names.get(shown.get()).cloned().unwrap_or_default()}
                }
                {
                    parent.adopt(&mark(Mark::ChevronDown, 12.0, ground.muted));
                }
            }
            Popover open:(open) label:(label) content:(content) gap:(0.0)
        }
    };
    host
}
