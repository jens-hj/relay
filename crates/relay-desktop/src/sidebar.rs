//! The sidebar: project navigation as ruled header cells above the agent
//! tree of the selected project, with usage and the server connection at
//! its foot.

use crate::icons::*;
use crate::ui::press;
use crate::{
    adapt,
    model::{EditTarget, Model, Page},
};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use relay_core::*;
use relay_ui::prelude::{
    AgentNode, AgentSidebar, AgentSidebarProps, ConnectionStatus, ConnectionStatusProps, Elbows,
    ElbowsProps, OnPress, UsageFoot, UsageFootProps,
};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Ellipsis, EllipsisProps, Label, LabelProps, Rule, TextRole, Tone,
    UiText, Weight,
};
use relay_ui::surfaces::{Connection, ConnectionInfo, ConnectionTelemetry};
use relay_ui::tokens::{ground, px};
use relay_ui::tree::Guide;
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
        AgentSidebar roots:(roots) width:(width) focus:(focused) root-indent:true
            on-new:(Some(new_agent)) on-open:(Some(open)) on-toggle:(Some(toggle))
            on-focus:(Some(focus)) on-approve:(Some(approve)) on-review:(Some(review)) {
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
    let side = Derived::new(crate::window_chrome::sidebar_header_height);
    let close = view! {
        CellButton label:"Close navigation" rule:(Rule::Left) horizontal-inset:(0.0)
            on-press:(press(move || model.sidebar_open.set(false))) {
            icon size:{px(16.0)}px window-close
        }
    };
    let focus_close = close.clone();
    view! {
        row height:{crate::window_chrome::sidebar_header_height()}px shrink:0 align:center
            label:"Sidebar header" stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
            @pointer:{move |event, ctx| crate::window_chrome::drag_header(model, event, ctx)} {
            row width:1fr min-width:0px {
                CellButton label:(Derived::new(move || format!("Toggle project {}", name.get())))
                    description:(Some(UiText::from(Derived::new(move || if menu.get() {"Expanded".to_string()} else {"Collapsed".to_string()}))))
                    on-press:(press(move || menu.set(!menu.get_untracked()))) start:true
                    horizontal-inset:(0.0) {
                    row width:1fr min-width:0px align:center gap:{px(10.0)}px
                        pad:(left:{crate::window_chrome::sidebar_title_inset().max(px(18.0))}px right:{px(12.0)}px) {
                        col width:1fr min-width:0px height:min-content gap:{px(3.0)}px {
                            Label text:"Project" role:(TextRole::Caption) tone:(Tone::Muted)
                                weight:500
                            Ellipsis text:(name) role:(TextRole::Label) weight:(Weight::SEMIBOLD)
                        }
                        icon size:{px(13.0)}px shrink:0
                            {if menu.get() {tree_chevron_down} else {tree_chevron_right}}
                    }
                }
            }
            row width:{side.get()}px shrink:0 {
                CellButton label:(Derived::new(move || format!("Open board for {}", name.get())))
                    rule:(Rule::Left) horizontal-inset:(0.0)
                    selected:(Derived::new(move || model.page.get() == Page::Board))
                    on-press:(press(move || { let id = model.project.get_untracked(); model.select_project(id); })) {
                    icon size:{px(16.0)}px board-icon
                }
            }
            row width:{side.get()}px shrink:0 {
                CellButton label:"Settings" rule:(Rule::Left) horizontal-inset:(0.0)
                    selected:(Derived::new(move || model.page.get() == Page::Settings))
                    on-press:(press(move || { model.sidebar_open.set(false); model.page.set(Page::Settings); })) {
                    icon size:{px(16.0)}px gear-icon
                }
            }
            if model.mobile_navigation.get() {
                row width:{side.get()}px shrink:0 {
                    { parent.adopt(&close); focus_close.focus(); }
                }
            }
        }
    }
}

