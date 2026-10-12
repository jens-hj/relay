//! Cells, rules and the controls that fill them. Hairlines divide the layout
//! into cells; a control fills its cell edge to edge, and neighbouring cells
//! share one rule, never a gap plus a border.

use crate::UiText;
use crate::tokens::{ground, px, status};
use mosaic::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Which edge of a cell carries its dividing rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rule {
    /// No rule.
    #[default]
    None,
    /// A rule on the leading edge: left in a row, top in a column.
    Leading,
    /// A rule on the left edge.
    Left,
    /// A rule on the top edge.
    Top,
    /// A rule on the right edge.
    Right,
    /// A rule on the bottom edge.
    Bottom,
}

#[derive(Clone, Copy)]
enum Axis {
    Row,
    Col,
}

thread_local! {
    /// Ruled containers being built, innermost last, with how many cells
    /// each has received. `None` marks a cell's own content, so cells nested
    /// inside a cell do not take its container's rules.
    static RULING: RefCell<Vec<Option<(Axis, usize)>>> = const { RefCell::new(Vec::new()) };
}

fn with_ruling<R>(frame: Option<Axis>, build: impl FnOnce() -> R) -> R {
    RULING.with(|stack| stack.borrow_mut().push(frame.map(|axis| (axis, 0))));
    let result = build();
    RULING.with(|stack| stack.borrow_mut().pop());
    result
}

/// The edge a new cell's rule goes on: the explicit `rule`, or the leading
/// edge when it follows another cell in a ruled container.
fn take_rule(rule: Rule) -> Option<StrokeEdges> {
    let axis = RULING.with(|stack| {
        let mut stack = stack.borrow_mut();
        match stack.last_mut() {
            Some(Some((axis, count))) => {
                *count += 1;
                (*count > 1).then_some(*axis)
            }
            _ => None,
        }
    });
    match rule {
        Rule::None => axis.map(|axis| match axis {
            Axis::Row => StrokeEdges::LEFT,
            Axis::Col => StrokeEdges::TOP,
        }),
        Rule::Leading => Some(match axis {
            Some(Axis::Col) => StrokeEdges::TOP,
            _ => StrokeEdges::LEFT,
        }),
        Rule::Left => Some(StrokeEdges::LEFT),
        Rule::Top => Some(StrokeEdges::TOP),
        Rule::Right => Some(StrokeEdges::RIGHT),
        Rule::Bottom => Some(StrokeEdges::BOTTOM),
    }
}

/// Adds the rule stroke to `element` (stroke drawn inside the box).
fn rule_visual(edges: Option<StrokeEdges>) -> StrokeSpec {
    let width = if edges.is_some() { px(1.0) } else { 0.0 };
    StrokeSpec::new(width, mosaic::core::theme::color(ground.rule))
        .edges(edges.unwrap_or(StrokeEdges::ALL))
}

/// A row of cells divided by single rules. Every [`Cell`], [`CellButton`]
/// and [`IconCell`] built directly inside it after the first takes a rule
/// on its left edge. Cells added later by reactive children are not
/// counted; give those an explicit `rule`.
#[component]
pub fn RuledRow(
    /// The cells.
    #[prop(optional)]
    children: Children,
) -> Element {
    with_ruling(Some(Axis::Row), || {
        view! {
            row {
                children
            }
        }
    })
}

/// A column of cells divided by single rules on their top edges. See
/// [`RuledRow`].
#[component]
pub fn RuledCol(
    /// The cells.
    #[prop(optional)]
    children: Children,
) -> Element {
    with_ruling(Some(Axis::Col), || {
        view! {
            col {
                children
            }
        }
    })
}

/// A layout cell: content padded 14px horizontally and centred vertically,
/// with the rule its ruled container gives it.
#[component]
pub fn Cell(
    /// An explicit rule edge; by default the ruled container decides.
    #[prop(default = Rule::None)]
    rule: Rule,
    /// The cell's contents.
    #[prop(optional)]
    children: Children,
) -> Element {
    let edges = take_rule(rule);
    with_ruling(None, || {
        view! {
            col min-width:0px justify:center gap:{px(3.0)}px
                pad:(horizontal:{px(14.0)}px vertical:0px) stroke:{rule_visual(edges)} {
                children
            }
        }
    })
}

/// Called when a control is activated.
pub type OnPress = Rc<dyn Fn()>;

/// A control that fills its cell: transparent at rest, a faint wash on
/// hover and press, and an inset ring for keyboard focus.
#[component]
pub fn CellButton(
    /// Accessible name.
    label: UiText,
    /// Activation handler.
    on_press: OnPress,
    /// Disables the control.
    #[prop(default = crate::Live::fixed(false))]
    disabled: crate::Live<bool>,
    /// An explicit rule edge; by default the ruled container decides.
    #[prop(default = Rule::None)]
    rule: Rule,
    /// The visible contents.
    #[prop(optional)]
    children: Children,
) -> Element {
    let edges = take_rule(rule);
    with_ruling(None, || {
        view! {
            button @click:{on_press()} width:fill height:fill min-width:0px align:center
                justify:center gap:{px(8.0)}px pad:(horizontal:{px(14.0)}px vertical:0px) radius:0px
                fill:(Color::TRANSPARENT) font-color:ground.ink stroke:{rule_visual(edges)}
                label:{label.get()} disabled:{disabled.get()} hover { fill:ground.hover }
                pressed { fill:ground.pressed }
                focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                children
            }
        }
    })
}

