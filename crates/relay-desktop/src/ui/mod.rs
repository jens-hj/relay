//! Relay views. The shell routes pages; boards, profiles and preferences live
//! in focused submodules.

mod board;
mod chrome;
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
use preferences::*;
#[cfg(test)]
pub(crate) use profile::overridden_fields;
use profile::*;
use relay_core::*;

pub fn shell(model: Model) -> Element {
    let width = State::new(1280.0f32);
    let root = view! {
        stack fill:surface.base font-family:monospace font-color:ink.fg font-size:{px(14.0)}px {
            row @layout:{ move |rect: Rect| width.set(rect.size.width) } {
                Sidebar model:(model) viewport:(width)
                col width:1fr {
                    if !matches!(model.page.get(), Page::Sessions | Page::Board | Page::Directors) {
                        PageHeader model:(model)
                            // Settings and New Project are not about the open project, so
                            // they title themselves.
                            eyebrow:(Derived::new(move || if matches!(model.page.get(), Page::Settings | Page::NewProject) { "Relay".to_string() } else { page_label(model.page.get()).to_string() }))
                            title:(Derived::new(move || if matches!(model.page.get(), Page::Settings | Page::NewProject) { page_label(model.page.get()).to_string() } else { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| p.name.clone()).unwrap_or_default() }))
                            compact:(Derived::new(move || width.get() < px(900.0)))
                    }
                    if !model.notice.get().is_empty() {
                        row height:min-content fill:attention.fill pad:{px(12.0)}px gap:{px(12.0)}px
                            align:center shrink:0
                            stroke:(width:{px(4.0)} color:attention.text edges:left) {
                            col width:1fr height:min-content gap:{px(4.0)}px {
                                text font-size:{px(12.0)}px font-color:{color(attention.on)}
                                    { model.notice.get() }
                                if model.can_retry() {
                                    text font-size:{px(11.0)}px font-color:{color(attention.on)}
                                        { model.retry_summary() }
                                }
                            }
                            if model.can_rebase() {
                                button #relay.action @click:{ model.review_latest(); }
                                    disabled:{ model.busy.get() } "Review latest state"
                            }
                            if model.can_rebase() {
                                button #relay.action @click:{ model.rebase_conflict(); }
                                    label:"Review conflict for new request"
                                    "Review conflict / new request"
                            }
                            if model.can_retry() {
                                button #relay.action @click:{ model.retry_pending(); }
                                    label:"Retry original request" "Retry original request"
                            }
                            button #relay.action @click:{ model.notice.set(String::new()); }
                                "Dismiss"
                        }
                    }
                    if model.page.get() == Page::Settings {
                        col {
                            Settings model:(model)
                        }
                    } else if matches!(model.page.get(), Page::NewProject | Page::Connections | Page::Publish | Page::DirectorStart) {
                        ProjectPage model:(model)
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
            if model.palette.get() {
                Palette model:(model)
            }
        }
    };
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

#[component]
fn Palette(model: Model) -> Element {
    let query = model.palette_query;
    let previous_focus = model.ui.get_untracked().focused();
    let ui = model.ui.get_untracked();
    on_cleanup(move || {
        if let Some(previous) = previous_focus
            && ui.inspection_snapshot().contains(previous.id())
        {
            previous.focus();
        }
    });
    let view = view! {
        col fill:scrim align:center pad:(horizontal:{px(30.0)}px vertical:{px(90.0)}px) {
            col height:min-content width:{px(560.0)} max-width:100% fill:surface.panel
                stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)}) pad:{px(18.0)}px
                gap:{px(12.0)}px {
                row height:min-content justify:between align:center {
                    text font-size:{px(15.0)}px font-weight:650 font-family:sans-serif
                        "Command palette"
                    button #relay.action @click:{ model.palette.set(false); } "Esc"
                }
                input #relay.field placeholder:"Find an action…" label:"Command search" query
                    as command_search
                { command_search.focus(); }
                for (label, index) in [("Open board", 0), ("Open sessions", 1), ("Edit project defaults", 2), ("Create director", 3), ("Search transcript", 4), ("Sync project", 5), ("Stop worker", 6), ("Open settings", 7), ("Project Connections", 8), ("New Project", 9), ("Publish board", 10), ("Reset sidebar width", 11)] {
                    if label.to_lowercase().contains(&query.get().to_lowercase()) {
                        button #relay.action
                            @click:{
                            palette_action(model, index);
                        }
                            (label)
                    }
                }
            }
        }
    };
    view.label("Command palette");
    let root_id = view.id();
    view.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let ui = model.ui.get_untracked();
        match event.key {
            Key::Enter => {
                if let Some(index) = [
                    "Open board",
                    "Open sessions",
                    "Edit project defaults",
                    "Create director",
                    "Search transcript",
                    "Sync project",
                    "Stop worker",
                    "Open settings",
                    "Project Connections",
                    "New Project",
                    "Publish board",
                    "Reset sidebar width",
                ]
                .iter()
                .position(|label| {
                    label
                        .to_lowercase()
                        .contains(&query.get_untracked().to_lowercase())
                }) {
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
