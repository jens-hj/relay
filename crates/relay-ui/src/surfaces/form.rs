//! Form primitives in ruled cells: a text field, single-choice segments, a
//! stepper, a toggle and a select.

use crate::icons::icon_mark;
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
    /// Accessible name of the input when it should say more than the
    /// visible label.
    #[prop(optional)]
    name: Option<String>,
) -> Element {
    let name = name.unwrap_or_else(|| label.clone());
    let row = view! {
        row height:{px(40.0)}px shrink:0 align:center
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {}
    };
    row.adopt(&label_cell(&label, label_width));
    let input = match editor {
        Some(editor) => editor,
        None => view! {
            input width:1fr height:fill radius:0px pad:(horizontal:{px(14.0)}px vertical:0px)
                fill:(Color::TRANSPARENT) stroke:(width:0px color:ground.rule) font-family:monospace
                font-size:{px(12.5)}px font-color:ground.ink label:(name) placeholder:(placeholder)
                focused { fill:ground.hover } value
        },
    };
    row.adopt(&input);
    row
}

/// Single choice as cells that fill their parent cell: the options divide
/// its width equally and take its full height, separated by single
/// hairlines, with no frame of their own (the parent cell's rules frame
/// them). The chosen cell sits on the selected ground with its text, icon
/// and a 2px bar under it in its tone; the rest are muted.
///
/// Colour follows the status family, one meaning each: a choice that
/// blocks or risks harm (Deny, Unrestricted Access) takes `status.failed`,
/// one that waits on you (Ask) `status.needs-you`, one that lets work run
/// on its own (Allow, Automatic) `status.full-steam`. Neutral choices
/// (theme) stay ink; violet stays reserved for live work.
#[component]
pub fn Segments(
    options: Vec<String>,
    selected: Live<usize>,
    on_select: OnIndex,
    /// Colours the chosen cell, for choices that carry a signal.
    #[prop(optional)]
    chosen_tone: Option<ColorToken>,
    /// The chosen colour per option, overriding `chosen_tone`; missing
    /// entries fall back to it.
    #[prop(optional)]
    tones: Vec<ColorToken>,
    /// An icon per option, left of its text; none when empty.
    #[prop(optional)]
    icons: Vec<SvgToken>,
    /// Names the choice; each cell is then labelled "{name}: {option}".
    #[prop(optional)]
    name: Option<String>,
    #[prop(default = Live::fixed(false))] disabled: Live<bool>,
) -> Element {
    let row = view! {
        row min-width:0px role:list {}
    };
    if let Some(name) = &name {
        row.label(name.clone());
    }
    let count = options.len();
    let cells = Rc::new(std::cell::RefCell::new(Vec::<Element>::new()));
    for (index, option) in options.into_iter().enumerate() {
        let tone = tones
            .get(index)
            .copied()
            .or(chosen_tone)
            .unwrap_or(ground.ink);
        let pick = on_select.clone();
        let current = selected.clone();
        let off = disabled.clone();
        let content = view! {
            row height:1fr min-width:0px align:center justify:center gap:{px(8.0)}px
                pad:(horizontal:{px(10.0)}px vertical:0px) {}
        };
        if let Some(token) = icons.get(index) {
            content.adopt(&icon_mark(*token, 14.0));
        }
        content.adopt(&view! {
            row width:max-content min-width:0px height:min-content clip {
                text text-wrap:none (option.clone())
            }
        });
        let bar = view! {
            el width:fill height:{px(2.0)}px shrink:0
                fill:{if current.get() == index { color(tone) } else { Color::TRANSPARENT }} {}
        };
        let cell = view! {
            button @click:{pick(index)} width:1fr min-width:0px height:fill radius:0px pad:0px
                label:(name.as_ref().map_or_else(|| option.clone(), |name| format!("{name}: {option}")))
                disabled:{off.get()} font-family:monospace font-size:{px(11.5)}px font-weight:500
                disabled { opacity:0.45 } {
                col {
                    { parent.adopt(&content); parent.adopt(&bar); }
                }
            }
        };
        let interaction = cell.interaction();
        let looks = selected.clone();
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
                color(tone)
            } else {
                color(ground.muted)
            }
        });
        cell.role(Role::Radio);
        // Arrow keys move the choice like a radio group.
        {
            let (choose, cells, off) = (on_select.clone(), cells.clone(), disabled.clone());
            cell.on_key(move |event, ctx| {
                if !matches!(event.kind, KeyEventKind::Down { .. }) || off.get_untracked() {
                    return;
                }
                let next = match event.key {
                    Key::ArrowRight | Key::ArrowDown => (index + 1).min(count - 1),
                    Key::ArrowLeft | Key::ArrowUp => index.saturating_sub(1),
                    Key::Home => 0,
                    Key::End => count - 1,
                    _ => return,
                };
                choose(next);
                if let Some(target) = cells.borrow().get(next) {
                    target.focus();
                }
                ctx.stop_propagation();
            });
        }
        cells.borrow_mut().push(cell.clone());
        row.adopt(&cell);
    }
    row
}

