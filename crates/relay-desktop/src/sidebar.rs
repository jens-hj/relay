//! The sidebar: project navigation as ruled header cells above the agent
//! tree of the selected project, with usage and the server connection at
//! its foot.

use crate::icons::*;
use crate::{
    adapt,
    model::{EditTarget, Model, Page},
};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use relay_core::*;
use relay_ui::prelude::{
    AgentNode, AgentSidebar, AgentSidebarProps, ConnectionStatus, ConnectionStatusProps, OnPress,
    UsageFoot, UsageFootProps,
};
use relay_ui::style::*;
use relay_ui::surfaces::{Connection, ConnectionInfo};
use relay_ui::tokens::{ground, px};
use std::rc::Rc;

/// Logical sidebar width before UI scale.
pub const SIDEBAR_WIDTH: f32 = 380.0;

fn server_identity(endpoint: &str) -> String {
    let Ok(url) = reqwest::Url::parse(endpoint) else {
        return endpoint.to_owned();
    };
    let Some(host) = url.host_str() else {
        return endpoint.to_owned();
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

fn unix_now() -> u64 {
    crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Answers a session's first pending permission request. Every view of the
/// request reads the same snapshot entry, so it resolves everywhere.
pub fn respond_first(model: Model, session_id: &str, allow: bool) {
    let snapshot = model.snapshot.get_untracked();
    if let Some(pending) = adapt::needs_you(&snapshot, &model.project.get_untracked())
        .into_iter()
        .find(|p| p.session_id == session_id)
    {
        respond(model, &pending.permission, allow);
    }
}

/// Answers one permission request.
pub fn respond(model: Model, permission: &ToolPermission, allow: bool) {
    model.submit(
        Command::RespondPermission {
            permission_id: permission.id.clone(),
            run_id: permission.run_id.clone(),
            allow,
        },
        model.snapshot.get_untracked().revision,
        crate::model::Saved::Action,
    );
}

#[component]
pub fn Sidebar(model: Model, viewport: State<f32>) -> Element {
    // The width is fixed by the design; phones use the drawer's width.
    let width = if model.mobile_navigation.get_untracked() {
        crate::ui::navigation::drawer_width(viewport.get_untracked())
            / mosaic::core::theme::scalar(relay_ui::tokens::ui_scale)
    } else {
        SIDEBAR_WIDTH
    };
    let menu = State::new(false);
    let roots = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let selected = if matches!(model.page.get(), Page::Sessions) {
            model.session.get()
        } else {
            String::new()
        };
        adapt::agent_forest(
            &snapshot,
            &model.project.get(),
            &selected,
            &model.collapsed_agents.get(),
            &model.catalogs.get(),
        )
    });
    let usage = Derived::new(move || adapt::harness_usage(&model.snapshot.get(), unix_now()));
    let open: Rc<dyn Fn(String)> = Rc::new(move |id| model.open_session(id));
    let toggle: Rc<dyn Fn(String, bool)> = Rc::new(move |id, open| {
        model.collapsed_agents.update(|ids| {
            if open {
                ids.remove(&id);
            } else {
                ids.insert(id);
            }
        });
    });
    let focus: Rc<dyn Fn(Option<String>)> = Rc::new(move |id| model.agent_focus.set(id));
    let approve: Rc<dyn Fn(String)> = Rc::new(move |id| respond_first(model, &id, true));
    let review: Rc<dyn Fn(String)> = Rc::new(move |id| model.open_session(id));
    let new_agent: OnPress = Rc::new(move || model.open_profile(EditTarget::New));
    let roots: relay_ui::Live<Vec<AgentNode>> = roots.into();
    let focused: relay_ui::Live<Option<String>> =
        Derived::new(move || model.agent_focus.get()).into();
    let agents = view! {
        AgentSidebar roots:(roots) width:(width) focus:(focused) on-new:(Some(new_agent))
            on-open:(Some(open)) on-toggle:(Some(toggle)) on-focus:(Some(focus))
            on-approve:(Some(approve)) on-review:(Some(review)) {
            if !usage.get().is_empty() {
                UsageFoot harnesses:(relay_ui::Live::from(usage))
            }
            ServerFoot model:(model)
        }
    };
    view! {
        col width:{px(width)}px shrink:0 fill:ground.panel clip label:"Sidebar" {
            NavigationHead model:(model) menu:(menu)
            if menu.get() {
                ProjectMenu model:(model) menu:(menu)
            }
            col height:1fr min-height:0px {
                { parent.adopt(&agents); }
            }
        }
    }
}

/// The 60px head: the selected project (opening the project menu), its
/// board and settings, as ruled cells that fill the head.
#[component]
fn NavigationHead(model: Model, menu: State<bool>) -> Element {
    let name = Derived::new(move || {
        model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == model.project.get())
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "No project".into())
    });
    view! {
        row height:{crate::window_chrome::sidebar_header_height()}px shrink:0 align:center
            label:"Sidebar header" stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
            @pointer:{move |event, ctx| crate::window_chrome::drag_header(model, event, ctx)} {
            button #relay.tree-control @click:{menu.set(!menu.get_untracked());} width:1fr
                height:fill justify:start gap:{px(10.0)}px
                pad:(left:{crate::window_chrome::sidebar_title_inset().max(px(18.0))}px right:{px(12.0)}px)
                label:{format!("Toggle project {}", name.get())}
                description:{if menu.get() {"Expanded"} else {"Collapsed"}} font-color:ground.ink {
                col width:1fr min-width:0px height:min-content gap:{px(3.0)}px {
                    row #relay.eyebrow height:min-content {
                        text "Project"
                    }
                    row #relay.title height:min-content clip font-size:{px(13.0)}px {
                        text text-wrap:none {name.get()}
                    }
                }
                icon size:{px(13.0)}px shrink:0
                    {if menu.get() {tree_chevron_down} else {tree_chevron_right}}
            }
            button #relay.tree-control
                @click:{let id = model.project.get_untracked(); model.select_project(id);}
                width:{crate::window_chrome::sidebar_header_height()}px height:fill
                label:{format!("Open board for {}", name.get())}
                stroke:(width:{px(1.0)} color:ground.hairline edges:left)
                fill:{if model.page.get() == Page::Board {color(ground.selected)} else {Color::TRANSPARENT}}
                font-color:{color(if model.page.get() == Page::Board {ground.ink} else {ground.muted})} {
                icon size:{px(16.0)}px board-icon
            }
            button #relay.tree-control
                @click:{model.sidebar_open.set(false); model.page.set(Page::Settings);}
                width:{crate::window_chrome::sidebar_header_height()}px height:fill label:"Settings"
                stroke:(width:{px(1.0)} color:ground.hairline edges:left)
                fill:{if model.page.get() == Page::Settings {color(ground.selected)} else {Color::TRANSPARENT}}
                font-color:{color(if model.page.get() == Page::Settings {ground.ink} else {ground.muted})} {
                icon size:{px(16.0)}px gear-icon
            }
            if model.mobile_navigation.get() {
                button #relay.tree-control @click:{model.sidebar_open.set(false);}
                    width:{crate::window_chrome::sidebar_header_height()}px height:fill
                    label:"Close navigation"
                    stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                    icon size:{px(16.0)}px window-close
                } as close_navigation
                {close_navigation.focus();}
            }
        }
    }
}

