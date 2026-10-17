//! Tree rows: the agent row, the folded branch line, the child status bar
//! and the square elbows.

use super::logic::{AgentLine, FoldedLine, Guide, StatusTally, TreeLine, status_segments};
use super::{OnAgent, OnToggle};
use crate::overlay::{Tooltip, TooltipProps};
use crate::prelude::*;
use crate::tokens::{Signal, ground, px, shape};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use mosaic::widgets::Role;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Row height in logical pixels.
pub(crate) const ROW_HEIGHT: f32 = 34.0;
/// Width of one elbow column.
pub(crate) const ELBOW_WIDTH: f32 = 14.0;

/// Paints one elbow column: a 1px rule at x 6.5, with a tick to the right
/// edge at mid-height for the line's own column.
fn paint_guide(scene: &mut mosaic::render::Scene, rect: Rect, guide: Guide) {
    let unit = rect.size.width / ELBOW_WIDTH;
    let tint = color(ground.hairline);
    let x = rect.origin.x + 6.0 * unit;
    let mid = rect.origin.y + rect.size.height / 2.0;
    let line = |scene: &mut mosaic::render::Scene, r: Rect| {
        scene.shape(mosaic::render::Shape::new(r).fill(tint));
    };
    match guide {
        Guide::Blank => {}
        Guide::Pass => line(
            scene,
            Rect::from_xywh(x, rect.origin.y, unit, rect.size.height),
        ),
        Guide::Branch | Guide::Last => {
            let bottom = if guide == Guide::Branch {
                rect.size.height
            } else {
                rect.size.height / 2.0 + unit / 2.0
            };
            line(scene, Rect::from_xywh(x, rect.origin.y, unit, bottom));
            line(
                scene,
                Rect::from_xywh(x, mid - unit / 2.0, rect.size.width - 6.0 * unit, unit),
            );
        }
    }
}

/// The elbow columns of a line, each the `indent` token (14px) wide and
/// as tall as the row. They sit flush in one group, so each level
/// indents by exactly one column; the row's gap follows the group.
pub(crate) fn elbow_cells(parent: &Element, guides: &[Guide]) {
    if guides.is_empty() {
        return;
    }
    let columns = guides.to_vec();
    let count = columns.len();
    let group = view! {
        el width:{px(mosaic::core::theme::scalar(shape.indent) * count as f32)}px
            height:{px(ROW_HEIGHT)}px shrink:0 nohit {}
    };
    group.paint(move |ctx| {
        let width = ctx.rect.size.width / count as f32;
        for (index, guide) in columns.iter().copied().enumerate() {
            paint_guide(
                ctx.scene,
                Rect::from_xywh(
                    ctx.rect.origin.x + index as f32 * width,
                    ctx.rect.origin.y,
                    width,
                    ctx.rect.size.height,
                ),
                guide,
            );
        }
    });
    parent.adopt(&group);
}

/// Square elbows for a line at any depth: one 14px column per level.
#[component]
pub fn Elbows(
    /// Columns, outermost first.
    guides: Vec<Guide>,
) -> Element {
    let host = view! {
        row width:max-content height:{px(ROW_HEIGHT)}px shrink:0 nohit {}
    };
    elbow_cells(&host, &guides);
    host
}

/// The colour of a child status bar segment: violet live, amber waiting,
/// red failed, then two greys for queued and done.
pub fn segment_token(status: AgentStatus) -> ColorToken {
    match status {
        AgentStatus::Working => Signal::Live.token(),
        AgentStatus::Waiting => Signal::NeedsYou.token(),
        AgentStatus::Failed => Signal::Failed.token(),
        AgentStatus::Queued => ground.rule_strong,
        AgentStatus::Idle | AgentStatus::Done => ground.outline,
    }
}

/// "2 working, 1 waiting, 1 failed".
pub fn tally_label(tally: &StatusTally) -> String {
    let parts: Vec<String> = AgentStatus::ALL
        .into_iter()
        .filter(|status| tally.get(*status) > 0)
        .map(|status| format!("{} {}", tally.get(status), status.word()))
        .collect();
    if parts.is_empty() {
        "No agents".to_string()
    } else {
        parts.join(", ")
    }
}

