//! Relay views. The shell routes pages; boards, profiles and preferences live
//! in focused submodules.

mod board;
mod chrome;
pub(crate) mod navigation;
mod preferences;
mod profile;

use crate::styles::*;
use crate::{
    controls::{
        AppearanceSegments, AppearanceSegmentsProps, ButtonStyle, button, stepper_with_options,
    },
    conversation::{Conversation, ConversationProps},
    labels::{
        DirectorMark, DirectorMarkProps, LabelStrip, LabelStripProps, Readout, ReadoutProps,
        RunState, SlidingSegments, SlidingSegmentsProps, SlotMeter, SlotMeterProps, StatusGlyph,
        StatusGlyphProps, Tag, TagProps,
    },
    model::{EditTarget, Model, Page},
    projects::*,
    sidebar::{Sidebar, SidebarProps},
    theme::*,
};
pub(crate) use board::*;
pub(crate) use chrome::*;
use mosaic::core::theme::color;
use mosaic::prelude::*;
use navigation::{Drawer, DrawerProps};
use preferences::*;
#[cfg(test)]
pub(crate) use profile::overridden_fields;
use profile::*;
use relay_core::*;

pub fn shell(model: Model) -> Element {
    let width = State::new(1280.0f32);
    let drawer_drag = State::new(None::<f32>);
    let mobile = Derived::new(move || crate::platform::compact_navigation(width.get()));
    Effect::new(move || {
        model.mobile_navigation.set(mobile.get());
        model.sidebar_open.set(false);
        drawer_drag.set(None);
    });
    let root = view! {
        stack width:{model.browser_viewport.get().map_or(Dimension::Fill, |size| Dimension::Px(size.width))}
            height:{model.browser_viewport.get().map_or(Dimension::Fill, |size| Dimension::Px(size.height))}
            fill:surface.base safe-area:all pad:(top:{model.browser_top_clearance.get()}px) clip font-family:monospace font-color:ink.fg font-size:{px(14.0)}px {
            row @layout:{ move |rect: Rect| width.set(rect.size.width) } {
                col width:{if mobile.get() {Dimension::Px(0.0)} else {Dimension::MaxContent}} shrink:0 {
                    if !mobile.get() {
                        Sidebar model:(model) viewport:(width)
                    }
                }
                col width:1fr min-width:0px label:"Main content" {
                    if !matches!(model.page.get(), Page::Sessions | Page::Board | Page::Directors | Page::DirectorStart) || model.snapshot.get().projects.is_empty() {
                        PageHeader model:(model)
                            // Settings and New Project are not about the open project, so
                            // they title themselves.
                            eyebrow:(Derived::new(move || if matches!(model.page.get(), Page::Settings | Page::NewProject) { "Relay".to_string() } else { page_label(model.page.get()).to_string() }))
                            title:(Derived::new(move || if matches!(model.page.get(), Page::Settings | Page::NewProject) { page_label(model.page.get()).to_string() } else { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| p.name.clone()).unwrap_or_default() }))
                            compact:(Derived::new(move || width.get() < px(900.0)))
                    }
                    if !matches!(model.page.get(), Page::Sessions | Page::Board | Page::Directors | Page::DirectorStart) || model.snapshot.get().projects.is_empty() {
                        Notice model:(model)
                    }
                    if model.page.get() == Page::Settings {
                        col {
                            Settings model:(model)
                        }
                    } else if matches!(model.page.get(), Page::NewProject | Page::Connections | Page::Publish) {
                        ProjectPage model:(model)
                    } else if model.page.get() == Page::DirectorStart {
                        Conversation model:(model)
                    } else {
                        col {
                            if model.snapshot.get().projects.is_empty() {
                                col height:min-content pad:{px(32.0)}px gap:{px(12.0)}px {
                                    text font-family:sans-serif font-size:{px(18.0)}px
                                        "Waiting for your workspace"
                                    text font-color:{color(ink.muted)} { model.status.get() }
                                    text font-color:ink.muted
                                        "Start relay-server and connect with its workspace token."
                                }
                            } else {
                                // One independent branch per page, each depending on the page
                                // alone: width and scale changes must not rebuild a page and
                                // its editing surfaces.
                                col {
                                    if model.page.get() == Page::Board {
                                        BoardArea model:(model) width:(width)
                                    }
                                    if model.page.get() == Page::Sessions {
                                        Sessions model:(model)
                                    }
                                    if !matches!(model.page.get(), Page::Board | Page::Sessions) {
                                        Profiles model:(model)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if mobile.get() && (model.sidebar_open.get() || drawer_drag.get().is_some()) {
                Drawer model:(model) width:(width) drag:(drawer_drag)
            }
            if model.palette.get() {
                Palette model:(model)
            }
        }
    };
    navigation::install(&root, model, width, drawer_drag);
    #[cfg(target_arch = "wasm32")]
    root.on_pointer(move |event, _| {
        if matches!(event.kind, PointerEventKind::Up(PointerButton::Primary)) {
            // Run within the original browser gesture, before WebKit's user
            // activation ends. The field has already placed its caret.
            crate::browser_text::focus(event.position.x, event.position.y);
        }
    });
    #[cfg(target_arch = "wasm32")]
    Effect::new(move || crate::browser::surface_color(color(surface.base)));
    crate::window_chrome::install(&root, model);
    root.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let command = if cfg!(target_os = "macos") {
            event.modifiers.meta
        } else {
            event.modifiers.ctrl
        };
        match &event.key {
            Key::Tab if !command && !event.modifiers.alt => {
                let ui = model.ui.get_untracked();
                if event.modifiers.shift {
                    ui.focus_prev();
                } else {
                    ui.focus_next();
                }
                if let Some(focused) = ui.focused() {
                    focused.reveal();
                }
                ctx.stop_propagation();
            }
            Key::Character(key) if command && key.eq_ignore_ascii_case("k") => {
                model.palette.set(!model.palette.get_untracked());
                ctx.stop_propagation();
            }
            Key::Character(key)
                if command
                    && key.eq_ignore_ascii_case("f")
                    && model.page.get_untracked() == Page::Sessions =>
            {
                model.searching.set(true);
                ctx.stop_propagation();
            }
            Key::Character(key) if command && key == "," => {
                model.page.set(Page::Settings);
                ctx.stop_propagation();
            }
            Key::Escape => {
                model.sidebar_open.set(false);
                model.palette.set(false);
                model.searching.set(false);
                ctx.stop_propagation();
            }
            _ => {}
        }
    });
    root
}

/// The board with its inspector; narrow windows show a selected issue full
/// width instead.
#[component]
fn BoardArea(model: Model, width: State<f32>) -> Element {
    view! {
        col {
            if width.get() < px(1050.0) && model.issue.get().is_some() {
                IssueDetail model:(model) full:true
            } else {
                row height:1fr {
                    Board model:(model) narrow:{ width.get() < px(850.0) }
                    if model.issue.get().is_some() {
                        IssueDetail model:(model)
                    }
                }
            }
        }
    }
}

#[component]
fn Sessions(model: Model) -> Element {
    view! {
        col {
            Conversation model:(model)
        }
    }
}

const PALETTE_ACTIONS: [(&str, &str); 12] = [
    ("Open board", "Navigate"),
    ("Open sessions", "Navigate"),
    ("Edit project defaults", "Profile"),
    ("Create director", "Create"),
    ("Search transcript", "Search"),
    ("Sync project", "Sync"),
    ("Stop worker", "Worker"),
    ("Open settings", "Settings"),
    ("Project Connections", "Project"),
    ("New Project", "Create"),
    ("Publish board", "Board"),
    ("Reset sidebar width", "Layout"),
];

#[component]
fn Palette(model: Model) -> Element {
    let query = model.palette_query;
    let previous_focus = model.ui.get_untracked().focused();
    let ui = model.ui.get_untracked();
    let available_height = State::new(900.0f32);
    let selected = State::new(0usize);
    let matches = Derived::new(move || {
        PALETTE_ACTIONS
            .iter()
            .enumerate()
            .filter(|(_, (label, _))| label.to_lowercase().contains(&query.get().to_lowercase()))
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    });
    let current = Derived::new(move || {
        let matches = matches.get();
        matches
            .iter()
            .copied()
            .find(|index| *index == selected.get())
            .or_else(|| matches.first().copied())
    });
    let view = view! {
        col fill:scrim align:center pad:(horizontal:{px(16.0)}px vertical:{px(40.0)}px)
            @layout:{move |rect:Rect| available_height.set(rect.size.height)} {
            col width:{px(560.0)} max-width:100% height:1fr
                max-height:{px(120.0 + if matches.get().is_empty() {60.0} else {40.0 * matches.get().len() as f32}).min(px(640.0)).min((available_height.get()-px(80.0)).max(px(160.0)))}
                fill:surface.panel stroke:(width:{px(1.0)} color:rule.line offset:{px(0.5)}) pad:0px
                gap:0px label:"Command palette panel" {
                row height:{px(44.0)}px shrink:0 gap:0px align:center
                    stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                    row width:1fr min-width:0px align:center gap:{px(9.0)}px
                        pad:(horizontal:{px(14.0)}px vertical:0px) {
                        icon size:{px(16.0)}px command-icon
                        text font-size:{px(15.0)}px font-weight:650 font-family:sans-serif
                            "Command palette"
                    }
                    button #relay.header-action @click:{model.palette.set(false);}
                        stroke:(width:{px(1.0)} color:rule.hair edges:left)
                        label:"Close command palette" "Esc"
                }
                input #relay.field placeholder:"Find an action…" label:"Command search"
                    height:{px(46.0)}px shrink:0 pad:(horizontal:{px(14.0)}px vertical:{px(10.0)}px)
                    stroke:(width:{px(1.0)} color:rule.hair edges:bottom)
                    focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(-2.0)}) } query
                    as command_search
                {command_search.focus();}
                scroll height:1fr min-height:0px label:"Command results" {
                    col height:min-content gap:0px {
                        for (_, index) in {matches.get().into_iter().map(|index| (index, index))} {
                            let index = *index;
                            let (label, category) = PALETTE_ACTIONS[index];
                            button #relay.tree-control @click:{palette_action(model, index);}
                                width:fill height:{px(40.0)}px justify:start gap:{px(12.0)}px
                                pad:(horizontal:{px(14.0)}px vertical:0px) label:(label)
                                font-color:{color(if current.get() == Some(index) {ink.on_inverse} else {ink.fg})}
                                font-size:{px(13.0)}px
                                fill:{if current.get() == Some(index) {color(ink.inverse)} else {Color::TRANSPARENT}}
                                stroke:(width:{px(1.0)} color:rule.hair edges:bottom)
                                hover {
                                    fill:{color(if current.get() == Some(index) {ink.inverse_hover} else {surface.raised})}
                                }
                                focused { stroke:(width:{px(3.0)} color:accent.focus edges:left) } {
                                icon size:{px(16.0)}px
                                    {match index {0|10 => board_icon, 1|6 => worker_icon, 2 => sliders_icon, 3 => director_icon, 5|11 => reset_icon, 7 => gear_icon, 8 => connections_icon, 9 => plus_icon, _ => command_icon}}
                                row width:1fr min-width:0px height:min-content {
                                    text text-wrap:none (label)
                                }
                                row width:max-content height:min-content opacity:0.65
                                    font-size:{px(10.0)}px {
                                    text text-transform:uppercase letter-spacing:{px(0.5)}px
                                        (category)
                                }
                                row width:{px(16.0)}px height:min-content
                                    opacity:{if current.get() == Some(index) {1.0} else {0.0}}
                                    font-size:{px(13.0)}px {
                                    text "↵"
                                }
                            }
                        }
                        if matches.get().is_empty() {
                            row height:{px(60.0)}px pad:{px(14.0)}px align:center
                                label:"No matching commands" font-color:ink.muted {
                                text "No matching commands"
                            }
                        }
                    }
                }
                row height:{px(30.0)}px shrink:0 align:center justify:between
                    pad:(horizontal:{px(14.0)}px vertical:0px)
                    stroke:(width:{px(1.0)} color:rule.hair edges:top) font-color:ink.muted
                    font-size:{px(10.0)}px {
                    text {format!("{} actions", matches.get().len())}
                    text "↑ ↓ Navigate · Enter Run · Esc Close"
                }
            } as panel
            {panel.on_pointer(|event, ctx| {if matches!(event.kind, PointerEventKind::Down(_) | PointerEventKind::Click(_)) {ctx.stop_propagation();}});}
        }
    };
    view.on_pointer(move |event, ctx| {
        if matches!(event.kind, PointerEventKind::Down(PointerButton::Primary)) {
            model.palette.set(false);
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
    view.label("Command palette");
    let root_id = view.id();
    view.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let ui = model.ui.get_untracked();
        match event.key {
            Key::Enter => {
                let focused_label = ui.focused().and_then(|focused| {
                    ui.inspection_snapshot()
                        .node(focused.id())
                        .and_then(|node| node.label.clone())
                });
                let index = focused_label
                    .and_then(|label| {
                        PALETTE_ACTIONS
                            .iter()
                            .position(|(action, _)| *action == label)
                    })
                    .or_else(|| current.get_untracked());
                if let Some(index) = index {
                    palette_action(model, index);
                }
                ctx.stop_propagation();
            }
            Key::Tab | Key::ArrowDown | Key::ArrowUp => {
                let snapshot = ui.inspection_snapshot();
                for _ in 0..snapshot.nodes.len() {
                    if event.key == Key::ArrowUp || (event.key == Key::Tab && event.modifiers.shift)
                    {
                        ui.focus_prev();
                    } else {
                        ui.focus_next();
                    }
                    let mut ancestor = ui.focused().map(|element| element.id());
                    while let Some(id) = ancestor {
                        if id == root_id {
                            if let Some(focused) = ui.focused() {
                                if let Some(node) = snapshot.node(focused.id())
                                    && let Some(index) =
                                        PALETTE_ACTIONS.iter().position(|(label, _)| {
                                            node.label.as_deref() == Some(*label)
                                        })
                                {
                                    selected.set(index);
                                }
                                focused.reveal();
                            }
                            ctx.stop_propagation();
                            return;
                        }
                        ancestor = snapshot.node(id).and_then(|node| node.parent);
                    }
                }
                ctx.stop_propagation();
            }
            _ => {}
        }
    });
    view
}

fn palette_action(model: Model, index: usize) {
    match index {
        0 => model.page.set(Page::Board),
        1 => model.page.set(Page::Sessions),
        2 => model.open_profile(EditTarget::Defaults),
        3 => model.open_profile(EditTarget::New),
        5 => model.sync_project(),
        6 => model.stop_worker(),
        7 => model.page.set(Page::Settings),
        8 => model.page.set(Page::Connections),
        9 => model.page.set(Page::NewProject),
        10 => model.page.set(Page::Publish),
        11 => crate::settings::reset_sidebar(model),
        _ => {
            model.page.set(Page::Sessions);
            model.searching.set(true);
        }
    }
    model.palette.set(false);
    model.palette_query.set(String::new());
}

fn page_label(page: Page) -> &'static str {
    match page {
        Page::Board => "Project board",
        Page::Sessions => "Sessions",
        Page::Directors => "Directors",
        Page::Settings => "Settings",
        Page::NewProject => "New Project",
        Page::Connections => "Project Connections",
        Page::Publish => "Publish board",
        Page::DirectorStart => "Director conversation",
    }
}