/// The project menu: every project, then the selected project's directors,
/// defaults, and a new project, one ruled row each.
#[component]
fn ProjectMenu(model: Model, menu: State<bool>) -> Element {
    view! {
        scroll max-height:{px(360.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            col height:min-content role:list label:"Projects" {
                for (_, project) in {model.snapshot.get().projects.into_iter().map(|p| (p.id.clone(), p))} {
                    let id = State::new(project.id.clone());
                    let name = State::new(project.name.clone());
                    row height:{px(34.0)}px role:list-item
                        label:{format!("Project row {}", name.get())}
                        fill:{if model.project.get() == id.get() {color(ground.selected)} else {Color::TRANSPARENT}}
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        button #relay.tree-control
                            @click:{menu.set(false); model.select_project(id.get_untracked());}
                            width:1fr height:fill justify:start
                            pad:(horizontal:{px(18.0)}px vertical:0px)
                            label:{format!("Select project {}", name.get())} font-color:ground.ink {
                            text text-wrap:none {name.get()}
                        }
                        button #relay.tree-control
                            @click:{menu.set(false); model.select_project(id.get_untracked()); model.page.set(Page::Connections);}
                            width:{px(34.0)}px height:fill
                            label:{format!("Project Connections for {}", name.get())}
                            stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                            icon size:{px(14.0)}px connections-icon
                        }
                    }
                }
                for (_, director) in {model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>()} {
                    let id = State::new(director.id.clone());
                    let name = State::new(director.name.clone());
                    row height:{px(34.0)}px role:list-item
                        label:{format!("Director row {}", name.get())}
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        button #relay.tree-control
                            @click:{menu.set(false); model.open_director(id.get_untracked());}
                            width:1fr height:fill justify:start gap:{px(8.0)}px
                            pad:(horizontal:{px(18.0)}px vertical:0px)
                            label:{format!("Open director {}", name.get())}
                            description:{capacity_label(director_capacity(&model.snapshot.get(), &id.get()))}
                            font-color:ground.ink {
                            icon size:{px(13.0)}px director-icon
                            text text-wrap:none {name.get()}
                        }
                        button #relay.tree-control
                            @click:{menu.set(false); model.open_director_profile(id.get_untracked());}
                            width:{px(34.0)}px height:fill
                            label:{format!("Profile for {}", name.get())}
                            stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                            icon size:{px(14.0)}px sliders-icon
                        }
                    }
                }
                row height:{px(34.0)}px
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    button #relay.tree-control
                        @click:{menu.set(false); model.open_profile(EditTarget::New);} width:1fr
                        height:fill justify:start gap:{px(8.0)}px
                        pad:(horizontal:{px(18.0)}px vertical:0px) label:"Create director" {
                        icon size:{px(13.0)}px plus-icon
                        text "New director"
                    }
                    button #relay.tree-control
                        @click:{menu.set(false); model.open_profile(EditTarget::Defaults);}
                        width:max-content height:fill gap:{px(8.0)}px
                        pad:(horizontal:{px(14.0)}px vertical:0px) label:"Project defaults"
                        stroke:(width:{px(1.0)} color:ground.hairline edges:left) {
                        icon size:{px(14.0)}px sliders-icon
                        text "Defaults"
                    }
                }
                row height:{px(34.0)}px {
                    button #relay.tree-control
                        @click:{menu.set(false); model.sidebar_open.set(false); model.page.set(Page::NewProject);}
                        width:1fr height:fill justify:start gap:{px(8.0)}px
                        pad:(horizontal:{px(18.0)}px vertical:0px) label:"New Project" {
                        icon size:{px(13.0)}px plus-icon
                        text "New project"
                    }
                }
            }
        }
    }
}