/// Everything inside a collapsed manager as one 48 × 4 bar, a segment per
/// state sized by count.
#[component]
pub fn ChildStatusBar(
    tally: StatusTally,
    /// Bar length in logical pixels before UI scale.
    #[prop(default = 48.0)]
    length: f32,
) -> Element {
    let bar = view! {
        el width:{px(length)}px height:{px(4.0)}px shrink:0 role:image
            label:(tally_label(&tally)) {}
    };
    bar.paint(move |ctx| {
        let unit = ctx.rect.size.width / length;
        for segment in status_segments(&tally, length) {
            let rect = Rect::from_xywh(
                ctx.rect.origin.x + segment.x * unit,
                ctx.rect.origin.y,
                segment.width * unit,
                ctx.rect.size.height,
            );
            ctx.scene
                .shape(mosaic::render::Shape::new(rect).fill(color(segment_token(segment.status))));
        }
    });
    bar
}

/// Keyboard navigation shared by the rows of one tree: arrow keys move
/// between rows, and focus survives a row being rebuilt.
#[derive(Default)]
pub struct TreeNav {
    lines: RefCell<Vec<TreeLine>>,
    rows: RefCell<HashMap<String, Element>>,
    focused: RefCell<Option<String>>,
}

impl TreeNav {
    pub fn new() -> Rc<Self> {
        Rc::new(TreeNav::default())
    }

    /// Replaces the lines the keys move over.
    pub fn set_lines(&self, lines: Vec<TreeLine>) {
        *self.lines.borrow_mut() = lines;
    }

    fn register(&self, id: &str, row: &Element) {
        self.rows.borrow_mut().insert(id.to_string(), row.clone());
        let restore = self.focused.borrow().as_deref() == Some(id);
        let free = crate::attached().is_none_or(|ui| ui.focused().is_none());
        if restore && free {
            row.focus();
        }
    }

    fn focus(&self, id: &str) -> bool {
        let row = self.rows.borrow().get(id).cloned();
        match row {
            Some(row) => {
                if let Some(ui) = crate::attached() {
                    ui.set_focus_navigating(true);
                }
                row.focus();
                true
            }
            None => false,
        }
    }

    fn step(&self, id: &str, forward: bool) -> bool {
        let ids: Vec<String> = self
            .lines
            .borrow()
            .iter()
            .filter_map(|line| match line {
                TreeLine::Agent(agent) => Some(agent.id.clone()),
                TreeLine::Folded(_) => None,
            })
            .collect();
        let Some(index) = ids.iter().position(|each| each == id) else {
            return false;
        };
        let next = if forward {
            ids.get(index + 1)
        } else {
            index.checked_sub(1).and_then(|i| ids.get(i))
        };
        next.is_some_and(|next| self.focus(next))
    }
}

