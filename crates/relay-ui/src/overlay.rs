//! Anchored overlays: popovers that hold controls, and tooltips that only
//! inform.

use crate::tokens::{ground, px};
use crate::{UiText, attached};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::widgets::{
    OverlayAlign, OverlayPlacement, OverlaySide, TooltipOptions, Visual, bind_anchored_overlay,
};
use std::rc::Rc;

/// Builds a popover's content each time it opens.
pub type PopoverContent = Rc<dyn Fn() -> Element>;

/// A panel anchored to the area this node covers, drawn above everything
/// and escaping ancestor clips. Place it in a `stack` over its trigger: it
/// fills the stack without hit-testing, so the trigger stays clickable.
///
/// Escape closes it, and so does a press outside it and outside the
/// trigger area when a [`Ui`] is attached (see [`crate::install`]). The
/// panel takes focus when it opens.
#[component]
pub fn Popover(
    /// Whether the panel is open; the popover clears it on dismissal.
    open: State<bool>,
    /// Accessible name of the panel.
    label: UiText,
    /// Builds the panel's content.
    content: PopoverContent,
    /// Side of the anchor the panel opens on.
    #[prop(default = OverlaySide::Bottom)]
    side: OverlaySide,
    /// Alignment along that side.
    #[prop(default = OverlayAlign::Start)]
    align: OverlayAlign,
    /// Takes focus back when Escape closes the panel, usually the control
    /// that opened it.
    #[prop(optional)]
    trigger: Option<Element>,
    /// Gap between anchor and panel in logical pixels.
    #[prop(default = 6.0)]
    gap: f32,
) -> Element {
    let anchor = view! {
        stack nohit {}
    };
    let anchor_handle = anchor.clone();
    anchor.show(
        move || open.get(),
        move |host| {
            let panel = host.child(
                Style::column()
                    .width(Dimension::MaxContent)
                    .height(Dimension::MinContent)
                    .shrink(0.0),
            );
            panel.visual_dyn(|| {
                Visual::new()
                    .fill(color(ground.panel_raised))
                    .stroke(StrokeSpec::new(px(1.0), color(ground.rule)).offset(-px(1.0)))
            });
            panel.role(mosaic::widgets::Role::Group);
            panel.label(label.get_untracked());
            panel.hit_testable(true);
            panel.focusable(true);
            panel.adopt(&content());
            bind_anchored_overlay(&anchor_handle, &panel, move || {
                OverlayPlacement::default()
                    .side(side)
                    .align(align)
                    .gap(px(gap))
            });
            panel.on_pointer(|event, ctx| {
                if matches!(
                    event.kind,
                    PointerEventKind::Down(_) | PointerEventKind::Click(_)
                ) {
                    ctx.stop_propagation();
                }
            });
            let trigger = trigger.clone();
            panel.on_key(move |event, ctx| {
                if matches!(event.kind, KeyEventKind::Down { .. }) && event.key == Key::Escape {
                    open.set(false);
                    if let Some(trigger) = &trigger {
                        trigger.focus();
                    }
                    ctx.stop_propagation();
                }
            });
            if let Some(ui) = attached() {
                let trigger = anchor_handle.clone();
                ui.root().on_pointer_for(&panel, move |event, _| {
                    if matches!(event.kind, PointerEventKind::Down(PointerButton::Primary))
                        && !trigger.layout_rect().contains(event.position)
                        && open.get_untracked()
                    {
                        open.set(false);
                    }
                });
            }
            panel.focus();
        },
    );
    anchor
}

/// An informational tooltip on the content it wraps: shown on hover or
/// keyboard focus, never hit-tested. Wraps its children in a stack that
/// hugs them; size it like any node.
#[component]
pub fn Tooltip(
    /// The tooltip text; also the anchor's accessible description.
    text: UiText,
    /// The anchor content.
    #[prop(optional)]
    children: Children,
) -> Element {
    let anchor = view! {
        stack width:min-content height:min-content shrink:0 {
            children
        }
    };
    let summary = text.get_untracked();
    mosaic::widgets::tooltip(&anchor, summary, TooltipOptions::default(), move |root| {
        root.hit_testable(false);
        root.style(
            Style::column()
                .width(Dimension::MaxContent)
                .height(Dimension::MaxContent)
                .shrink(0.0)
                .padding(Edges::symmetric(px(9.0), px(7.0))),
        );
        root.visual_dyn(|| {
            Visual::new()
                .fill(color(ground.panel_raised))
                .stroke(StrokeSpec::new(px(1.0), color(ground.rule)).offset(-px(1.0)))
        });
        let text = text.clone();
        root.text_leaf_dyn(
            move || text.get(),
            TextStyle::new(px(11.5))
                .family(FontFamily::Monospace)
                .color(color(ground.muted)),
        );
    });
    anchor
}
