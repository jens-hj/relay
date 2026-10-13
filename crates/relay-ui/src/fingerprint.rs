//! The fingerprint strip: a 4px line splitting every tool call of a
//! session by tool type, in the tools' colours, sized by count.

use crate::prelude::*;
use crate::tokens::{ToolKind, ground, px};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::widgets::{
    OverlayAlign, OverlayPlacement, OverlaySide, TooltipOptions, TooltipTrigger, Visual,
};
use std::time::Duration;

/// Calls of one tool type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToolCount {
    pub kind: ToolKind,
    pub count: u32,
}

crate::value::live_value!(Vec<ToolCount>);

/// Share of all calls, rounded to a whole percent.
pub fn share(count: u32, total: u32) -> u32 {
    if total == 0 {
        0
    } else {
        (count as f32 * 100.0 / total as f32).round() as u32
    }
}

/// "Read: 18 calls, 26 percent".
pub fn segment_label(tool: ToolCount, total: u32) -> String {
    let calls = if tool.count == 1 { "call" } else { "calls" };
    format!(
        "{}: {} {calls}, {} percent",
        tool.kind.name(),
        tool.count,
        share(tool.count, total)
    )
}

/// A segment's bar height and opacity: raised to 10px and opaque while it
/// is hovered or focused, dimmed to 0.45 while another one is, 4px at 0.85
/// at rest.
pub fn segment_look(active: bool, other_active: bool) -> (f32, f32) {
    if active {
        (10.0, 1.0)
    } else if other_active {
        (4.0, 0.45)
    } else {
        (4.0, 0.85)
    }
}

/// The strip. Segments keep the caller's order and grow by count with a
/// 2px gap; each is a button, and hovering or focusing it raises it, dims
/// the rest, and shows its tool, count and share.
#[component]
pub fn FingerprintStrip(tools: Live<Vec<ToolCount>>) -> Element {
    let strip = view! {
        row height:{px(14.0)}px shrink:0 gap:{px(2.0)}px align:start role:list
            label:"Tool calls by type" {}
    };
    let hovered = State::new(None::<ToolKind>);
    let focused = State::new(None::<ToolKind>);
    strip.switch(
        move || tools.get(),
        move |parent, tools| {
            let total: u32 = tools.iter().map(|tool| tool.count).sum();
            for &tool in tools.iter().filter(|tool| tool.count > 0) {
                parent.adopt(&segment(tool, total, hovered, focused));
            }
        },
    );
    strip
}

fn segment(
    tool: ToolCount,
    total: u32,
    hovered: State<Option<ToolKind>>,
    focused: State<Option<ToolKind>>,
) -> Element {
    let kind = tool.kind;
    let token = kind.token();
    let active = move || hovered.get() == Some(kind) || focused.get() == Some(kind);
    let other = move || {
        let lit = hovered.get().or(focused.get());
        lit.is_some() && lit != Some(kind)
    };
    let host = view! {
        el height:{px(14.0)}px min-width:{px(3.0)}px role:button
            label:(segment_label(tool, total)) {}
    };
    let weight = tool.count as f32;
    host.restyle_dyn(move |style| style.width(Dimension::Fr(weight)).min_width(px(3.0)));
    host.focusable(true);
    let bar = view! {
        el width:fill nohit {}
    };
    bar.style_dyn(move || {
        let (height, _) = segment_look(active(), other());
        Style::default().size(Dimension::Fill, px(height))
    });
    bar.visual_dyn(move || {
        let (_, opacity) = segment_look(active(), other());
        let tint = color(token);
        Visual::new().fill(tint.alpha(tint.a * opacity))
    });
    host.adopt(&bar);
    {
        let interaction = host.interaction();
        Effect::new(move || {
            let over = interaction.hovered();
            let now = hovered.get_untracked();
            if over {
                hovered.set(Some(kind));
            } else if now == Some(kind) {
                hovered.set(None);
            }
        });
        Effect::new(move || {
            let on = interaction.focused();
            let now = focused.get_untracked();
            if on {
                focused.set(Some(kind));
            } else if now == Some(kind) {
                focused.set(None);
            }
        });
    }
    let open = State::new(false);
    Effect::new(move || open.set(active()));
    let detail = format!(
        "{} {} · {}%",
        tool.count,
        if tool.count == 1 { "call" } else { "calls" },
        share(tool.count, total)
    );
    let options = TooltipOptions::default()
        .trigger(TooltipTrigger::Manual)
        .open(open)
        .show_delay(Duration::ZERO)
        .hide_delay(Duration::ZERO)
        .placement(
            OverlayPlacement::default()
                .side(OverlaySide::Bottom)
                .align(OverlayAlign::Start)
                .gap(2.0),
        );
    mosaic::widgets::tooltip(&host, segment_label(tool, total), options, move |root| {
        root.hit_testable(false);
        root.style(
            Style::row()
                .width(Dimension::MaxContent)
                .height(Dimension::MaxContent)
                .shrink(0.0)
                .align_items(Align::Center)
                .gap(px(8.0))
                .padding(Edges::symmetric(px(9.0), px(7.0))),
        );
        root.visual_dyn(move || {
            Visual::new()
                .fill(color(ground.panel_raised))
                .stroke(StrokeSpec::new(px(1.0), color(token)).offset(-px(1.0)))
        });
        let name = kind.name().to_string();
        let detail = detail.clone();
        root.adopt(&view! {
            el width:{px(7.0)}px height:{px(7.0)}px shrink:0 fill:(token) {}
        });
        root.adopt(&view! {
            text font-family:monospace font-size:{px(11.5)}px font-weight:600 font-color:(token)
                (name)
        });
        root.adopt(&view! {
            text font-family:monospace font-size:{px(11.5)}px font-color:ground.muted (detail)
        });
    });
    host
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mount;

    fn session() -> Vec<ToolCount> {
        [
            (ToolKind::Read, 18),
            (ToolKind::Search, 9),
            (ToolKind::Edit, 14),
            (ToolKind::Create, 3),
            (ToolKind::Run, 7),
            (ToolKind::Plan, 0),
        ]
        .into_iter()
        .map(|(kind, count)| ToolCount { kind, count })
        .collect()
    }

    #[test]
    fn shares_and_looks() {
        assert_eq!(share(18, 69), 26);
        assert_eq!(share(0, 0), 0);
        assert_eq!(
            segment_label(
                ToolCount {
                    kind: ToolKind::Read,
                    count: 18
                },
                69
            ),
            "Read: 18 calls, 26 percent"
        );
        assert_eq!(segment_look(true, false), (10.0, 1.0));
        assert_eq!(segment_look(false, true), (4.0, 0.45));
        assert_eq!(segment_look(false, false), (4.0, 0.85));
    }

    #[test]
    fn segments_grow_by_count_and_raise_on_hover() {
        let mounted = mount(Size::new(800.0, 200.0), || {
            view! {
                col width:{560.0}px {
                    FingerprintStrip tools:(session())
                }
            }
        });
        let total = 51;
        let read = mounted.rect(&format!("Read: 18 calls, {} percent", share(18, total)));
        let create = mounted.rect(&format!("Create: 3 calls, {} percent", share(3, total)));
        assert!(
            read.size.width > create.size.width * 5.0,
            "{read:?} {create:?}"
        );
        assert_eq!(read.size.height, 14.0);
        assert!(
            !mounted
                .labels()
                .iter()
                .any(|label| label.starts_with("Plan"))
        );
        mounted.hover(read.center());
        assert!(mounted.has("18 calls · 35%"), "{:?}", mounted.labels());
    }
}
