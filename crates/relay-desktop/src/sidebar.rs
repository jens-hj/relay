use crate::styles::*;
use crate::{
    controls::button,
    labels::{RunState, SlotMeter, SlotMeterProps, StatusGlyph, StatusGlyphProps},
    model::{EditTarget, Model, Page},
    theme::*,
};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use relay_core::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TreeItem {
    Project(String),
    Director(String),
    Worker(String),
}

type TreeFocus = Rc<RefCell<BTreeMap<TreeItem, Element>>>;

// Sidebar hints contain only information. Mosaic also supports interactive
// tooltips, whose default hit testing would cover nearby navigation rows.
fn tooltip(
    anchor: &Element,
    summary: impl Into<String>,
    options: TooltipOptions,
    build: impl Fn(&Element) + 'static,
) -> mosaic::widgets::Tooltip {
    mosaic::widgets::tooltip(
        anchor,
        summary,
        options.trigger(TooltipTrigger::Hover),
        move |root| {
            root.hit_testable(false);
            build(root);
        },
    )
}

#[component]
pub fn Sidebar(model: Model, viewport: State<f32>) -> Element {
    let edges = State::new(ResizeEdges::RIGHT);
    let focus: TreeFocus = Rc::default();
    view! {
        col width:{px(model.preferences.get().sidebar_width)}px min-width:{px(160.0)}
            max-width:{(viewport.get()*0.4).max(px(160.0)).min(px(360.0))} fill:surface.sidebar
            pad:(horizontal:{px(10.0)}px vertical:{px(14.0)}px) gap:{px(18.0)}px shrink:0 clip
            resizable:($edges) label:"Sidebar"
            @resize:{ move |event: &ResizeEvent, _| if event.phase == ResizePhase::End {
                model.preferences.update(|p| p.sidebar_width = (event.size.width / p.scale).clamp(160.0, 360.0));
            } } {
            col height:min-content pad:(horizontal:{px(6.0)}px vertical:0px) gap:{px(20.0)}px
                shrink:0 {
                text font-size:{px(26.0)}px font-weight:750 font-family:sans-serif font-color:ink.fg
                    "Relay"
                row height:min-content justify:between align:center pad:(bottom:{px(6.0)}px)
                    stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                    text font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
                        letter-spacing:{px(0.6)}px "Projects"
                    text font-size:{px(11.0)}px font-color:{color(ink.muted)}
                        {format!("{:02}", model.snapshot.get().projects.len())}
                }
            }
            scroll {
                col height:min-content gap:{px(6.0)}px role:list label:"Project agents" {
                    for (_, project) in {model.snapshot.get().projects.into_iter().map(|p| (p.id.clone(), p))} {
                        col height:min-content {
                            ProjectTree model:(model) project-id:(project.id.clone())
                                focus:(focus.clone())
                        }
                    }
                    button #relay.tree-control @click:{model.page.set(Page::NewProject);} width:fill
                        gap:{px(8.0)}px pad:(horizontal:{px(4.0)}px vertical:0px)
                        label:"New Project" {
                        icon size:{px(13.0)}px plus-icon
                        row #relay.tree-label {
                            text text-wrap:none font-size:{px(12.0)}px "New Project"
                        }
                    }
                }
            }
            row height:{px(40.0)}px gap:{px(10.0)}px align:center shrink:0
                stroke:(width:{px(1.0)} color:rule.hair edges:top) pad:(top:{px(6.0)}px) {
                button #relay.tree-control @click:{model.page.set(Page::Settings);}
                    width:{px(32.0)}px label:"Settings"
                    fill:{if model.page.get() == Page::Settings {color(ink.inverse)} else {Color::TRANSPARENT}}
                    font-color:{color(if model.page.get() == Page::Settings {ink.on_inverse} else {ink.muted})} {
                    icon size:{px(18.0)}px gear-icon
                    tooltip #relay.tooltip summary:"Settings" {text "Settings · Ctrl/Cmd+,"}
                }
                row width:1fr height:min-content align:center gap:{px(6.0)}px {
                    el width:{px(7.0)}px height:{px(7.0)}px shrink:0
                        fill:if model.connected.get() {status.success} else {status.danger} {}
                    text font-size:{px(11.0)}px font-color:{color(ink.muted)} text-wrap:none
                        {if model.connected.get() {"Connected"} else if model.status.get().starts_with("Connecting") {"Connecting…"} else {"Offline"}}
                    tooltip #relay.tooltip summary:"Server connection" side:top {
                        text font-size:{px(12.0)}px {model.status.get()}
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectTree(model: Model, project_id: String, focus: TreeFocus) -> Element {
    let id = State::new(project_id);
    let menu = State::new(false);
    let name = Derived::new(move || {
        model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == id.get())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    });
    let open = Derived::new(move || model.expanded_projects.get().contains(&id.get()));
    view! {
        col height:min-content gap:{px(3.0)}px {
            row #relay.tree-row height:{px(34.0)}px gap:{px(2.0)}px align:center role:list-item
                label:{format!("Project row {}",name.get())} pad:(left:{px(4.0)}px) {
                button #relay.tree-control @click:{toggle(model.expanded_projects, id.get_untracked());}
                    width:1fr shrink:1 gap:{px(8.0)}px pad:(horizontal:{px(4.0)}px vertical:0px)
                    label:{format!("Toggle project {}", name.get())}
                    description:{if open.get() {"Expanded"} else {"Collapsed"}}
                    font-color:ink.fg
                    {
                    icon size:{px(13.0)}px shrink:0 {if open.get() {tree_chevron_down} else {tree_chevron_right}}
                    row #relay.tree-label {
                        text text-wrap:none font-family:{FontFamily::SansSerif} font-weight:{650} font-size:{px(14.0)}px {name.get()}
                    }
                    tooltip #relay.tooltip summary:"Expand or collapse project" {text {name.get()}}
                }
                    as navigation
                {bind_tree_key(model, navigation, TreeItem::Project(id.get_untracked()), focus.clone());}
                button #relay.tree-control @click:{model.select_project(id.get_untracked());}
                    width:{px(28.0)}px label:{format!("Open board for {}", name.get())}
                    fill:{if model.project.get() == id.get() && model.page.get() == Page::Board {color(ink.inverse)} else {Color::TRANSPARENT}}
                    font-color:{color(if model.project.get() == id.get() && model.page.get() == Page::Board {ink.on_inverse} else {ink.muted})}
                    {
                    icon size:{px(16.0)}px board-icon
                    tooltip #relay.tooltip summary:"Open project board" {text "Open project board"}
                }
                button #relay.tree-control @click:{menu.set(!menu.get_untracked());}
                    width:{px(24.0)}px label:{format!("Project actions for {}",name.get())} {
                    text "⋯"
                }
            }
            if menu.get() {
                col height:min-content {
                    button #relay.action
                        @click:{model.select_project(id.get_untracked());model.page.set(Page::Connections);menu.set(false);}
                        label:{format!("Project Connections for {}",name.get())}
                        "Project Connections"
                }
            }
            if open.get() {
                row height:min-content {
                    el width:{px(14.0)}px shrink:0 {}
                    col height:min-content gap:{px(2.0)}px
                        stroke:(width:{px(1.0)} color:rule.hair edges:left) {
                        let focus = focus.clone();
                        for (_, director) in {model.snapshot.get().directors.into_iter().filter(|d| d.project_id == id.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>()} {
                            col height:min-content {
                                DirectorTree model:(model) director-id:(director.id.clone())
                                    focus:(focus.clone())
                            }
                        }
                        row height:{px(32.0)}px align:center gap:{px(2.0)}px
                            pad:(left:{px(4.0)}px) {
                            button #relay.tree-control
                                @click:{model.select_project(id.get_untracked()); model.open_profile(EditTarget::New);}
                                width:1fr shrink:1 gap:{px(6.0)}px
                                label:{format!("Create director in {}", name.get())}
                                {
                                el width:{px(24.0)}px height:fill align:center justify:center {icon size:{px(13.0)}px plus-icon}
                                row #relay.tree-label {
                                    text text-wrap:none font-size:{px(11.0)}px "New Director"
                                }
                                tooltip #relay.tooltip summary:"Create director" {text "Create director"}
                            }
                            button #relay.tree-control
                                @click:{model.select_project(id.get_untracked()); model.open_profile(EditTarget::Defaults);}
                                width:{px(24.0)}px
                                label:{format!("Project defaults for {}", name.get())}
                                {
                                icon size:{px(14.0)}px sliders-icon
                                tooltip #relay.tooltip summary:"Project defaults" {text "Project defaults"}
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DirectorTree(model: Model, director_id: String, focus: TreeFocus) -> Element {
    let id = State::new(director_id);
    let director = Derived::new(move || {
        model
            .snapshot
            .get()
            .directors
            .into_iter()
            .find(|d| d.id == id.get())
    });
    let name = Derived::new(move || director.get().map(|d| d.name).unwrap_or_default());
    let open = Derived::new(move || model.expanded_directors.get().contains(&id.get()));
    let capacity =
        Derived::new(move || crate::labels::director_capacity(&model.snapshot.get(), &id.get()));
    let row_width = State::new(0.0f32);
    // Zed Mono has a fixed advance, so the label width is predictable: only
    // draw the capacity meter when it fits beside the full name.
    let meter_fits = Derived::new(move || {
        let (_, _, limit) = capacity.get();
        let meter_width = limit.min(16) as f32 * px(8.0) + if limit > 16 { px(28.0) } else { 0.0 };
        let name_width = name.get().chars().count() as f32 * px(12.0) * 0.6;
        limit > 0
            && row_width.get()
                >= px(4.0 + 24.0 + 15.0 + 6.0 + 24.0 + 12.0) + name_width + meter_width
    });
    let workers = Derived::new(move || {
        let snapshot = model.snapshot.get();
        snapshot
            .sessions
            .into_iter()
            .filter(|s| {
                s.director_id == id.get()
                    && s.role == SessionRole::Worker
                    && director.get().is_some_and(|d| s.project_id == d.project_id)
            })
            .collect::<Vec<_>>()
    });
    let selected = Derived::new(move || {
        let snapshot = model.snapshot.get();
        (model.page.get() == Page::DirectorStart && model.worker_director.get() == id.get())
            || (model.page.get() == Page::Directors
                && model.editor.get() == EditTarget::Director(id.get()))
            || (model.page.get() == Page::Sessions
                && snapshot.sessions.iter().any(|s| {
                    s.id == model.session.get()
                        && s.director_id == id.get()
                        && (s.role == SessionRole::Director || !open.get())
                }))
    });
    view! {
        col height:min-content gap:{px(2.0)}px {
            row #relay.tree-row height:{px(34.0)}px gap:{px(2.0)}px align:center role:list-item
                label:{format!("Director row {}",name.get())}
                @layout:{move |rect: Rect| row_width.set(rect.size.width)}
                pad:(left:{px(4.0)}px right:{px(2.0)}px)
                fill:{if selected.get() {color(surface.selected)} else {Color::TRANSPARENT}}
                stroke:(width:{px(if selected.get() {2.0} else {0.0})} color:ink.fg edges:left)
                hover { fill:surface.raised } {
                button #relay.tree-control @click:{toggle(model.expanded_directors, id.get_untracked());}
                    width:{px(24.0)}px label:{format!("Toggle director {}", name.get())}
                    description:{if open.get() {"Expanded"} else {"Collapsed"}}
                    {
                    icon size:{px(13.0)}px {if open.get() {tree_chevron_down} else {tree_chevron_right}}
                    tooltip #relay.tooltip summary:"Expand or collapse workers" {text "Expand or collapse workers"}
                }
                button #relay.tree-control @click:{model.open_director(id.get_untracked());} width:1fr
                    shrink:1 gap:{px(6.0)}px label:{format!("Open director {}", name.get())}
                    description:{format!("{} · {}", if model.snapshot.get().sessions.iter().any(|s| s.director_id == id.get() && s.role == SessionRole::Director) {"Open director conversation"} else {"Send first director prompt"}, capacity_label(capacity.get()))}
                    font-color:ink.fg
                    {
                    icon size:{px(15.0)}px shrink:0 director-icon
                    row #relay.tree-label {
                        text width:max-content text-wrap:none font-size:{px(12.0)}px font-color:{color(ink.fg)} {name.get()}
                    }
                    tooltip #relay.tooltip summary:"Director" {
                        col height:min-content gap:{px(4.0)}px {
                            text {name.get()}
                            text font-color:{color(ink.muted)} font-size:{px(11.0)}px
                                {if model.snapshot.get().sessions.iter().any(|s| s.director_id == id.get() && s.role == SessionRole::Director) {"Open director conversation"} else {"Send first director prompt"}}
                        }
                    }
                }
                    as navigation
                {bind_tree_key(model, navigation, TreeItem::Director(id.get_untracked()), focus.clone());}
                if meter_fits.get() {
                    SlotMeter running:(Derived::new(move || capacity.get().0))
                        active:(Derived::new(move || capacity.get().1))
                        limit:(Derived::new(move || capacity.get().2))
                }
                button #relay.tree-control @click:{model.open_director_profile(id.get_untracked());}
                    width:{px(24.0)}px label:{format!("Profile for {}", name.get())}
                    {
                    icon size:{px(14.0)}px sliders-icon
                    tooltip #relay.tooltip summary:"Director profile" {text "Director profile"}
                }
            }
            if open.get() {
                row height:min-content {
                    el width:{px(16.0)}px shrink:0 {}
                    col height:min-content gap:{px(2.0)}px pad:(left:{px(20.0)}px)
                        stroke:(width:{px(1.0)} color:rule.hair edges:left) {
                        let focus = focus.clone();
                        for (_, worker) in {workers.get().into_iter().map(|s| (s.id.clone(), s))} {
                            col height:min-content {
                                WorkerTree model:(model) session-id:(worker.id.clone())
                                    focus:(focus.clone())
                            }
                        }
                        if workers.get().is_empty() {
                            row height:{px(28.0)}px pad:(left:{px(6.0)}px) align:center clip {
                                text font-color:ink.muted font-size:{px(11.0)}px text-wrap:none
                                    "No workers yet"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn WorkerTree(model: Model, session_id: String, focus: TreeFocus) -> Element {
    let id = State::new(session_id);
    let session = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .into_iter()
            .find(|s| s.id == id.get())
    });
    let title = Derived::new(move || session.get().map(|s| s.title).unwrap_or_default());
    let detail = Derived::new(move || {
        let snapshot = model.snapshot.get();
        session
            .get()
            .map(|s| {
                let issue = snapshot
                    .issues
                    .iter()
                    .find(|i| Some(&i.id) == s.issue_id.as_ref())
                    .map(|i| format!("{} · ", i.label()))
                    .unwrap_or_default();
                format!(
                    "{issue}{}",
                    crate::labels::session_state(&snapshot, &s).label()
                )
            })
            .unwrap_or_default()
    });
    let selected =
        Derived::new(move || model.page.get() == Page::Sessions && model.session.get() == id.get());
    let state = Derived::new(move || {
        session
            .get()
            .map(|s| crate::labels::session_state(&model.snapshot.get(), &s))
            .unwrap_or(RunState::Unavailable)
    });
    let navigation = view! {
        button #relay.tree-control #relay.tree-leaf @click:{model.open_session(id.get_untracked());} width:fill shrink:1
            pad:(horizontal:{px(6.0)}px vertical:0px) gap:{px(6.0)}px
            label:{format!("Open worker {}", title.get())} description:{detail.get()}
            fill:{if selected.get() {color(surface.selected)} else {Color::TRANSPARENT}}
            stroke:(width:{px(if selected.get() {2.0} else {0.0})} color:ink.fg edges:left)
            font-color:{color(if selected.get() {ink.fg} else {ink.muted})}
            hover { fill:surface.raised } {
            icon size:{px(14.0)}px shrink:0 worker-icon
            row #relay.tree-label {
                text width:max-content text-wrap:none font-size:{px(12.0)}px font-color:{color(if selected.get() {ink.fg} else {ink.muted})} {title.get()}
            }
            StatusGlyph state:(state)
            tooltip #relay.tooltip summary:"Worker" {
                col height:min-content gap:{px(4.0)}px {
                    text {title.get()}
                    text font-size:{px(11.0)}px font-color:{color(ink.muted)} {detail.get()}
                }
            }
        }
    };
    bind_tree_key(
        model,
        &navigation,
        TreeItem::Worker(id.get_untracked()),
        focus,
    );
    navigation
}

fn toggle(state: State<std::collections::BTreeSet<String>>, id: String) {
    state.update(|ids| {
        if !ids.remove(&id) {
            ids.insert(id);
        }
    });
}

fn visible_items(model: Model) -> Vec<TreeItem> {
    let snapshot = model.snapshot.get_untracked();
    let projects = model.expanded_projects.get_untracked();
    let directors = model.expanded_directors.get_untracked();
    let mut items = Vec::new();
    for project in &snapshot.projects {
        items.push(TreeItem::Project(project.id.clone()));
        if !projects.contains(&project.id) {
            continue;
        }
        for director in snapshot
            .directors
            .iter()
            .filter(|d| d.project_id == project.id)
        {
            items.push(TreeItem::Director(director.id.clone()));
            if !directors.contains(&director.id) {
                continue;
            }
            items.extend(
                snapshot
                    .sessions
                    .iter()
                    .filter(|s| {
                        s.project_id == project.id
                            && s.director_id == director.id
                            && s.role == SessionRole::Worker
                    })
                    .map(|s| TreeItem::Worker(s.id.clone())),
            );
        }
    }
    items
}

fn focus_item(focus: &TreeFocus, item: &TreeItem) {
    let element = focus.borrow().get(item).cloned();
    if let Some(element) = element {
        element.focus();
        element.reveal();
    }
}

fn bind_tree_key(model: Model, element: &Element, item: TreeItem, focus: TreeFocus) {
    focus.borrow_mut().insert(item.clone(), element.clone());
    let remove = focus.clone();
    let removed_item = item.clone();
    mosaic::core::reactive::on_cleanup(move || {
        remove.borrow_mut().remove(&removed_item);
    });
    element.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. })
            || event.modifiers.ctrl
            || event.modifiers.meta
            || event.modifiers.alt
        {
            return;
        }
        let items = visible_items(model);
        let Some(index) = items.iter().position(|i| i == &item) else {
            return;
        };
        match &event.key {
            Key::ArrowDown => {
                if let Some(next) = items.get(index + 1) {
                    focus_item(&focus, next);
                }
            }
            Key::ArrowUp => {
                if let Some(previous) = index.checked_sub(1).and_then(|i| items.get(i)) {
                    focus_item(&focus, previous);
                }
            }
            Key::Home => {
                if let Some(first) = items.first() {
                    focus_item(&focus, first);
                }
            }
            Key::End => {
                if let Some(last) = items.last() {
                    focus_item(&focus, last);
                }
            }
            Key::ArrowRight => {
                let open = match &item {
                    TreeItem::Project(id) => model.expanded_projects.get_untracked().contains(id),
                    TreeItem::Director(id) => model.expanded_directors.get_untracked().contains(id),
                    TreeItem::Worker(_) => return,
                };
                if !open {
                    match &item {
                        TreeItem::Project(id) => {
                            model.expanded_projects.update(|ids| {
                                ids.insert(id.clone());
                            });
                        }
                        TreeItem::Director(id) => {
                            model.expanded_directors.update(|ids| {
                                ids.insert(id.clone());
                            });
                        }
                        _ => {}
                    }
                } else if let Some(child) = items.get(index + 1) {
                    let is_child = matches!(
                        (&item, child),
                        (TreeItem::Project(_), TreeItem::Director(_))
                            | (TreeItem::Director(_), TreeItem::Worker(_))
                    );
                    if is_child {
                        focus_item(&focus, child);
                    }
                }
            }
            Key::ArrowLeft => {
                let snapshot = model.snapshot.get_untracked();
                match &item {
                    TreeItem::Project(id) => {
                        model.expanded_projects.update(|ids| {
                            ids.remove(id);
                        });
                    }
                    TreeItem::Director(id)
                        if model.expanded_directors.get_untracked().contains(id) =>
                    {
                        model.expanded_directors.update(|ids| {
                            ids.remove(id);
                        });
                    }
                    TreeItem::Director(id) => {
                        if let Some(director) = snapshot.directors.iter().find(|d| &d.id == id) {
                            focus_item(&focus, &TreeItem::Project(director.project_id.clone()));
                        }
                    }
                    TreeItem::Worker(id) => {
                        if let Some(session) = snapshot.sessions.iter().find(|s| &s.id == id) {
                            focus_item(&focus, &TreeItem::Director(session.director_id.clone()));
                        }
                    }
                }
            }
            _ => return,
        }
        ctx.stop_propagation();
    });
}

fn capacity_label((running, active, limit): (usize, usize, usize)) -> String {
    if limit == 0 {
        return "Delegation paused".into();
    }
    format!("{active} of {limit} workers active, {running} running")
}
