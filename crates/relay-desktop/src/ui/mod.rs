//! Relay views. The shell routes pages; boards, profiles and preferences live
//! in focused submodules.

mod board;
mod chrome;
pub(crate) mod navigation;
mod preferences;
mod profile;
mod status;

use crate::window_chrome::{WindowControls, WindowControlsProps};
use crate::{
    controls::button,
    conversation::{Conversation, ConversationProps},
    icons::*,
    model::{EditTarget, Model, Page},
    projects::*,
    sidebar::{Sidebar, SidebarProps},
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
use relay_ui::prelude::{AgentStatus, Chip, ChipProps, Segments, SegmentsProps};
use relay_ui::surfaces::{CommandPalette, CommandPaletteProps};
use relay_ui::tokens::{ground, px};
pub(crate) use status::*;

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
            fill:ground.page safe-area:all pad:(top:{model.browser_top_clearance.get()}px) clip font-family:monospace font-color:ground.ink font-size:{px(14.0)}px {
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
                                    text font-color:{color(ground.muted)} { model.status.get() }
                                    text font-color:ground.muted
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
            if crate::window_chrome::DRAWS_WINDOW_BUTTONS {
                row width:max-content height:min-content
                    translate:(x:{width.get() - crate::window_chrome::window_controls_width()}px y:0px) {
                    WindowControls model:(model)
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
    Effect::new(move || crate::browser::surface_color(color(ground.page)));
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
            // P pauses or resumes the open session's queue, as its header
            // cell shows, unless a text field takes the key.
            Key::Character(key)
                if !command
                    && !event.modifiers.alt
                    && key.eq_ignore_ascii_case("p")
                    && model.page.get_untracked() == Page::Sessions
                    && !typing(model) =>
            {
                model.toggle_queue();
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

const PALETTE_ACTIONS: [(&str, &str); 11] = [
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
];

/// The icon of a palette category.
fn category_icon(category: &str) -> SvgToken {
    use relay_ui::icons::command;
    match category {
        "Navigate" => command.navigate,
        "Profile" => command.profile,
        "Create" => command.create,
        "Search" => command.search,
        "Sync" => command.sync,
        "Worker" => command.worker,
        "Settings" => command.settings,
        "Project" => command.project,
        _ => command.board,
    }
}

/// The command palette: relay-ui's palette over a scrim that dismisses it.
/// Closing returns focus to where it was.
#[component]
fn Palette(model: Model) -> Element {
    let previous_focus = model.ui.get_untracked().focused();
    let ui = model.ui.get_untracked();
    let available_height = State::new(900.0f32);
    let commands: Vec<relay_ui::surfaces::Command> = PALETTE_ACTIONS
        .iter()
        .enumerate()
        .map(|(index, (label, category))| {
            relay_ui::surfaces::Command::new(index.to_string(), *label)
                .detail(*category)
                .icon(category_icon(category))
        })
        .collect();
    let run: relay_ui::surfaces::OnCommand = std::rc::Rc::new(move |id: String| {
        if let Ok(index) = id.parse() {
            palette_action(model, index);
        }
    });
    // The list keeps the panel inside the window: head, field and foot take
    // 114px, the scrim's padding 80px.
    let list_height =
        Derived::new(move || ((available_height.get() / px(1.0)) - 194.0).clamp(80.0, 420.0));
    let panel = view! {
        CommandPalette commands:(commands) query:(model.palette_query) on-run:(run)
            title:(Some("Command palette".to_string()))
            on-close:(Some(press(move || model.palette.set(false))))
            close-label:"Close command palette" field-label:"Command search"
            placeholder:"Find an action…" footer:true autofocus:true
            max-height:(relay_ui::Live::from(list_height))
            label:(Some("Command palette panel".to_string()))
    };
    panel.on_pointer(|event, ctx| {
        if matches!(
            event.kind,
            PointerEventKind::Down(_) | PointerEventKind::Click(_)
        ) {
            ctx.stop_propagation();
        }
    });
    let view = view! {
        col fill:{mosaic::core::theme::color(ground.page).alpha(0.72)} align:center
            pad:(horizontal:{px(16.0)}px vertical:{px(40.0)}px)
            @layout:{move |rect:Rect| available_height.set(rect.size.height)} {
            { parent.adopt(&panel); }
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

/// Whether keyboard focus is in a text field.
fn typing(model: Model) -> bool {
    let ui = model.ui.get_untracked();
    ui.focused().is_some_and(|focused| {
        ui.inspection_snapshot()
            .node(focused.id())
            .is_some_and(|node| node.role == mosaic::widgets::Role::TextInput)
    })
}

/// An activation handler for relay-ui controls.
pub(crate) fn press(handler: impl Fn() + 'static) -> relay_ui::prelude::OnPress {
    std::rc::Rc::new(handler)
}

/// A relay-ui cell action for ruled grids and strips: the control fills a
/// 36px cell closed by hairlines on its right and bottom, so neighbours
/// share one rule. `disabled` is read reactively.
pub(crate) fn action_cell(
    label: impl Into<String>,
    text: impl Into<String>,
    disabled: impl Fn() -> bool + 'static,
    handler: impl Fn() + 'static,
) -> Element {
    use relay_ui::prelude::{CellButton, CellButtonProps, Label, LabelProps, TextRole};
    let label = label.into();
    let text = text.into();
    let blocked = Derived::new(disabled);
    view! {
        row height:{px(36.0)}px min-width:0px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:(right bottom)) {
            CellButton label:(label) on-press:(press(handler)) disabled:(blocked) {
                Label text:(text) role:(TextRole::Label) weight:500
            }
        }
    }
}
