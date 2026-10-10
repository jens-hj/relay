//! Narrow-window navigation overlays the workspace without rebuilding it.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn drawer_width(viewport: f32) -> f32 {
    (viewport - 48.0).clamp(0.0, 320.0)
}

#[component]
pub(crate) fn Drawer(model: Model, width: State<f32>, drag: State<Option<f32>>) -> Element {
    let ui = model.ui.get_untracked();
    let previous_focus = ui.focused();
    let view = view! {
        stack label:"Navigation drawer" {
            button @click:{model.sidebar_open.set(false);} width:fill height:fill fill:scrim
                radius:0px pad:0px label:"Dismiss navigation" ""
            col width:{drawer_width(width.get())}px place:start clip
                translate:(x:{drag.get().unwrap_or_else(|| drawer_width(width.get())) - drawer_width(width.get())}px) {
                Sidebar model:(model) viewport:(width)
            }
        }
    };
    let id = view.id();
    view.on_key(move |event, ctx| {
        let ui = model.ui.get_untracked();
        if matches!(event.kind, KeyEventKind::Down { .. }) && event.key == Key::Tab {
            let snapshot = ui.inspection_snapshot();
            for _ in 0..snapshot.nodes.len() {
                if event.modifiers.shift {
                    ui.focus_prev();
                } else {
                    ui.focus_next();
                }
                let mut ancestor = ui.focused().map(|element| element.id());
                while let Some(current) = ancestor {
                    if current == id {
                        break;
                    }
                    ancestor = snapshot.node(current).and_then(|node| node.parent);
                }
                if ancestor.is_some() {
                    break;
                }
            }
            if let Some(focused) = ui.focused() {
                focused.reveal();
            }
            ctx.stop_propagation();
        }
    });
    view.__hot_on_remove(move || {
        if let Some(previous) = previous_focus
            && ui.inspection_snapshot().contains(previous.id())
        {
            previous.focus();
        }
    });
    view
}

#[derive(Clone, Copy)]
struct Swipe {
    start: Vector2,
    open: bool,
    claimed: bool,
}

pub(crate) fn install(root: &Element, model: Model, width: State<f32>, drag: State<Option<f32>>) {
    let swipe = Rc::new(RefCell::new(None::<Swipe>));
    root.on_pointer(move |event, ctx| {
        if !model.mobile_navigation.get_untracked() || event.pointer_type != PointerType::Touch {
            return;
        }
        let mut candidate = swipe.borrow_mut();
        let extent = drawer_width(width.get_untracked());
        let left = model.ui.get_untracked().safe_area().left;
        match event.kind {
            PointerEventKind::Down(PointerButton::Primary) => {
                let open = model.sidebar_open.get_untracked();
                // Only the edge opens navigation. Swipes elsewhere belong to
                // the board/transcript; the open drawer can be swiped closed.
                *candidate = (event.position.x >= left
                    && event.position.x <= left + if open { extent } else { 28.0 }
                    && !model.palette.get_untracked())
                .then_some(Swipe {
                    start: event.position,
                    open,
                    claimed: false,
                });
            }
            PointerEventKind::Move => {
                let Some(mut gesture) = *candidate else {
                    return;
                };
                let delta = event.position - gesture.start;
                if !gesture.claimed {
                    if delta.y.abs() > 12.0 && delta.y.abs() >= delta.x.abs() {
                        *candidate = None;
                        return;
                    }
                    let towards_drawer = if gesture.open {
                        delta.x < -12.0
                    } else {
                        delta.x > 12.0
                    };
                    if !towards_drawer || delta.x.abs() < delta.y.abs() * 1.5 {
                        return;
                    }
                    gesture.claimed = true;
                    *candidate = Some(gesture);
                    // Cancel the original control press before taking the
                    // sequence. Lifting after a swipe must never activate it.
                    ctx.claim_pointer();
                    ctx.suppress_click();
                }
                drag.set(Some(
                    (if gesture.open { extent } else { 0.0 } + delta.x).clamp(0.0, extent),
                ));
                ctx.stop_propagation();
            }
            PointerEventKind::Up(PointerButton::Primary) => {
                if let Some(gesture) = candidate.take()
                    && gesture.claimed
                {
                    let progress = drag.get_untracked().unwrap_or_default();
                    model
                        .sidebar_open
                        .set(progress >= extent * if gesture.open { 0.65 } else { 0.35 });
                    drag.set(None);
                    ctx.suppress_click();
                    ctx.stop_propagation();
                }
            }
            PointerEventKind::Cancel => {
                *candidate = None;
                drag.set(None);
            }
            _ => {}
        }
    });
}