/// The server connection at the sidebar foot, with its endpoint,
/// transport, protocol and the frontend and server builds.
#[component]
fn ServerFoot(model: Model) -> Element {
    let state = Derived::new(move || {
        if model.connected.get() {
            Connection::Connected
        } else if model.status.get().starts_with("Connecting") {
            Connection::Connecting
        } else {
            Connection::Offline
        }
    });
    let latency = Derived::new(move || {
        model
            .round_trip_ms
            .get()
            .filter(|_| model.connected.get())
            .map(|ms| ms.min(u64::from(u32::MAX)) as u32)
    });
    let info = Derived::new(move || {
        let frontend = BuildInfo::current();
        ConnectionInfo {
            endpoint: model.server_endpoint.get(),
            transport: "HTTP + WebSocket".into(),
            protocol: if model.connected.get() {
                format!("v{}", model.snapshot.get().protocol_version)
            } else {
                String::new()
            },
            revision: format!(
                "Frontend {} · Server {}",
                frontend.short_revision(),
                model
                    .server_build
                    .get()
                    .map(|b| b.short_revision())
                    .unwrap_or_else(|| "unknown".into())
            ),
        }
    });
    let host = view! {
        col height:min-content shrink:0 label:"Server connection details"
            description:{format!("{} · {}", server_identity(&model.server_endpoint.get()), model.status.get())} {}
    };
    host.switch(
        move || info.get(),
        move |parent, info| {
            parent.adopt(&view! {
                ConnectionStatus state:(relay_ui::Live::from(state))
                    latency:(relay_ui::Live::from(latency)) info:(info.clone())
            });
        },
    );
    host
}

/// Running, occupied (running or queued) and allowed workers for a director.
pub fn director_capacity(snapshot: &Snapshot, director_id: &str) -> (usize, usize, usize) {
    let statuses: Vec<&WorkerStatus> = snapshot
        .sessions
        .iter()
        .filter(|s| s.director_id == director_id && s.role != SessionRole::Director)
        .filter_map(|s| s.worker.as_ref().map(|w| &w.status))
        .collect();
    let running = statuses
        .iter()
        .filter(|s| matches!(s, WorkerStatus::Running))
        .count();
    let active = statuses
        .iter()
        .filter(|s| matches!(s, WorkerStatus::Running | WorkerStatus::Queued))
        .count();
    let limit = snapshot
        .directors
        .iter()
        .find(|d| d.id == director_id)
        .and_then(|d| snapshot.effective_profile(d).ok())
        .map(|p| p.max_workers as usize)
        .unwrap_or(0);
    (running, active, limit)
}

fn capacity_label((running, active, limit): (usize, usize, usize)) -> String {
    if limit == 0 {
        return "Delegation paused".into();
    }
    format!("{active} of {limit} workers active, {running} running")
}