/// One agent: expand caret, status, harness, name, child count, then the
/// model and its effort steps, or for a collapsed manager the waiting count
/// and a status bar of everything inside. Working rows carry a model-colored wash
/// that reaches as far as the effort; waiting rows an amber wash.
///
/// The row is focusable: Enter opens it, Right expands and Left collapses;
/// with a [`TreeNav`], Up and Down move between rows, Left on a closed row
/// goes to its parent and Right on an open one to its first child.
#[component]
pub fn AgentTreeRow(
    line: AgentLine,
    /// Hide the glyph when a transcript has no measured harness identity.
    #[prop(default = true)]
    show_harness: bool,
    /// Opens (selects) the agent.
    #[prop(optional)]
    on_open: Option<OnAgent>,
    /// Expands or collapses the agent's children.
    #[prop(optional)]
    on_toggle: Option<OnToggle>,
    /// Creates a child of this agent.
    #[prop(optional)]
    on_spawn: Option<OnAgent>,
    #[prop(optional)] on_edit: Option<OnAgent>,
    #[prop(optional)] on_archive: Option<OnAgent>,
    #[prop(optional)] on_delete: Option<OnAgent>,
    #[prop(default = Live::fixed(false))] can_archive: Live<bool>,
    /// Keep overflowing actions within the tree's vertical viewport.
    #[prop(optional)]
    action_viewport: Option<State<Rect>>,
    /// Arrow-key navigation across the tree this row belongs to.
    #[prop(optional)]
    nav: Option<Rc<TreeNav>>,
    /// The ground under the row, for the wash blend.
    #[prop(default = ground.panel)]
    over: ColorToken,
) -> Element {
    let under = if line.selected { ground.selected } else { over };
    let row = view! {
        stack height:{px(ROW_HEIGHT)}px shrink:0 clip label:(line.name.clone())
            fill:{if line.selected { color(ground.selected) } else { Color::TRANSPARENT }}
            hover { fill:ground.hover }
            focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {}
    };
    if line.archived {
        row.opacity(0.45);
    }
    row.role(Role::ListItem);
    row.focusable(true);
    match line.status {
        AgentStatus::Working => {
            let level = line.effort.map_or(0, |effort| effort.level());
            row.adopt(&view! {
                Wash reach:(live_reach(level)) over:(under)
                    colour:(line.model.as_ref().map(|model| model.hue.token()))
            });
        }
        AgentStatus::Waiting => {
            row.adopt(&view! {
                Wash signal:(Signal::NeedsYou) over:(under)
            });
        }
        _ => {}
    }
    // Elbows sit flush before the gapped content, so every level indents
    // by exactly one column.
    let outer = view! {
        row align:center pad:(left:{px(12.0)}px right:{px(14.0)}px) {}
    };
    elbow_cells(&outer, &line.guides);
    let content = view! {
        row width:1fr min-width:0px gap:{px(8.0)}px align:center {}
    };
    outer.adopt(&content);
    if line.expandable {
        content.adopt(&caret(&line, on_toggle.clone()));
    } else {
        content.adopt(&view! {
            el width:{px(12.0)}px height:{px(12.0)}px shrink:0 {}
        });
    }
    let name_tone = match line.status {
        AgentStatus::Idle | AgentStatus::Done => Tone::Muted,
        AgentStatus::Waiting => Tone::Signal(Signal::NeedsYou),
        _ => Tone::Ink,
    };
    let weight = if line.selected {
        Weight::SEMIBOLD
    } else {
        Weight::MEDIUM
    };
    content.adopt(&view! {
        row width:max-content height:min-content gap:{px(8.0)}px align:center shrink:0
            font-color:ground.muted {
            StatusSquare status:(line.status)
            if show_harness {
                HarnessGlyph harness:(line.harness)
            }
        }
    });
    content.adopt(&crate::marks::hugging_ellipsis(
        line.name.clone(),
        TextRole::Body,
        name_tone,
        weight,
    ));
    if line.child_count > 0 {
        content.adopt(&caption(
            line.child_count.to_string(),
            10.5,
            ground.muted,
            400,
        ));
    }
    content.adopt(&view! {
        el width:1fr min-width:{px(8.0)}px height:{px(1.0)}px {}
    });
    if line.shows_summary() {
        let waiting = line.below.waiting;
        if waiting > 0 {
            content.adopt(&caption(
                format!("{waiting} waiting"),
                11.0,
                Signal::NeedsYou.token(),
                500,
            ));
        }
        content.adopt(&view! {
            ChildStatusBar tally:(line.below)
        });
    } else if let Some(model) = &line.model {
        content.adopt(&caption(model.short_name(), 11.0, ground.muted, 400));
        if let Some(effort) = line.effort {
            content.adopt(&view! {
                EffortTrack levels:(effort.levels) current:(effort.track_step())
                    tiers:(effort.tiers) colour:(model.hue.token()) size:(TrackSize::Compact)
                    orientation:(Orientation::Vertical)
            });
        }
    }
    row.adopt(&outer);
    let actions = view! {
        row width:0px height:{px(ROW_HEIGHT)}px place:end align:center
            label:(format!("Actions for {}", line.name)) {}
    };
    // Escape the scroll viewport and sidebar clips without taking row space.
    actions.overlay(true);
    if let Some(viewport) = action_viewport {
        let bounds = State::new(Rect::default());
        row.on_layout(move |rect| bounds.set(rect));
        let actions = actions.clone();
        Effect::new(move || {
            let row = bounds.get();
            let viewport = viewport.get();
            // Hide partial rows too: controls must never cover the header or footer.
            let visible = row.size.height > 0.0
                && row.origin.y >= viewport.origin.y
                && row.max_y() <= viewport.max_y();
            actions.opacity(if visible { 1.0 } else { 0.0 });
            actions.hit_testable(visible);
        });
    }
    // Hidden overflow descendants still participate in the row's hover path.
    // Arm only inside the visible row, then retain the full strip until exit.
    let pointer_revealed = State::new(false);
    let interaction = row.interaction();
    Effect::new(move || {
        if !interaction.hovered() {
            pointer_revealed.set(false);
        }
    });
    let pointer_row = row.clone();
    row.on_pointer(move |event, _| {
        if matches!(
            event.kind,
            PointerEventKind::Move | PointerEventKind::Down(_)
        ) && pointer_row.layout_rect().contains(event.position)
            && action_viewport
                .is_none_or(|viewport| viewport.get_untracked().contains(event.position))
        {
            pointer_revealed.set(true);
        }
    });
    if let Some(spawn) = on_spawn.filter(|_| !line.archived) {
        let id = line.id.clone();
        let action = view! {
            button @click:{spawn(id.clone())} width:{px(ROW_HEIGHT)}px height:{px(ROW_HEIGHT)}px
                shrink:0 align:center justify:center pad:0px radius:0px fill:(under)
                disabled:{!line.can_spawn} label:(format!("New worker for {}", line.name))
                hover { fill:ground.panel-raised }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                { parent.adopt(&crate::marks::mark(crate::marks::Mark::Plus, 12.0, ground.ink)); }
            }
        };
        let visible = reveal_row_action(pointer_revealed.into(), &action, Live::fixed(false));
        let host = view! {
            Tooltip
                text:(if line.can_spawn { "New Worker" } else { "New Worker · start this agent first" }) {
                { parent.adopt(&action); }
            }
        };
        let hit_host = host.clone();
        Effect::new(move || {
            hit_host.hit_testable(visible.get());
        });
        actions.adopt(&host);
    }

    if let Some(edit) = on_edit {
        let id = line.id.clone();
        let action = view! {
            button @click:{edit(id.clone())} width:{px(ROW_HEIGHT)}px height:{px(ROW_HEIGHT)}px
                shrink:0 align:center justify:center pad:0px radius:0px fill:(under)
                label:(format!("Edit {}", line.name)) hover { fill:ground.panel-raised }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                { parent.adopt(&crate::icons::icon_mark(crate::icons::mark.edit, 16.0)); }
            }
        };
        let visible = reveal_row_action(pointer_revealed.into(), &action, Live::fixed(false));
        let host = view! {
            Tooltip text:"Edit Profile" {
                { parent.adopt(&action); }
            }
        };
        let hit_host = host.clone();
        Effect::new(move || {
            hit_host.hit_testable(visible.get());
        });
        actions.adopt(&host);
    }

    if let Some(archive) = on_archive.filter(|_| !line.archived) {
        let id = line.id.clone();
        let name = line.name.clone();
        let host = view! {
            row width:max-content height:min-content shrink:0 {}
        };
        host.show(move || can_archive.get(), move |parent| {
            let archive = archive.clone();
            let id = id.clone();
            let name = name.clone();
            let action = view! {
                button @click:{archive(id.clone())} width:{px(ROW_HEIGHT)}px
                    height:{px(ROW_HEIGHT)}px shrink:0 align:center justify:center pad:0px
                    radius:0px fill:(under) label:(format!("Archive {name}"))
                    hover { fill:ground.panel-raised }
                    focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                    { parent.adopt(&crate::marks::mark(crate::marks::Mark::Archive, 12.0, ground.ink)); }
                }
            };
            // Reconcile the hover bindings with this conditional button.
            let scope = Scope::new(|| {});
            let visible = scope.run(|| reveal_row_action(pointer_revealed.into(), &action, Live::fixed(false)));
            action.on_exit(move |_, done| { scope.dispose(); done.finish(); });
            let tooltip = view! {
                Tooltip text:"Archive Branch" {
                    { parent.adopt(&action); }
                }
            };
            let hit_host = tooltip.clone();
            scope.run(|| Effect::new(move || { hit_host.hit_testable(visible.get()); }));
            parent.adopt(&tooltip);
        });
        actions.adopt(&host);
    }

    if let Some(delete) = on_delete.filter(|_| line.archived || line.can_delete) {
        let confirm = State::new(false);
        let id = line.id.clone();
        let name = line.name.clone();
        let action = view! {
            button
                @click:{if confirm.get_untracked() { delete(id.clone()); } else { confirm.set(true); }}
                width:{px(ROW_HEIGHT)}px height:{px(ROW_HEIGHT)}px shrink:0 align:center
                justify:center pad:0px radius:0px fill:(under)
                label:{if confirm.get() { format!("Confirm deletion of {name}") } else { format!("Delete {name}") }}
                hover { fill:ground.panel-raised }
                focused { stroke:(width:{px(1.0)} color:ground.ink offset:{px(-1.0)}) } {
                if confirm.get() {
                    { parent.adopt(&crate::marks::mark(crate::marks::Mark::Check, 12.0, ground.ink)); }
                } else {
                    { parent.adopt(&crate::marks::mark(crate::marks::Mark::Trash, 12.0, ground.ink)); }
                }
            }
        };
        let visible = reveal_row_action(pointer_revealed.into(), &action, confirm.into());
        let host = view! {
            Tooltip text:"Delete Branch Permanently (Two Clicks)" {
                { parent.adopt(&action); }
            }
        };
        let hit_host = host.clone();
        Effect::new(move || {
            hit_host.hit_testable(visible.get());
        });
        actions.adopt(&host);
    }
    row.adopt(&actions);

    {
        let open = on_open.clone();
        let id = line.id.clone();
        row.on_pointer(move |event, ctx| match event.kind {
            PointerEventKind::Down(PointerButton::Primary) => ctx.request_focus(),
            PointerEventKind::Click(PointerButton::Primary) => {
                if let Some(open) = &open {
                    open(id.clone());
                }
            }
            _ => {}
        });
    }
    {
        let line = line.clone();
        let nav = nav.clone();
        row.on_key(move |event, ctx| {
            if !matches!(event.kind, KeyEventKind::Down { .. }) {
                return;
            }
            let handled = row_key(&line, &event.key, &on_open, &on_toggle, nav.as_deref());
            if handled {
                ctx.stop_propagation();
            }
        });
    }
    if let Some(nav) = &nav {
        let id = line.id.clone();
        let tracker = nav.clone();
        row.on_focus_change(move |focused| {
            let mut slot = tracker.focused.borrow_mut();
            if focused {
                *slot = Some(id.clone());
            } else if slot.as_deref() == Some(id.as_str()) {
                *slot = None;
            }
        });
        nav.register(&line.id, &row);
    }
    row
}