/// The one strong action of a surface: amber fill, dark text, 30px high.
#[component]
pub fn PrimaryButton(
    /// Visible and accessible text.
    text: UiText,
    /// Activation handler.
    on_press: OnPress,
    #[prop(default = crate::Live::fixed(false))] disabled: crate::Live<bool>,
) -> Element {
    let label = text.clone();
    view! {
        button @click:{on_press()} height:{px(30.0)}px width:max-content shrink:0 align:center
            pad:(horizontal:{px(12.0)}px vertical:0px) radius:0px fill:status.needs-you
            font-color:ground.inverse font-size:{px(12.0)}px font-weight:500 label:{label.get()}
            disabled:{disabled.get()} hover { fill:status.needs-you fill:ground.hover }
            pressed { fill:status.needs-you fill:ground.pressed }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(2.0)}) } {
            text {text.get()}
        }
    }
}

/// A secondary action: a 1px outline, 30px high.
#[component]
pub fn SecondaryButton(
    /// Visible and accessible text.
    text: UiText,
    /// Activation handler.
    on_press: OnPress,
    #[prop(default = crate::Live::fixed(false))] disabled: crate::Live<bool>,
) -> Element {
    let label = text.clone();
    view! {
        button @click:{on_press()} height:{px(30.0)}px width:max-content shrink:0 align:center
            pad:(horizontal:{px(12.0)}px vertical:0px) radius:0px fill:(Color::TRANSPARENT)
            stroke:(width:{px(1.0)} color:ground.outline offset:{px(-1.0)}) font-color:ground.ink
            font-size:{px(12.0)}px font-weight:500 label:{label.get()} disabled:{disabled.get()}
            hover { fill:ground.hover } pressed { fill:ground.pressed }
            focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(2.0)}) } {
            text {text.get()}
        }
    }
}

/// An inline action set as underlined text (offset 3px, rule-strong
/// decoration).
#[component]
pub fn LinkButton(
    /// Visible and accessible text.
    text: UiText,
    /// Activation handler.
    on_press: OnPress,
) -> Element {
    let label = text.clone();
    view! {
        button @click:{on_press()} width:max-content height:min-content shrink:0 radius:0px pad:0px
            fill:(Color::TRANSPARENT) font-color:ground.ink font-size:{px(11.5)}px font-weight:500
            label:{label.get()} hover { fill:(Color::TRANSPARENT) }
            pressed { fill:(Color::TRANSPARENT) }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(2.0)}) } {
            col width:max-content height:min-content gap:{px(2.0)}px {
                text {text.get()}
                el width:fill height:{px(1.0)}px fill:ground.rule-strong {}
            }
        }
    }
}

/// A 32 × 32 outlined icon control.
#[component]
pub fn IconCell(
    /// Accessible name.
    label: UiText,
    /// Activation handler.
    on_press: OnPress,
    /// The icon.
    #[prop(optional)]
    children: Children,
) -> Element {
    with_ruling(None, || {
        view! {
            button @click:{on_press()} width:{px(32.0)}px height:{px(32.0)}px shrink:0 align:center
                justify:center radius:0px pad:0px fill:(Color::TRANSPARENT) font-color:ground.ink
                stroke:(width:{px(1.0)} color:ground.hairline offset:{px(-1.0)}) label:{label.get()}
                hover { fill:ground.hover } pressed { fill:ground.pressed }
                focused { stroke:+(width:{px(1.0)} color:ground.ink offset:{px(-2.0)}) } {
                children
            }
        }
    })
}

/// A small outlined tag, such as a depth chip.
#[component]
pub fn Chip(
    /// The chip text.
    text: UiText,
    /// Text and outline colour; outline grey by default.
    #[prop(optional)]
    tone: Option<ColorToken>,
) -> Element {
    let edge = tone.unwrap_or(ground.outline);
    let ink = tone.unwrap_or(ground.ink);
    view! {
        row width:max-content height:min-content shrink:0 align:center
            pad:(horizontal:{px(7.0)}px vertical:{px(5.0)}px)
            stroke:(width:{px(1.0)} color:(edge) offset:{px(-1.0)}) font-size:{px(11.0)}px
            font-weight:500 font-color:(ink) {
            text {text.get()}
        }
    }
}

/// A keyboard hint, 10px faint ("P", "⇧S").
#[component]
pub fn Kbd(
    /// The keys.
    keys: UiText,
) -> Element {
    view! {
        row width:max-content height:min-content shrink:0 font-size:{px(10.0)}px
            font-color:ground.faint {
            text {keys.get()}
        }
    }
}

/// A free-standing 1px hairline, horizontal by default.
#[component]
pub fn Hairline(
    /// Vertical instead of horizontal.
    #[prop(default = false)]
    vertical: bool,
    /// The rule colour; hairline grey by default.
    #[prop(default = ground.hairline)]
    colour: ColorToken,
) -> Element {
    let line = view! {
        el shrink:0 fill:(colour) {}
    };
    line.style_dyn(move || {
        let style = Style::default().shrink(0.0);
        if vertical {
            style.size(px(1.0), Dimension::Fill)
        } else {
            style.size(Dimension::Fill, px(1.0))
        }
    });
    line
}