/// A bare text input that fills its cell: a hairline frame, an ink ring
/// while focused. Multiline inputs take `height`.
#[component]
pub fn TextInput(
    value: State<String>,
    /// Accessible name.
    label: String,
    #[prop(default = UiText::fixed(String::new()))] placeholder: UiText,
    #[prop(default = false)] multiline: bool,
    /// Height in logical pixels before UI scale; one line by default.
    #[prop(optional)]
    height: Option<f32>,
    /// Draws the hairline frame; off when the input fills a ruled cell.
    #[prop(default = true)]
    framed: bool,
) -> Element {
    let hint = placeholder.clone();
    let edge = if framed { px(1.0) } else { 0.0 };
    let tall = height.unwrap_or(if multiline { 92.0 } else { 36.0 });
    if multiline {
        view! {
            input multiline width:fill height:{px(tall)}px shrink:0 radius:0px pad:{px(10.0)}px
                fill:ground.code stroke:(width:(edge) color:ground.hairline offset:{px(-1.0)})
                font-family:monospace font-size:{px(12.5)}px font-color:ground.ink label:(label)
                placeholder:{hint.get()}
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } value
        }
    } else {
        view! {
            input width:fill height:{px(tall)}px shrink:0 radius:0px
                pad:(horizontal:{px(12.0)}px vertical:0px) fill:ground.code
                stroke:(width:(edge) color:ground.hairline offset:{px(-1.0)}) font-family:monospace
                font-size:{px(12.5)}px font-color:ground.ink label:(label) placeholder:{hint.get()}
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } value
        }
    }
}

/// A number with − and + cells, clamped to `min..=max`. It fills its
/// parent cell: the − and + cells are square-ish ends at the cell's full
/// height, the value takes the rest, and one hairline divides each pair;
/// the parent cell's own rules frame it.
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
    /// Accessible names of the − and + cells.
    #[prop(default = "Decrease".to_string())]
    decrease_label: String,
    #[prop(default = "Increase".to_string())] increase_label: String,
    /// A unit after the value ("%"); hidden when empty.
    #[prop(optional)]
    suffix: String,
) -> Element {
    let (down, up) = (value.clone(), value.clone());
    let (lower, raise) = (on_change.clone(), on_change);
    let decrease: OnPress =
        Rc::new(move || lower((down.get_untracked().saturating_sub(step)).clamp(min, max)));
    let increase: OnPress =
        Rc::new(move || raise((up.get_untracked().saturating_add(step)).clamp(min, max)));
    let shown = value.clone();
    let named = value.clone();
    // The value is a spin button: Up and Down step it.
    let (keyed_down, keyed_up) = (decrease.clone(), increase.clone());
    let spin = view! {
        row width:max-content height:min-content justify:center label:(label.clone())
            description:{named.get().to_string()} {
            text font-family:monospace font-size:{px(12.5)}px font-weight:500
                {format!("{}{suffix}", shown.get())}
        }
    };
    spin.role(Role::SpinButton);
    spin.focusable(true);
    spin.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        match event.key {
            Key::ArrowUp | Key::ArrowRight => keyed_up(),
            Key::ArrowDown | Key::ArrowLeft => keyed_down(),
            _ => return,
        }
        ctx.stop_propagation();
    });
    view! {
        row min-width:0px {
            row width:{px(40.0)}px shrink:0 {
                CellButton label:(decrease_label) on-press:(decrease) horizontal-inset:(0.0) {
                    text font-family:monospace font-size:{px(13.0)}px "−"
                }
            }
            row width:1fr min-width:0px align:center justify:center
                stroke:(width:{px(1.0)} color:ground.hairline edges:(left right)) {
                { parent.adopt(&spin); }
            }
            row width:{px(40.0)}px shrink:0 {
                CellButton label:(increase_label) on-press:(increase) horizontal-inset:(0.0) {
                    text font-family:monospace font-size:{px(13.0)}px "+"
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

/// One choice from a list: a control filling its parent cell (no frame of
/// its own) showing the current option and a chevron, opening a popover
/// list.
#[component]
pub fn Select(
    label: String,
    options: Vec<String>,
    selected: Live<usize>,
    on_select: OnIndex,
    /// Width of the option list in logical pixels before UI scale; the
    /// control itself fills its cell.
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
        stack min-width:0px {
            button @click:{toggle()} width:fill height:fill radius:0px
                pad:(horizontal:{px(12.0)}px vertical:0px) gap:{px(8.0)}px align:center
                fill:(Color::TRANSPARENT) label:(label.clone()) hover { fill:ground.hover }
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