/// Applies one key to a row; true when it was used.
fn row_key(
    line: &AgentLine,
    key: &Key,
    on_open: &Option<OnAgent>,
    on_toggle: &Option<OnToggle>,
    nav: Option<&TreeNav>,
) -> bool {
    match key {
        Key::Enter | Key::Space => match on_open {
            Some(open) => {
                open(line.id.clone());
                true
            }
            None => false,
        },
        Key::ArrowRight if line.expandable && !line.expanded => match on_toggle {
            Some(toggle) => {
                toggle(line.id.clone(), true);
                true
            }
            None => false,
        },
        Key::ArrowRight => nav.is_some_and(|nav| line.expanded && nav.step(&line.id, true)),
        Key::ArrowLeft if line.expandable && line.expanded => match on_toggle {
            Some(toggle) => {
                toggle(line.id.clone(), false);
                true
            }
            None => false,
        },
        Key::ArrowLeft => match (nav, &line.parent) {
            (Some(nav), Some(parent)) => nav.focus(parent),
            _ => false,
        },
        Key::ArrowDown => nav.is_some_and(|nav| nav.step(&line.id, true)),
        Key::ArrowUp => nav.is_some_and(|nav| nav.step(&line.id, false)),
        _ => false,
    }
}

/// The 12 × 34 expand/collapse control: a muted triangle pointing down
/// when open, right when closed.
fn caret(line: &AgentLine, on_toggle: Option<OnToggle>) -> Element {
    let expanded = line.expanded;
    let id = line.id.clone();
    let label = if expanded { "Collapse" } else { "Expand" };
    let button = view! {
        el width:{px(12.0)}px height:{px(ROW_HEIGHT)}px shrink:0 role:button label:(label) {}
    };
    button.paint(move |ctx| {
        let unit = ctx.rect.size.width / 12.0;
        let centre = ctx.rect.center();
        let local = |x: f32, y: f32| Vector2::new((x - 6.0) * unit, (y - 6.0) * unit);
        let points = if expanded {
            vec![local(2.5, 4.5), local(9.5, 4.5), local(6.0, 8.5)]
        } else {
            vec![local(4.5, 2.5), local(8.5, 6.0), local(4.5, 9.5)]
        };
        let bounds = Rect::from_xywh(
            centre.x - 6.0 * unit,
            centre.y - 6.0 * unit,
            12.0 * unit,
            12.0 * unit,
        );
        ctx.scene.shape(
            mosaic::render::Shape::new(bounds)
                .path(points, true)
                .fill(color(ground.muted)),
        );
    });
    button.on_pointer(move |event, ctx| {
        if matches!(event.kind, PointerEventKind::Click(PointerButton::Primary)) {
            if let Some(toggle) = &on_toggle {
                toggle(id.clone(), !expanded);
            }
            ctx.stop_propagation();
        }
    });
    button
}