/// One full-height cell of a project menu row.
fn menu_cell(
    label: Derived<String>,
    press_handler: relay_ui::prelude::OnPress,
    main: bool,
    content: Element,
    description: Option<Derived<String>>,
) -> Element {
    let about = description.map(UiText::from);
    let cell = if main {
        view! {
            row width:1fr min-width:0px {}
        }
    } else {
        view! {
            row width:{px(34.0)}px shrink:0 {}
        }
    };
    let button = if main {
        view! {
            CellButton label:(label) on-press:(press_handler) start:true horizontal-inset:(18.0)
                description:(about) {
                { parent.adopt(&content); }
            }
        }
    } else {
        view! {
            CellButton label:(label) on-press:(press_handler) rule:(Rule::Left)
                horizontal-inset:(0.0) {
                { parent.adopt(&content); }
            }
        }
    };
    cell.adopt(&button);
    cell
}

/// The project menu: each project with its directors, then defaults and a
/// new project, one ruled row each.
#[component]
fn ProjectMenu(model: Model, menu: State<bool>) -> Element {
    // Scroll attributes style its content, not its viewport. Bound the viewport
    // explicitly to the fixed-height menu rows so short lists leave room for agents.
    let height = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let directors = snapshot
            .directors
            .iter()
            .filter(|director| {
                snapshot
                    .projects
                    .iter()
                    .any(|project| project.id == director.project_id)
            })
            .count();
        (34.0 * (snapshot.projects.len() + directors + 2) as f32).min(360.0)
    });
    view! {
        col height:{px(height.get())}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            scroll {
                col height:min-content role:list label:"Projects" {
                    for (_, project) in {model.snapshot.get().projects.into_iter().map(|p| (p.id.clone(), p))} {
                        let id = State::new(project.id.clone());
                        let name = State::new(project.name.clone());
                        row height:{px(34.0)}px role:list-item
                            label:{format!("Project row {}", name.get())}
                            fill:{if model.project.get() == id.get() {color(ground.selected)} else {Color::TRANSPARENT}}
                            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                            {
                                parent.adopt(&menu_cell(
                                    Derived::new(move || format!("Select project {}", name.get())),
                                    press(move || { menu.set(false); model.select_project(id.get_untracked()); }),
                                    true,
                                    view! { Ellipsis text:(name) role:(TextRole::Label) },
                                    None,
                                ));
                                parent.adopt(&menu_cell(
                                    Derived::new(move || format!("Project Connections for {}", name.get())),
                                    press(move || { menu.set(false); model.select_project(id.get_untracked()); model.page.set(Page::Connections); }),
                                    false,
                                    view! { icon size:{px(14.0)}px connections-icon },
                                    None,
                                ));
                            }
                        }
                        for (_, director) in {model.snapshot.get().directors.into_iter().filter(|d| d.project_id == id.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>()} {
                            let id = State::new(director.id.clone());
                            let name = State::new(director.name.clone());
                            let project_id = State::new(director.project_id.clone());
                            row height:{px(34.0)}px
                                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                                Elbows
                                    guides:(vec![if model.snapshot.get().directors.iter().rev().find(|d| d.project_id == project_id.get()).is_some_and(|d| d.id == id.get()) { Guide::Last } else { Guide::Branch }])
                                row width:1fr min-width:0px role:list-item
                                    label:{format!("Director row {}", name.get())} {
                                    {
                                        let open = menu_cell(
                                            Derived::new(move || format!("Open director {}", name.get())),
                                            press(move || { menu.set(false); model.open_director(id.get_untracked()); }),
                                            true,
                                            view! {
                                                row width:1fr min-width:0px height:min-content gap:{px(8.0)}px align:center {
                                                    icon size:{px(13.0)}px director-icon
                                                    Ellipsis text:(name) role:(TextRole::Label)
                                                }
                                            },
                                            Some(Derived::new(move || capacity_label(director_capacity(&model.snapshot.get(), &id.get())))),
                                        );
                                        parent.adopt(&open);
                                        parent.adopt(&menu_cell(
                                            Derived::new(move || format!("Profile for {}", name.get())),
                                            press(move || { menu.set(false); model.open_director_profile(id.get_untracked()); }),
                                            false,
                                            view! { icon size:{px(14.0)}px sliders-icon },
                                            None,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    row height:{px(34.0)}px
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        row width:1fr min-width:0px {
                            CellButton label:"Create director" start:true horizontal-inset:(18.0)
                                on-press:(press(move || { menu.set(false); model.open_profile(EditTarget::New); })) {
                                icon size:{px(13.0)}px plus-icon
                                Label text:"New director" role:(TextRole::Label)
                            }
                        }
                        row width:max-content shrink:0 {
                            CellButton label:"Project defaults" rule:(Rule::Left)
                                on-press:(press(move || { menu.set(false); model.open_profile(EditTarget::Defaults); })) {
                                icon size:{px(14.0)}px sliders-icon
                                Label text:"Defaults" role:(TextRole::Label)
                            }
                        }
                    }
                    row height:{px(34.0)}px {
                        CellButton label:"New Project" start:true horizontal-inset:(18.0)
                            on-press:(press(move || { menu.set(false); model.sidebar_open.set(false); model.page.set(Page::NewProject); })) {
                            icon size:{px(13.0)}px plus-icon
                            Label text:"New project" role:(TextRole::Label)
                        }
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
        } else if model.status.get().starts_with("Connecting")
            || model.connection_telemetry.get().2.is_some()
            || model.status.get().contains("retrying")
        {
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
    let clock = State::new(unix_now());
    let telemetry = Derived::new(move || {
        let (samples, since, retry) = model.connection_telemetry.get();
        let connected = model.connected.get();
        let incompatible = model
            .status
            .get()
            .contains("Incompatible Relay server protocol")
            || model.snapshot.get().protocol_version != PROTOCOL_VERSION;
        ConnectionTelemetry {
            samples: samples
                .into_iter()
                .map(|ms| ms.min(u64::from(u32::MAX)) as u32)
                .collect(),
            client_revision: BuildInfo::current().short_revision(),
            server_revision: model
                .server_build
                .get()
                .map(|build| build.short_revision())
                .unwrap_or_else(|| "unknown".into()),
            offline_since: since.map(connection_time),
            retry_in: retry.map(|deadline| {
                deadline
                    .saturating_sub(unix_now().max(clock.get()))
                    .min(u64::from(u32::MAX)) as u32
            }),
            compatible: if incompatible {
                Some(false)
            } else if connected || model.server_build.get().is_some() {
                Some(true)
            } else {
                None
            },
            warning: incompatible.then(|| "client and server protocols incompatible".into()),
        }
    });
    let host = view! {
        col height:min-content shrink:0 label:"Server connection details"
            description:{format!("{} · {}", server_identity(&model.server_endpoint.get()), model.status.get())} {}
    };
    let countdown = host.clone();
    host.animate(move |_| {
        if model.connection_telemetry.get_untracked().2.is_some() {
            clock.set(unix_now());
            countdown.paint_dirty();
        }
        mosaic::widgets::Tick::Sleep(std::time::Duration::from_secs(1))
    });
    host.adopt(&view! {
        ConnectionStatus state:(relay_ui::Live::from(state)) latency:(relay_ui::Live::from(latency))
            sending:(relay_ui::Live::from(Derived::new(move || model.sending())))
            info:(relay_ui::Live::from(info)) telemetry:(relay_ui::Live::from(telemetry))
            on-copy:(Some(press(move || model.ui.get_untracked().set_clipboard_text(model.server_endpoint.get_untracked()))))
    });
    host
}

#[cfg(not(target_arch = "wasm32"))]
fn connection_time(seconds: u64) -> String {
    jiff::Timestamp::from_second(seconds.min(i64::MAX as u64) as i64)
        .map(|time| {
            time.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| "unknown".into())
}
#[cfg(target_arch = "wasm32")]
fn connection_time(seconds: u64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds as f64 * 1000.0));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
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
