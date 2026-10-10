//! Platform window chrome for Relay's existing header cells (issue #11).
use crate::{model::Model, styles::*, theme::*};
use mosaic::prelude::*;
use mosaic::widgets::{CursorIcon, InspectionNode, input::EventCtx};

pub(crate) const NATIVE_HEADER_HEIGHT: f32 = 56.0;

pub(crate) fn window_config() -> WindowConfig {
    #[cfg(target_os = "macos")]
    {
        WindowConfig {
            size: Size::new(1380.0, 900.0),
            macos: MacOsWindowConfig {
                transparent_titlebar: true,
                title_hidden: true,
                full_size_content_view: true,
                traffic_lights_visible: true,
                traffic_light_inset: Some(NATIVE_HEADER_HEIGHT / 2.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        WindowConfig {
            size: Size::new(1380.0, 900.0),
            decorations: WindowDecorations::Client,
            browser_gestures: cfg!(target_arch = "wasm32"),
            ..Default::default()
        }
    }
}

pub(crate) fn sidebar_header_height() -> f32 {
    // Native traffic lights do not scale with Relay's typography. Keep their
    // strip stable so changing interface scale never displaces the buttons.
    if cfg!(target_os = "macos") {
        NATIVE_HEADER_HEIGHT
    } else {
        px(NATIVE_HEADER_HEIGHT)
    }
}

pub(crate) fn sidebar_title_inset() -> f32 {
    if cfg!(target_os = "macos") {
        // Editor example's 78px content inset, adjusted from its 19px native
        // button center to ours at 28px; native geometry stays unscaled.
        87.0 + px(8.0)
    } else {
        px(16.0)
    }
}

/// Follow the hit leaf back to this header. Static text and decorative cells
/// remain drag targets; actual controls (including disabled ones) do not.
pub(crate) fn is_drag_target(
    nodes: &[InspectionNode],
    target: mosaic::layout::NodeId,
    header: mosaic::layout::NodeId,
) -> bool {
    let mut current = Some(target);
    while let Some(id) = current {
        let Some(node) = nodes.iter().find(|node| node.id == id) else {
            return false;
        };
        if matches!(
            node.role,
            Role::Button
                | Role::TextInput
                | Role::Switch
                | Role::Checkbox
                | Role::Radio
                | Role::ColorWell
                | Role::Slider
                | Role::SpinButton
                | Role::ComboBox
        ) {
            return false;
        }
        if id == header {
            return true;
        }
        current = node.parent;
    }
    false
}

#[derive(Clone, Copy)]
pub(crate) enum NativeAction {
    Drag,
    Resize(ResizeDirection),
}

#[derive(Clone, Copy)]
pub(crate) struct NativeGesture {
    pub event: PointerEvent,
    pub action: NativeAction,
}

pub(crate) fn drag_header(model: Model, event: &PointerEvent, ctx: &mut EventCtx) {
    if cfg!(target_arch = "wasm32") || event.kind != PointerEventKind::Down(PointerButton::Primary)
    {
        return;
    }
    let ui = model.ui.get_untracked();
    let Some(target) = ui.hit_test(event.position) else {
        return;
    };
    if !is_drag_target(
        &ui.inspection_snapshot().nodes,
        target.id(),
        ctx.element().id(),
    ) {
        return;
    }
    model.window_gesture.set(Some(NativeGesture {
        event: *event,
        action: NativeAction::Drag,
    }));
    ctx.suppress_click();
}

#[component]
pub(crate) fn WindowControls(model: Model) -> Element {
    view! {
        row width:max-content shrink:0 label:"Window controls" {
            if !cfg!(any(target_os = "macos", target_arch = "wasm32")) {
                WindowButton source:(window_minimize) label:("Minimize window")
                    action:(move || { let _ = model.window.get_untracked().minimize(); })
                WindowButton source:(window_maximize) label:("Maximize or restore window")
                    action:(move || { let _ = model.window.get_untracked().toggle_maximize(); })
                WindowButton source:(window_close) label:("Close window")
                    action:(move || { let _ = model.window.get_untracked().request_close(); })
            }
        }
    }
}

#[component]
fn WindowButton(source: SvgToken, label: &'static str, action: impl Fn() + 'static) -> Element {
    view! {
        button #relay.header-action @click.stop:{(action)()} @pointer-down.stop:{}
            width:{px(32.0)}px shrink:0 pad:0px label:(label) {
            icon size:{px(13.0)}px stroke:ink.fg (source)
        }
    }
}

/// Client decorations remove the platform resize border. Thin edge targets
/// restore native window resizing without resizing Relay's layout elements.
pub(crate) fn install(root: &Element, model: Model) {
    // The native window manager can consume the release of a drag/resize.
    // Cancel Mosaic's capture before handing over, after the pointer handler
    // returns so Cancel never re-enters a borrowed handler. The next click
    // must hit its own control even if no Up event comes back from the OS.
    Effect::new(move || {
        let Some(gesture) = model.window_gesture.get() else {
            return;
        };
        model.window_gesture.set(None);
        model.ui.get_untracked().dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Cancel,
            ..gesture.event
        });
        let window = model.window.get_untracked();
        let _ = match gesture.action {
            NativeAction::Drag => window.start_drag(),
            NativeAction::Resize(direction) => window.start_resize(direction),
        };
    });
    if cfg!(any(target_os = "macos", target_arch = "wasm32")) {
        return;
    }
    let size = State::new(Size::ZERO);
    root.on_layout(move |rect| size.set(rect.size));
    use ResizeDirection::*;
    for (direction, x, y, width, height, cursor) in [
        (
            North,
            Align::Start,
            Align::Start,
            Dimension::Fill,
            Dimension::Px(4.0),
            CursorIcon::ResizeVertical,
        ),
        (
            South,
            Align::Start,
            Align::End,
            Dimension::Fill,
            Dimension::Px(4.0),
            CursorIcon::ResizeVertical,
        ),
        (
            West,
            Align::Start,
            Align::Start,
            Dimension::Px(4.0),
            Dimension::Fill,
            CursorIcon::ResizeHorizontal,
        ),
        (
            East,
            Align::End,
            Align::Start,
            Dimension::Px(4.0),
            Dimension::Fill,
            CursorIcon::ResizeHorizontal,
        ),
        (
            NorthWest,
            Align::Start,
            Align::Start,
            Dimension::Px(10.0),
            Dimension::Px(10.0),
            CursorIcon::ResizeDiagonalDown,
        ),
        (
            NorthEast,
            Align::End,
            Align::Start,
            Dimension::Px(10.0),
            Dimension::Px(10.0),
            CursorIcon::ResizeDiagonalUp,
        ),
        (
            SouthWest,
            Align::Start,
            Align::End,
            Dimension::Px(10.0),
            Dimension::Px(10.0),
            CursorIcon::ResizeDiagonalUp,
        ),
        (
            SouthEast,
            Align::End,
            Align::End,
            Dimension::Px(10.0),
            Dimension::Px(10.0),
            CursorIcon::ResizeDiagonalDown,
        ),
    ] {
        let edge = root.child(Style::stack());
        edge.style_dyn(move || {
            let size = size.get();
            let left = if x == Align::End {
                let Dimension::Px(width) = width else {
                    unreachable!()
                };
                (size.width - width).max(0.0)
            } else {
                0.0
            };
            let top = if y == Align::End {
                let Dimension::Px(height) = height else {
                    unreachable!()
                };
                (size.height - height).max(0.0)
            } else {
                0.0
            };
            Style::stack()
                .width(width)
                .height(height)
                .align_self(Align::Start)
                .margin(Edges {
                    left: left.into(),
                    top: top.into(),
                    ..Edges::ZERO
                })
        });
        edge.label(format!("Resize window {direction:?}"));
        edge.cursor(cursor).on_pointer(move |event, ctx| {
            if event.kind == PointerEventKind::Down(PointerButton::Primary) {
                model.window_gesture.set(Some(NativeGesture {
                    event: *event,
                    action: NativeAction::Resize(direction),
                }));
                ctx.suppress_click();
                ctx.stop_propagation();
            }
        });
    }
}