/// A single line of mono text at an exact size, weight and colour.
pub(crate) fn caption(text: String, size: f32, tone: ColorToken, weight: u16) -> Element {
    view! {
        text font-family:monospace font-size:{px(size)}px font-weight:(weight) font-color:(tone)
            shrink:0 (text)
    }
}

/// A branch folded past the fold depth: "3 more levels · 6 agents", its
/// status bar, and a Focus link that re-roots the tree on it.
#[component]
pub fn FoldedBranch(
    line: FoldedLine,
    /// Focuses the folded branch's parent.
    #[prop(optional)]
    on_focus: Option<OnAgent>,
) -> Element {
    let outer = view! {
        row height:{px(ROW_HEIGHT)}px shrink:0 align:center
            pad:(left:{px(12.0)}px right:{px(14.0)}px) {}
    };
    elbow_cells(&outer, &line.guides);
    let content = view! {
        row width:1fr min-width:0px gap:{px(8.0)}px align:center {}
    };
    outer.adopt(&content);
    let parent = line.parent.clone();
    let focus: crate::cells::OnPress = Rc::new(move || {
        if let Some(focus) = &on_focus {
            focus(parent.clone());
        }
    });
    content.adopt(&view! {
        Ellipsis text:(line.text()) role:(TextRole::Label) tone:(Tone::Muted)
    });
    content.adopt(&view! {
        ChildStatusBar tally:(line.tally)
    });
    content.adopt(&view! {
        LinkButton text:"Focus" on-press:(focus)
    });
    outer
}

// Row focus follows pointer selection; only the action's own focus reveals it.
// Preserve a press or pending confirmation after the pointer leaves.
fn reveal_row_action(
    pointer_revealed: Live<bool>,
    action: &Element,
    confirming: Live<bool>,
) -> Derived<bool> {
    let button_interaction = action.interaction();
    let visible = Derived::new(move || {
        pointer_revealed.get()
            || button_interaction.focused()
            || button_interaction.pressed()
            || confirming.get()
    });
    action.opacity(0.0);
    let fade = Rc::new(std::cell::Cell::new(0.0f32));
    let destination = Rc::new(std::cell::Cell::new(0.0f32));
    let animating = Rc::new(std::cell::Cell::new(false));
    let animated = action.clone();
    Effect::new(move || {
        destination.set(if visible.get() { 1.0 } else { 0.0 });
        if animating.replace(true) {
            return;
        }
        let (fade, destination, animating, target) = (
            fade.clone(),
            destination.clone(),
            animating.clone(),
            animated.clone(),
        );
        animated.animate(move |dt| {
            let distance = destination.get() - fade.get();
            let step = dt.as_secs_f32() / 0.12;
            let next = fade.get() + distance.signum() * step.min(distance.abs());
            fade.set(next);
            target.opacity(next);
            if (destination.get() - next).abs() < 0.001 {
                animating.set(false);
                mosaic::widgets::Tick::Done
            } else {
                mosaic::widgets::Tick::Sleep(std::time::Duration::from_millis(16))
            }
        });
    });
    let hit = action.clone();
    Effect::new(move || {
        hit.hit_testable(visible.get());
    });
    action.on_pointer(move |event, ctx| {
        if matches!(
            event.kind,
            PointerEventKind::Down(_) | PointerEventKind::Click(_)
        ) {
            ctx.stop_propagation();
        }
    });
    visible
}
