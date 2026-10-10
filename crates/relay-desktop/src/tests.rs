use crate::{
    model::{EditTarget, Model, Page},
    network::NetworkState,
    theme, ui,
};
use mosaic::{core::reactive::flush, prelude::*, text::FontContext};
use relay_core::*;
use std::time::Duration;

struct Mounted {
    _scope: Scope,
    ui: Ui,
    model: Model,
    commands: tokio::sync::mpsc::UnboundedReceiver<CommandEnvelope>,
    size: Size,
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self._scope.dispose();
    }
}

fn mount(light: bool, width: f32) -> Mounted {
    mosaic::core::builtins::install();
    install_theme(&theme::palette(light));
    install_theme(&theme::icons());
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    let mut fonts = FontContext::embedded_only();
    crate::fonts::configure(&mut fonts);
    assert!(fonts.has_family("Reddit Sans"));
    assert!(fonts.has_family("Zed Mono"));
    ui.set_fonts(fonts);
    let (sender, commands) = tokio::sync::mpsc::unbounded_channel();
    let model = scope.run(|| Model::new(&ui, sender));
    model.receive(NetworkState {
        snapshot: demo_snapshot(DirectorProfile::default()),
        connected: true,
        status: "Connected".into(),
        ..Default::default()
    });
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&ui::shell(model));
    });
    let mounted = Mounted {
        _scope: scope,
        ui,
        model,
        commands,
        size: Size::new(width, 900.0),
    };
    mounted.settle();
    mounted
}

#[test]
fn first_director_draft_requests_command_discovery_and_completes() {
    let mounted = mount(false, 820.0);
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(sender));
    mounted
        ._scope
        .run(|| crate::command_ui::bind_discovery(mounted.model));
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.sessions.clear();
    snapshot.projects[0].fixture = false;
    let director_id = snapshot.directors[0].id.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_director(director_id);
    mounted.settle();
    let id = mounted.model.session.get_untracked();
    assert!(id.starts_with("director-draft-"));
    assert!(
        matches!(requests.try_recv().unwrap(), crate::buffer_network::Request::Catalog {session, force:false} if session == id)
    );
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("/com".into()), false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            id.clone(),
            Ok(HarnessCatalog {
                commands: vec![HarnessCommand {
                    name: "compact".into(),
                    description: "Compact context".into(),
                    argument_hint: String::new(),
                    dispatch: CommandDispatch::Unavailable,
                    reason: Some("Send the first message before using this command".into()),
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    assert_eq!(
        mounted.model.completion.get_untracked().unwrap().choices[0].label,
        "/compact"
    );
    mounted.key(Key::Tab, false);
    assert!(
        mounted
            .model
            .notice
            .get_untracked()
            .contains("first message")
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &id)),
        "/com"
    );
}

#[test]
fn harness_completion_inserts_durable_skills_and_opens_native_model_flow() {
    let mut mounted = mount(false, 820.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            Ok(HarnessCatalog {
                session_id: "session-plan".into(),
                harness: Harness::ClaudeCode,
                skills: vec![HarnessSkill {
                    id: "native-review".into(),
                    name: "review".into(),
                    description: "Review changes".into(),
                    argument_hint: String::new(),
                    enabled: true,
                    path: None,
                }],
                commands: vec![HarnessCommand {
                    name: "model".into(),
                    description: "Choose a model".into(),
                    argument_hint: String::new(),
                    dispatch: CommandDispatch::Flow,
                    reason: None,
                }],
                models: vec![HarnessModel {
                    id: "test-model".into(),
                    name: "Test model".into(),
                    efforts: vec!["high".into()],
                    default_effort: None,
                    fast: false,
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    let catalog = mounted.model.catalogs.get_untracked();
    mounted.model.catalogs.set(Default::default());
    mounted.settle();
    mounted.key(Key::Character("$rev".into()), false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted.model.catalogs.set(catalog);
    mounted.settle();
    assert_eq!(
        mounted.model.completion.get_untracked().unwrap().choices[0].label,
        "$review"
    );
    let suggestion = mounted.rect("$review");
    assert!(suggestion.size.height > 0.0 && suggestion.size.width > 0.0);
    assert!(suggestion.origin.y >= 0.0 && suggestion.max_y() <= mounted.size.height);
    mounted.key(Key::Tab, false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(selected_skills(&parts)[0].id, "native-review");
    assert_eq!(plain_text(&parts), "$review ");
    mounted.key(Key::Character("λ".into()), false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$review λ"
    );
    let parts = vec![Part::text("/mod")];
    let id = parts[0].id.clone();
    crate::buffer::edit(mounted.model, "session-plan", parts);
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    crate::command_ui::complete(mounted.model, &id, "/mod", 4);
    mounted.settle();
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.command_flow.get_untracked(), "model");
    mounted.click("Choose model Test model");
    mounted.click("Choose effort high");
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(envelope.command,Command::SetHarnessSelection{selection:HarnessSelection{model:Some(ref model),effort:Some(ref effort),..},..} if model=="test-model" && effort=="high")
    );
}

#[test]
fn command_completion_escape_and_ime_keep_normal_editing() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted
        .model
        .catalogs
        .set(std::collections::BTreeMap::from([(
            "session-plan".into(),
            Ok(HarnessCatalog {
                skills: vec![HarnessSkill {
                    id: "x".into(),
                    name: "review".into(),
                    description: String::new(),
                    argument_hint: String::new(),
                    enabled: true,
                    path: None,
                }],
                ..Default::default()
            }),
        )]));
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|s| s.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted.key(Key::Character("$".into()), false);
    assert!(mounted.model.completion.get_untracked().is_some());
    mounted.key(Key::Escape, false);
    assert!(mounted.model.completion.get_untracked().is_none());
    mounted.key(Key::Enter, false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n"
    );
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "確定".into(),
        cursor: Some((0, 6)),
    });
    mounted.settle();
    mounted.key(Key::Enter, false);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n"
    );
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "$\n確定"
    );
    assert!(mounted.commands.is_empty());
}

#[test]
fn sidebar_tree_disclosure_keyboard_and_secondary_actions_are_independent() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    let focused_label = || {
        let focused = mounted.ui.focused().unwrap().id();
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| n.id == focused)
            .unwrap()
            .label
            .unwrap()
    };
    mounted.focus("Toggle project Relay · demo");
    mounted.key(Key::ArrowLeft, false);
    assert!(
        !mounted
            .model
            .expanded_projects
            .get_untracked()
            .contains("demo")
    );
    mounted.key(Key::ArrowRight, false);
    mounted.key(Key::ArrowRight, false);
    assert_eq!(focused_label(), "Open director Project director");
    mounted.key(Key::ArrowRight, false);
    mounted.key(Key::ArrowRight, false);
    assert_eq!(focused_label(), "Open worker Profile validation");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.session.get_untracked(), "session-worker");
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(focused_label(), "Open director Project director");
    mounted.key(Key::ArrowLeft, false);
    assert!(
        !mounted
            .model
            .expanded_directors
            .get_untracked()
            .contains("director-main")
    );
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(focused_label(), "Toggle project Relay · demo");
    mounted.key(Key::End, false);
    assert_eq!(focused_label(), "Open director Review director");
    mounted.key(Key::ArrowUp, false);
    assert_eq!(focused_label(), "Open director Project director");
    mounted.key(Key::Home, false);
    assert_eq!(focused_label(), "Toggle project Relay · demo");
    mounted.click("Toggle project Relay · demo");
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert_eq!(mounted.model.session.get_untracked(), "session-worker");
    mounted.model.open_session("session-worker".into());
    mounted.settle();
    assert!(
        mounted
            .model
            .expanded_projects
            .get_untracked()
            .contains("demo")
    );
    assert!(
        mounted
            .model
            .expanded_directors
            .get_untracked()
            .contains("director-main")
    );
    mounted.rect("Open worker Profile validation");
    mounted.click("Profile for Project director");
    assert_eq!(mounted.model.page.get_untracked(), Page::Directors);
    assert_eq!(
        mounted.model.editor.get_untracked(),
        EditTarget::Director("director-main".into())
    );
    mounted.click("Open director Project director");
    assert_eq!(mounted.model.session.get_untracked(), "session-plan");
    mounted.click("Open board for Relay · demo");
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
    assert!(mounted.model.issue.get_untracked().is_none());
    // A profile/transcript viewport's scale binding must die with that view.
    mounted.click("Settings");
    mounted.focus("Theme: Light");
    mounted.click("Theme: Light");
    mounted.click("Increase interface scale");
    let snapshot = mounted.ui.inspection_snapshot();
    let sidebar = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Sidebar"))
        .unwrap()
        .id;
    for node in snapshot.nodes.iter().filter(|n| n.role == Role::Button) {
        let mut parent = node.parent;
        let mut button_ancestor = false;
        while let Some(id) = parent {
            if id == sidebar {
                assert!(!button_ancestor, "Nested sidebar control: {:?}", node.label);
                break;
            }
            let ancestor = snapshot.nodes.iter().find(|n| n.id == id).unwrap();
            button_ancestor |= ancestor.role == Role::Button;
            parent = ancestor.parent;
        }
    }
}

#[test]
fn sidebar_tree_groups_by_project_and_director_and_reacts_without_losing_expansion() {
    for light in [false, true] {
        let mounted = mount(light, 820.0);
        mounted.click("Toggle director Project director");
        let mut snapshot = mounted.model.snapshot.get_untracked();
        let mut project = snapshot.projects[0].clone();
        project.id = "other".into();
        project.name = "Other project".into();
        snapshot.projects.push(project);
        let mut director = snapshot.directors[0].clone();
        director.id = "director-other".into();
        director.project_id = "other".into();
        director.name = "Other director".into();
        snapshot.directors.push(director);
        let mut worker = snapshot.sessions[1].clone();
        worker.id = "other-worker".into();
        worker.project_id = "other".into();
        worker.director_id = "director-other".into();
        worker.title = "Other worker".into();
        snapshot.sessions.push(worker);
        mounted.model.receive(NetworkState {
            snapshot: snapshot.clone(),
            connected: true,
            ..Default::default()
        });
        mounted.settle();
        mounted.rect("Open worker Profile validation");
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Open director Other director"))
        );
        mounted.click("Toggle project Other project");
        assert_eq!(mounted.model.project.get_untracked(), "demo");
        mounted.click("Toggle director Other director");
        mounted.rect("Open worker Other worker");
        snapshot.directors[0].name = "Renamed director".into();
        snapshot.sessions[1].title = "Renamed worker".into();
        snapshot.projects.reverse();
        snapshot.revision += 1;
        mounted.model.receive(NetworkState {
            snapshot,
            connected: true,
            ..Default::default()
        });
        mounted.settle();
        let project = mounted.rect("Toggle project Relay · demo");
        let director = mounted.rect("Open director Renamed director");
        let worker = mounted.rect("Open worker Renamed worker");
        assert!(project.origin.x < director.origin.x && director.origin.x < worker.origin.x);
        assert!(project.origin.y < director.origin.y && director.origin.y < worker.origin.y);
        let review = mounted.rect("Open director Review director");
        assert!(worker.origin.y < review.origin.y);
        mounted.click("Open worker Other worker");
        assert_eq!(mounted.model.project.get_untracked(), "other");
        assert_eq!(mounted.model.session.get_untracked(), "other-worker");
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect("Open worker Other worker").center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.ui.tick(Duration::from_millis(600));
        mounted.settle();
        assert!(
            mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.role == Role::Tooltip && n.label.as_deref() == Some("Worker"))
        );
        mounted.click("Open director Other director");
        assert_eq!(mounted.model.page.get_untracked(), Page::DirectorStart);
        assert_eq!(
            mounted.model.worker_director.get_untracked(),
            "director-other"
        );
        mounted.rect("Next message");
        mounted.rect("Send message");
        mounted.click("Create director in Relay · demo");
        assert_eq!(mounted.model.project.get_untracked(), "demo");
        assert_eq!(mounted.model.editor.get_untracked(), EditTarget::New);
        mounted.click("Project defaults for Other project");
        assert_eq!(mounted.model.project.get_untracked(), "other");
        assert_eq!(mounted.model.editor.get_untracked(), EditTarget::Defaults);
    }
}

#[test]
fn sidebar_footer_stays_fixed_while_large_trees_scroll_and_scale() {
    let mut mounted = mount(false, 820.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.size = Size::new(820.0, 360.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    for i in 0..30 {
        let mut director = snapshot.directors[0].clone();
        director.id = format!("extra-{i}");
        director.name = format!("Director {i}");
        snapshot.directors.push(director);
    }
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    let before = mounted.rect("Settings");
    assert!(before.origin.y > 300.0 && before.origin.y + before.size.height <= 360.0);
    mounted.focus("Open director Director 29");
    assert_eq!(mounted.rect("Settings"), before);
    mounted.focus("Settings");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
    mounted.model.preferences.update(|p| {
        p.scale = 1.5;
        p.sidebar_width = 160.0;
    });
    mounted.settle();
    let settings = mounted.rect("Settings");
    let sidebar = mounted.rect("Sidebar");
    assert!((settings.size.height / before.size.height - 1.5).abs() < 0.05);
    assert!(settings.origin.y + settings.size.height <= 360.0);
    assert!(settings.origin.x + settings.size.width <= sidebar.size.width);
    mounted.click("Settings");
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
}

#[test]
fn sidebar_connection_details_fit_narrow_widths_and_connection_states() {
    for sidebar_width in [160.0, 220.0, 360.0] {
        let mounted = mount(false, 1380.0);
        mounted
            .model
            .preferences
            .update(|p| p.sidebar_width = sidebar_width);
        mounted
            .model
            .server_endpoint
            .set("https://relay.example:7440/".into());
        mounted.click("Server connection details");
        let settings = mounted.rect("Settings");
        let revision = mounted.rect("Workspace revision");
        assert!(revision.size.width >= settings.size.width);
        assert!((revision.origin.x - settings.origin.x - settings.size.width).abs() < 1.0);
        let indicator = mounted.rect("Connection indicator");
        let sidebar = mounted.rect("Sidebar");
        assert!(
            (indicator.origin.x + indicator.size.width - sidebar.size.width + 12.0).abs() < 1.0
        );
        assert!(!has_label(&mounted, "Revision meaning"));
        for sample in [Some(42), Some(180), Some(480), None] {
            mounted.model.round_trip_ms.set(sample);
            mounted.settle();
            let snapshot = mounted.ui.inspection_snapshot();
            let sidebar = mounted.rect("Sidebar");
            for label in [
                "Workspace revision",
                "Server address",
                "Connection transport",
                "Server protocol",
                "Connection quality",
                "Connection status",
            ] {
                let head = snapshot
                    .nodes
                    .iter()
                    .find(|n| n.label.as_deref() == Some(label))
                    .unwrap();
                for node in &snapshot.nodes {
                    let mut parent = node.parent;
                    while let Some(id) = parent {
                        if id == head.id {
                            let rect = node.rect;
                            assert!(
                                rect.origin.x >= sidebar.origin.x - 1.0
                                    && rect.origin.x + rect.size.width
                                        <= sidebar.origin.x + sidebar.size.width + 1.0,
                                "{label} at {sidebar_width}px, {sample:?}: {:?} {rect:?} outside {sidebar:?}",
                                node.label
                            );
                            break;
                        }
                        parent = snapshot.nodes.iter().find(|n| n.id == id).unwrap().parent;
                    }
                }
            }
        }
    }
}

#[test]
fn sidebar_server_details_expose_endpoint_quality_and_header_status() {
    let mounted = mount(false, 1380.0);
    mounted
        .model
        .server_endpoint
        .set("https://relay.example:7440/".into());
    mounted.model.round_trip_ms.set(Some(42));

    mounted.click("Server connection details");

    for label in [
        "Server connection details",
        "Server address",
        "Connection transport",
        "Server protocol",
        "Connection quality",
        "Connection status",
    ] {
        assert!(has_label(&mounted, label), "Missing details row: {label}");
    }
    assert_eq!(mounted.model.round_trip_ms.get_untracked(), Some(42));
    mounted.click("Server connection details");
    assert!(!has_label(&mounted, "Server address"));
}

#[test]
fn approval_control_scales_with_its_label_and_keeps_the_existing_approval() {
    let mounted = mount(false, 1380.0);
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.click("Open issue #2");
    let before = mounted.rect("Approve implementation for this turn");
    mounted.model.worker_approval.set(true);
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted.settle();
    let after = mounted.rect("Approve implementation for this turn");
    assert!((after.size.height / before.size.height - 1.5).abs() < 0.05);
    assert!(mounted.model.worker_approval.get_untracked());
    mounted.focus("Approve implementation for this turn");
    mounted.key(Key::Space, false);
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn display_settings_persist_validate_and_preserve_existing_file_on_failure() {
    use crate::settings::{Preferences, ThemeMode};
    let directory = std::env::temp_dir().join(format!("relay-settings-{}", uuid::Uuid::new_v4()));
    let path = directory.join("settings.toml");
    let mut preferences = Preferences::load(&path).unwrap();
    assert_eq!(preferences.mode, ThemeMode::Dark);
    assert_eq!(preferences.scale, 1.0);
    preferences.mode = ThemeMode::System;
    preferences.dark_neutral = true;
    preferences.light_warm = true;
    preferences.scale = 1.5;
    preferences.sidebar_width = 300.0;
    preferences.save(&path).unwrap();
    assert_eq!(Preferences::load(&path).unwrap(), preferences);
    let before = std::fs::read(&path).unwrap();
    preferences.scale = f32::NAN;
    assert!(preferences.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, "scale = 8.0").unwrap();
    assert!(Preferences::load(&path).is_err());
    std::fs::write(&path, "broken syntax").unwrap();
    assert!(Preferences::load(&path).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn settings_work_disconnected_and_scale_layout_and_hit_targets_without_losing_drafts() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.receive(NetworkState::default());
    mounted.model.worker_prompt.set("Keep this draft".into());
    mounted.key(Key::Character(",".into()), true);
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
    let before = mounted.rect("Theme: Dark");
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted.settle();
    let after = mounted.rect("Theme: Dark");
    assert!((after.size.height / before.size.height - 1.5).abs() < 0.05);
    assert!((mounted.rect("Sidebar").size.width - 330.0).abs() < 1.0);
    mounted.focus("Theme: Light");
    mounted.click("Theme: Light");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::Light
    );
    let paper = mosaic::core::theme::color(theme::surface.base);
    mounted.focus("Light palette: Warm");
    mounted.click("Light palette: Warm");
    assert_ne!(mosaic::core::theme::color(theme::surface.base), paper);
    mounted.focus("Theme: Dark");
    mounted.click("Theme: Dark");
    let slate = mosaic::core::theme::color(theme::surface.base);
    mounted.focus("Dark palette: Neutral");
    mounted.click("Dark palette: Neutral");
    assert_ne!(mosaic::core::theme::color(theme::surface.base), slate);
    mounted.focus("Theme: System");
    mounted.click("Theme: System");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
    assert_eq!(mosaic::core::theme::scalar(theme::ui_scale), 1.5);
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Keep this draft"
    );
    assert!(mounted.commands.is_empty());
}

#[test]
fn sidebar_drag_clamps_width_and_settings_remain_keyboard_reachable_in_short_window() {
    let mut mounted = mount(false, 820.0);
    let sidebar = mounted.rect("Sidebar");
    let start = Vector2::new(sidebar.origin.x + sidebar.size.width - 1.0, 80.0);
    for (kind, position) in [
        (PointerEventKind::Down(PointerButton::Primary), start),
        (PointerEventKind::Move, Vector2::new(700.0, 80.0)),
        (
            PointerEventKind::Up(PointerButton::Primary),
            Vector2::new(700.0, 80.0),
        ),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
    }
    assert_eq!(
        mounted.model.preferences.get_untracked().sidebar_width,
        328.0
    );
    assert!((mounted.rect("Sidebar").size.width - 328.0).abs() < 1.0);
    mounted.size = Size::new(820.0, 360.0);
    mounted.settle();
    mounted.focus("Settings");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
    mounted.key(Key::Character("k".into()), true);
    mounted.focus("Reset sidebar width");
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.preferences.get_untracked().sidebar_width,
        220.0
    );
}

#[test]
fn large_scale_settings_keep_controls_visible_in_a_narrow_window() {
    let mut mounted = mount(false, 820.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| {
        p.scale = 2.0;
        p.sidebar_width = 160.0;
    });
    mounted.size = Size::new(820.0, 600.0);
    mounted.key(Key::Character(",".into()), true);
    let nodes = mounted.ui.inspection_snapshot();
    let number = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("200"))
        .unwrap();
    let details = mounted.ui.inspection_details(number.id).unwrap();
    assert_eq!(
        details
            .attributes
            .iter()
            .find(|a| a.name == "font-size")
            .unwrap()
            .value,
        "28.0"
    );
    for label in [
        "Theme: System",
        "Decrease interface scale",
        "Reset interface scale",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(rect.origin.x >= 320.0, "{label}: {rect:?}");
        assert!(
            rect.origin.x + rect.size.width <= 820.0,
            "{label}: {rect:?}"
        );
    }
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.0);
}

impl Mounted {
    fn settle(&self) {
        flush();
        for _ in 0..12 {
            self.ui.tick(Duration::from_millis(16));
            flush();
            if self.ui.frame(self.size, 1.0).is_none() {
                break;
            }
        }
    }
    fn rect(&self, label: &str) -> Rect {
        self.ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing control: {label}"))
            .rect
    }
    fn click(&self, label: &str) {
        let position = self.rect(label).center();
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            self.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        self.settle();
    }
    fn focus(&self, label: &str) {
        let snapshot = self.ui.inspection_snapshot();
        let target = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing control: {label}"))
            .id;
        for _ in 0..snapshot.nodes.len() {
            self.key(Key::Tab, false);
            if self
                .ui
                .focused()
                .is_some_and(|element| element.id() == target)
            {
                self.settle();
                let rect = self.rect(label);
                assert!(
                    rect.origin.y >= 0.0
                        && rect.origin.y + rect.size.height <= self.size.height + 0.01,
                    "{label}: {rect:?}"
                );
                return;
            }
        }
        panic!("Control is not keyboard reachable: {label}");
    }
    fn key(&self, key: Key, command: bool) {
        self.ui.dispatch_key(KeyEvent {
            kind: KeyEventKind::Down { repeat: false },
            key,
            modifiers: Modifiers {
                ctrl: command && !cfg!(target_os = "macos"),
                meta: command && cfg!(target_os = "macos"),
                ..Default::default()
            },
        });
        self.settle();
    }
}

fn touch(mounted: &Mounted, kind: PointerEventKind, x: f32, y: f32) {
    mounted.ui.dispatch_pointer(PointerEvent {
        kind,
        position: Vector2::new(x, y),
        pointer_type: PointerType::Touch,
        modifiers: Modifiers::default(),
        timestamp: Duration::from_millis(100),
    });
    mounted.settle();
}

#[test]
fn phone_shell_reserves_safe_areas_without_scaling_or_squeezing_the_board() {
    for light in [false, true] {
        let mounted = mount(light, 390.0);
        mounted.ui.set_safe_area(mosaic::layout::ResolvedEdges {
            top: 59.0,
            bottom: 34.0,
            left: 0.0,
            right: 0.0,
        });
        mounted.settle();
        assert!(mounted.model.mobile_navigation.get_untracked());
        assert!(!mounted.model.sidebar_open.get_untracked());
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Sidebar"))
        );
        let content = mounted.rect("Main content");
        assert_eq!(content.origin, Vector2::new(0.0, 59.0));
        assert_eq!(content.size, Size::new(390.0, 807.0));
        assert_eq!(mounted.rect("Page header").origin.y, 59.0);
        mounted.model.browser_top_clearance.set(16.0);
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 75.0);
        mounted.model.browser_top_clearance.set(0.0);
        mounted.settle();
        let menu = mounted.rect("Open navigation");
        assert!(menu.size.width >= 44.0 && menu.size.height >= 44.0);
        mounted.click("Open navigation");
        let sidebar = mounted.rect("Sidebar");
        assert_eq!(sidebar.origin.y, 59.0);
        assert_eq!(sidebar.size.width, 320.0);
        assert_eq!(sidebar.origin.y + sidebar.size.height, 866.0);
        mounted.click("Close navigation");
        mounted.model.preferences.update(|p| p.scale = 2.0);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 59.0);
        mounted.model.browser_top_clearance.set(16.0);
        mounted.settle();
        assert_eq!(mounted.rect("Page header").origin.y, 75.0);
        mounted.ui.set_safe_area(mosaic::layout::ResolvedEdges {
            left: 24.0,
            right: 24.0,
            top: 0.0,
            bottom: 21.0,
        });
        mounted.settle();
        assert_eq!(mounted.rect("Main content").origin.x, 24.0);
        assert_eq!(mounted.rect("Main content").size.width, 342.0);
    }
}

#[test]
fn phone_navigation_tracks_edge_swipes_and_dismisses_without_activating_content() {
    let mounted = mount(false, 390.0);
    let down = PointerEventKind::Down(PointerButton::Primary);
    let up = PointerEventKind::Up(PointerButton::Primary);
    // A vertical edge scroll and a horizontal gesture in the board stay there.
    for (x, dx, dy) in [(10.0, 8.0, 80.0), (100.0, 180.0, 0.0)] {
        touch(&mounted, down, x, 150.0);
        touch(&mounted, PointerEventKind::Move, x + dx, 150.0 + dy);
        touch(&mounted, up, x + dx, 150.0 + dy);
        assert!(!mounted.model.sidebar_open.get_untracked());
    }
    touch(&mounted, down, 10.0, 150.0);
    touch(&mounted, PointerEventKind::Move, 90.0, 152.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    assert_eq!(mounted.rect("Sidebar").size.width, 320.0);
    assert!((mounted.rect("Sidebar").origin.x + 240.0).abs() < 1.0);
    touch(&mounted, PointerEventKind::Move, 230.0, 152.0);
    touch(&mounted, up, 230.0, 152.0);
    assert!(mounted.model.sidebar_open.get_untracked());
    assert!(mounted.commands.is_empty());
    assert!(mounted.model.issue.get_untracked().is_none());
    touch(&mounted, down, 200.0, 70.0);
    touch(&mounted, PointerEventKind::Move, 25.0, 70.0);
    touch(&mounted, up, 25.0, 70.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    // A cancelled drag leaves the initial state intact.
    touch(&mounted, down, 10.0, 150.0);
    touch(&mounted, PointerEventKind::Move, 200.0, 150.0);
    touch(&mounted, PointerEventKind::Cancel, 200.0, 150.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    mounted.key(Key::Escape, false);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    touch(&mounted, down, 370.0, 200.0);
    touch(&mounted, up, 370.0, 200.0);
    assert!(!mounted.model.sidebar_open.get_untracked());
    mounted.click("Open navigation");
    mounted.click("Open board for Relay · demo");
    assert!(!mounted.model.sidebar_open.get_untracked());
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
}

#[test]
fn resizing_between_phone_and_desktop_keeps_the_conversation_editor_mounted() {
    let mut mounted = mount(false, 820.0);
    mounted.click("Open issue #2");
    mounted.click("Profile design");
    let editor = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.role == Role::TextInput
                && n.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Draft text "))
        })
        .map(|n| n.id);
    mounted.size.width = 390.0;
    mounted.settle();
    assert!(mounted.model.mobile_navigation.get_untracked());
    mounted.click("Open navigation");
    mounted.click("Close navigation");
    mounted.size.width = 820.0;
    mounted.settle();
    assert!(!mounted.model.mobile_navigation.get_untracked());
    mounted.rect("Sidebar");
    let after = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.role == Role::TextInput
                && n.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Draft text "))
        })
        .map(|n| n.id);
    assert!(editor.is_some(), "Conversation editor must exist");
    assert_eq!(editor, after);
}

#[test]
fn board_issue_and_session_navigation_work_in_both_themes_and_widths() {
    for light in [true, false] {
        for width in [820.0, 1380.0] {
            let mounted = mount(light, width);
            let card = mounted.rect("Open issue #2");
            assert!(card.size.width > 100.0);
            mounted.click("Open issue #2");
            assert_eq!(
                mounted.model.issue.get_untracked().as_deref(),
                Some("issue-2")
            );
            mounted.click("Profile design");
            assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
            mounted.rect("Message m2");
            mounted.click("Session actions");
            mounted.click("Linked issue");
            assert_eq!(mounted.model.page.get_untracked(), Page::Board);
            assert_eq!(
                mounted.model.issue.get_untracked().as_deref(),
                Some("issue-2")
            );
        }
    }
}

#[test]
fn command_palette_shares_edges_scrolls_and_dismisses_without_clicking_through() {
    for (width, scale) in [(1380.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        mounted.focus("Settings");
        let previous = mounted.ui.focused().unwrap().id();
        mounted.key(Key::Character("k".into()), true);
        let panel = mounted.rect("Command palette panel");
        let search = mounted.rect("Command search");
        let results = mounted.rect("Command results");
        assert!((search.origin.x - panel.origin.x).abs() < 1.0);
        assert!((search.size.width - panel.size.width).abs() < 1.0);
        assert!(panel.origin.y >= 0.0 && panel.origin.y + panel.size.height <= mounted.size.height);
        assert!(
            results.size.height > 0.0
                && results.origin.y + results.size.height <= panel.origin.y + panel.size.height
        );
        for label in ["Open board", "Open sessions", "Reset sidebar width"] {
            let action = mounted.rect(label);
            assert!((action.origin.x - panel.origin.x).abs() < 1.0);
            assert!((action.size.width - panel.size.width).abs() < 1.0);
        }
        mounted.click("Command search");
        assert!(mounted.model.palette.get_untracked());
        let page = mounted.model.page.get_untracked();
        mounted.click("Settings");
        assert!(!mounted.model.palette.get_untracked());
        assert_eq!(mounted.model.page.get_untracked(), page);
        assert_eq!(mounted.ui.focused().unwrap().id(), previous);
        mounted.key(Key::Character("k".into()), true);
        mounted.model.palette_query.set("not a command".into());
        mounted.settle();
        assert!(has_label(&mounted, "No matching commands"));
        mounted.key(Key::Enter, false);
        assert!(mounted.model.palette.get_untracked());
        mounted.key(Key::Escape, false);
        assert!(!mounted.model.palette.get_untracked());
    }
}

#[test]
fn command_palette_enter_runs_the_keyboard_selected_row() {
    let mounted = mount(false, 1380.0);
    mounted.key(Key::Character("k".into()), true);
    mounted.key(Key::ArrowDown, false);
    mounted.key(Key::ArrowDown, false);
    mounted.key(Key::Enter, false);
    assert!(!mounted.model.palette.get_untracked());
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
}

#[test]
fn palette_and_search_open_with_keyboard_focus() {
    let mounted = mount(false, 1380.0);
    mounted.key(Key::Character("k".into()), true);
    assert!(mounted.model.palette.get_untracked());
    let focused = mounted.ui.focused().unwrap().id();
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.id == focused)
            .unwrap()
            .label
            .as_deref(),
        Some("Command search")
    );
    mounted.model.palette_query.set("sessions".into());
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    mounted.key(Key::Character("f".into()), true);
    assert!(mounted.model.searching.get_untracked());
    mounted.model.search.set("explicit overrides".into());
    mounted.settle();
    let labels: Vec<_> = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter_map(|n| n.label)
        .collect();
    assert!(labels.iter().any(|l| l == "Message m1"));
    // Find does not remove the surrounding conversation from the buffer.
    assert!(labels.iter().any(|l| l == "Message m3"));
    mounted.key(Key::Escape, false);
    assert!(!mounted.model.searching.get_untracked());
}

#[test]
fn typing_in_recorded_text_creates_an_inline_reply_and_enter_is_a_newline() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    mounted.click("Message m1");
    mounted.key(Key::Character("Feedback".into()), false);
    mounted.key(Key::Enter, false);
    mounted.key(Key::Character("Another line".into()), false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    let PartKind::Reply {
        anchor,
        parts: reply,
    } = &parts[0].kind
    else {
        panic!("No anchored reply")
    };
    assert_eq!(anchor.message_id, "m1");
    assert_eq!(plain_text(reply), "Feedback\nAnother line");
    assert_eq!(mounted.model.snapshot.get_untracked().comments.len(), 0);
    let focused = mounted.ui.focused().unwrap().id();
    assert!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.id == focused)
            .unwrap()
            .label
            .as_deref()
            .unwrap()
            .starts_with("Draft text")
    );
}

#[test]
fn long_profile_forms_scroll_without_squeezing_permission_controls() {
    let mounted = mount(false, 820.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let plan = mounted.rect("Plan permission");
    let delegate = mounted.rect("Delegate permission");
    assert!(plan.size.height >= 30.0);
    assert!(delegate.origin.y >= plan.origin.y + plan.size.height - 0.5);
    let save = mounted.rect("Save profile");
    assert!(save.origin.y + save.size.height <= mounted.size.height);
}

#[test]
fn comments_keep_drafts_after_failure_and_clear_only_on_acknowledgment() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let source = mounted
        .model
        .snapshot
        .get_untracked()
        .messages
        .iter()
        .find(|m| m.id == "m2")
        .unwrap()
        .clone();
    mounted.model.start_comment(&source);
    mounted.model.comment_quote.set("project defaults".into());
    mounted
        .model
        .comment_body
        .set("Preserve the override.".into());
    mounted.model.save_comment();
    let request = mounted.commands.try_recv().unwrap();
    let failure = NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((request.request_id.clone(), Err("Network failed".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    };
    mounted.model.receive(failure.clone());
    assert_eq!(
        mounted.model.comment_body.get_untracked(),
        "Preserve the override."
    );
    mounted.model.save_comment();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(request.request_id, retry.request_id);
    // A held previous outcome must not complete a new retry.
    mounted.model.receive(failure);
    assert!(mounted.model.busy.get_untracked());
    let mut saved = mounted.model.snapshot.get_untracked();
    saved.apply(retry.command, &retry.request_id, 123).unwrap();
    mounted.model.receive(NetworkState {
        snapshot: saved,
        connected: true,
        outcome: Some((retry.request_id, Ok(()))),
        outcome_serial: 2,
        ..Default::default()
    });
    assert!(mounted.model.comment_body.get_untracked().is_empty());
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert_eq!(mounted.model.snapshot.get_untracked().comments.len(), 1);
}

#[test]
fn profile_controls_preserve_inheritance_and_reject_invalid_imports() {
    let mut mounted = mount(true, 1380.0);
    mounted.model.open_profile(EditTarget::New);
    mounted.settle();
    mounted.focus("Increase worker limit");
    mounted.click("Increase worker limit");
    assert_eq!(mounted.model.editor_profile.get_untracked().max_workers, 5);
    assert_eq!(mounted.model.origin("max_workers"), "Director override");
    mounted.focus("Inherit max_workers");
    mounted.click("Inherit max_workers");
    assert_eq!(mounted.model.editor_profile.get_untracked().max_workers, 4);
    mounted.model.toml.set("unknown_setting = true".into());
    mounted.model.import_toml();
    assert!(mounted.model.notice.get_untracked().contains("unknown"));
    mounted.model.editor_name.set("Security director".into());
    mounted.model.save_profile();
    let request = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(request.command, &request.request_id, 123)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id.clone(), Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.editor.get_untracked(),
        EditTarget::Director(format!("director-{}", request.request_id))
    );
    assert_eq!(mounted.model.snapshot.get_untracked().directors.len(), 3);
}

fn live_snapshot() -> Snapshot {
    let mut defaults = DirectorProfile::default();
    defaults
        .permissions
        .insert(Task::Implement, Permission::Ask);
    let mut snapshot = demo_snapshot(defaults);
    snapshot.projects[0].fixture = false;
    snapshot.projects[0].github = Some(GitHubProject {
        owner: "team".into(),
        number: 7,
        url: "https://github.com/orgs/team/projects/7".into(),
        last_synced_at: None,
        sync_error: None,
    });
    snapshot.directors[0].overrides = ProfileOverrides::default();
    snapshot
}

#[test]
fn live_worker_approval_failure_retry_and_ack_open_exact_session() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Implement the issue".into());
    mounted.settle();
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Approve")
    );
    mounted.focus("Approve implementation for this turn");
    mounted.click("Approve implementation for this turn");
    mounted.model.run_worker(false);
    let request = mounted.commands.try_recv().unwrap();
    assert!(matches!(
        request.command,
        Command::StartSession {
            approve_implementation: true,
            ..
        }
    ));
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            request.request_id.clone(),
            Err("Revision conflict; review latest".into()),
        )),
        outcome_serial: 1,
        outcome_conflict: true,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Implement the issue"
    );
    mounted.model.review_latest();
    mounted.model.retry_pending();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(retry.request_id, request.request_id);
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((retry.request_id.clone(), Ok(()))),
        outcome_serial: 2,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.session.get_untracked(),
        format!("session-{}", retry.request_id)
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert!(mounted.model.worker_prompt.get_untracked().is_empty());
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn delayed_start_acknowledgement_opens_the_sessions_own_project() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let mut other = snapshot.projects[0].clone();
    other.id = "other-project".into();
    other.repository = "team/other".into();
    snapshot.projects.push(other);
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Implement this issue".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let request = mounted.commands.try_recv().unwrap();
    mounted.model.select_project("other-project".into());
    let mut session = snapshot.sessions[0].clone();
    session.id = format!("session-{}", request.request_id);
    snapshot.sessions.push(session.clone());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), session.project_id);
    assert_eq!(mounted.model.session.get_untracked(), session.id);
    assert_eq!(
        mounted.model.worker_director.get_untracked(),
        session.director_id
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert!(!mounted.model.worker_approval.get_untracked());
}

#[test]
fn deny_scope_harness_and_capacity_cannot_be_overridden() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Implement".into());
    mounted.model.worker_approval.set(true);
    mounted.model.snapshot.update(|s| {
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Deny);
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("denied")
    );
    mounted.model.snapshot.update(|s| {
        s.projects[0]
            .defaults
            .permissions
            .insert(Task::Implement, Permission::Allow);
        s.projects[0].defaults.scope = DirectorScope::Issues {
            issue_ids: vec!["other".into()],
        };
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("scope")
    );
    mounted.model.snapshot.update(|s| {
        s.projects[0].defaults.scope = DirectorScope::Project;
        s.projects[0].defaults.harness = Harness::ClaudeCode;
    });
    assert!(mounted.model.worker_gate(false).is_ok());
    mounted.model.snapshot.update(|s| {
        s.projects[0].defaults.harness = Harness::Codex;
        s.projects[0].defaults.max_workers = 0;
    });
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("slots")
    );
}

#[test]
fn worker_running_stop_completed_review_and_continue_use_recorded_session() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.director_id = snapshot.directors[0].id.clone();
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread-live".into()),
        worktree: Some("/repo/worktrees/live".into()),
        branch: Some("worker/live".into()),
        base_commit: Some("abc123".into()),
        error: None,
        usage: Some(TokenUsage {
            input_tokens: 42,
            cached_input_tokens: 12,
            output_tokens: 7,
        }),
        changes: Some(ChangeSet {
            files: vec!["file.rs".into()],
            diff: "+change".into(),
            truncated: true,
        }),
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    mounted.focus("Stop worker");
    mounted.click("Stop worker");
    let stop = mounted.commands.try_recv().unwrap();
    assert!(matches!(stop.command, Command::StopWorker { session_id: id } if id == session_id));
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((stop.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    mounted.click("Session actions");
    mounted.focus("Toggle change review");
    mounted.click("Toggle change review");
    mounted.rect("Session details");
    mounted.model.worker_prompt.set("Review result".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(true);
    let send = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&send.command, Command::SendWorker { session_id: id, .. } if *id == session_id)
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            send.request_id.clone(),
            Err("Continuation response lost".into()),
        )),
        outcome_serial: 2,
        outcome_ambiguous: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.worker_prompt.get_untracked(), "Review result");
    mounted.model.worker_prompt.set("Changed prompt".into());
    mounted.model.retry_pending();
    let exact = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&send).unwrap()
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((exact.request_id, Ok(()))),
        outcome_serial: 3,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Changed prompt"
    );
    mounted.model.worker_approval.set(true);
    for status in [
        WorkerStatus::Failed,
        WorkerStatus::Stopped,
        WorkerStatus::Interrupted,
    ] {
        mounted
            .model
            .snapshot
            .update(|s| s.sessions[0].worker.as_mut().unwrap().status = status);
        mounted.settle();
        assert!(mounted.model.worker_gate(true).is_ok());
        mounted.rect("Next message");
    }
}

#[test]
fn live_initial_selection_and_dynamic_columns_metadata_remain_reactive() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let mut fixture = snapshot.projects[0].clone();
    fixture.id = "fixture-project".into();
    fixture.fixture = true;
    fixture.github = None;
    snapshot.projects[0].id = "remote-project".into();
    for issue in &mut snapshot.issues {
        issue.project_id = "remote-project".into();
    }
    for director in &mut snapshot.directors {
        director.project_id = "remote-project".into();
    }
    let mut historical = snapshot.projects[0].clone();
    historical.id = "historical-remote".into();
    historical.github.as_mut().unwrap().number = 6;
    snapshot.projects.push(historical);
    snapshot.projects.insert(0, fixture);
    mounted.model.project.set(String::new());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), "remote-project");
    mounted.settle();
    mounted.model.snapshot.update(|s| {
        s.projects[1].columns.reverse();
        s.projects[1].columns[0].title = "Changed status".into();
        s.projects[1].columns.push(BoardColumn {
            id: "new-column".into(),
            title: "New status".into(),
        });
        s.issues[1].title = "Updated issue".into();
        s.issues[1].body = "Updated body".into();
        s.projects[1].github.as_mut().unwrap().sync_error = Some("Sync failed".into());
    });
    mounted.settle();
    mounted.rect("Sync project");
    mounted.rect("Column new-column");
    let snapshot = mounted.model.snapshot.get_untracked();
    let columns = &snapshot.projects[1].columns;
    let first = mounted.rect(&format!("Column {}", columns[0].id));
    let second = mounted.rect(&format!("Column {}", columns[1].id));
    assert!(first.origin.x < second.origin.x);
    mounted.rect("Updated issue");
    mounted.click("Open issue #2");
    mounted.rect("Updated body");
    mounted.rect("Start worker");
}

#[test]
fn removed_board_items_keep_history_but_block_new_turns_until_restored() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let issue = snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == "issue-2")
        .unwrap();
    let previous_column = issue.column_id.clone();
    issue.column_id = "github-removed-from-board".into();
    let director_id = snapshot.directors[0].id.clone();
    let session = &mut snapshot.sessions[0];
    session.issue_id = Some("issue-2".into());
    session.director_id = director_id;
    session.fixture = false;
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Completed,
        thread_id: Some("recorded-thread".into()),
        worktree: Some("/repo/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted
        .model
        .worker_prompt
        .set("Continue implementation".into());
    mounted.model.worker_approval.set(true);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    mounted.rect("Issue removed from board");
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("no longer")
    );
    mounted.model.open_session(session_id);
    mounted.model.worker_approval.set(true);
    mounted.settle();
    mounted.rect("Session issue removed from board");
    assert!(
        mounted
            .model
            .worker_gate(true)
            .unwrap_err()
            .contains("no longer")
    );
    // Board membership changes do not stop an already active turn.
    mounted
        .model
        .snapshot
        .update(|s| s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Running);
    mounted.settle();
    mounted.focus("Stop worker");
    mounted.click("Stop worker");
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StopWorker { .. }
    ));
    mounted.model.busy.set(false);
    mounted.model.snapshot.update(|s| {
        s.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
        s.issues
            .iter_mut()
            .find(|i| i.id == "issue-2")
            .unwrap()
            .column_id = previous_column;
    });
    assert!(mounted.model.worker_gate(true).is_ok());
    mounted.model.issue.set(None);
    mounted.model.page.set(Page::Board);
    mounted.settle();
    mounted.rect("Open issue #2");
    mounted.model.snapshot.update(|s| {
        s.issues
            .iter_mut()
            .find(|i| i.id == "issue-2")
            .unwrap()
            .column_id = "github-removed-from-board".into()
    });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Open issue #2"))
    );
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Column github-removed-from-board"))
    );
    assert!(
        mounted
            .model
            .snapshot
            .get_untracked()
            .issues
            .iter()
            .any(|i| i.id == "issue-2")
    );
}

#[test]
fn unmodified_default_director_can_delegate_automatically() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults = DirectorProfile::default();
    assert_eq!(snapshot.projects[0].defaults, DirectorProfile::default());
    assert!(
        !snapshot.projects[0]
            .defaults
            .responsibilities
            .contains(&Task::Implement)
    );
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted
        .model
        .worker_prompt
        .set("Delegate this issue".into());
    assert!(mounted.model.worker_gate(false).is_ok());
    mounted.model.run_worker(false);
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StartSession {
            approve_implementation: false,
            ..
        }
    ));
}

#[test]
fn ambiguous_retry_preserves_entire_envelope_after_review_and_revision_changes() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Original prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 9;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    assert!(!mounted.model.can_rebase());
    mounted.model.review_latest();
    mounted.model.rebase_conflict();
    mounted.model.retry_pending();
    let retry = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&retry).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Original prompt"
    );
}

#[test]
fn known_conflict_requires_explicit_new_request_rebase_and_retains_draft() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Reviewed prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 1;
    let revision = snapshot.revision;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Revision conflict".into()))),
        outcome_serial: 1,
        outcome_conflict: true,
        ..Default::default()
    });
    mounted.model.review_latest();
    mounted.model.retry_pending();
    let exact = mounted.commands.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((exact.request_id, Err("Revision conflict".into()))),
        outcome_serial: 2,
        outcome_conflict: true,
        ..Default::default()
    });
    mounted.settle();
    mounted.focus("Review conflict for new request");
    mounted.click("Review conflict for new request");
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Reviewed prompt"
    );
    assert!(mounted.commands.try_recv().is_err());
    mounted.model.run_worker(false);
    let rebased = mounted.commands.try_recv().unwrap();
    assert_ne!(rebased.request_id, original.request_id);
    assert_eq!(rebased.expected_revision, revision);
    assert_eq!(
        serde_json::to_value(&rebased.command).unwrap(),
        serde_json::to_value(&original.command).unwrap()
    );
}

#[test]
fn ambiguous_start_is_reconciled_from_snapshot_without_duplicate_launch() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(live_snapshot());
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.model.worker_prompt.set("Original prompt".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(false);
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    mounted.model.worker_prompt.set("Next draft".into());
    mounted.model.run_worker(false);
    assert!(mounted.commands.try_recv().is_err());
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut session = snapshot.sessions[0].clone();
    session.id = format!("session-{}", original.request_id);
    session.issue_id = Some("issue-2".into());
    snapshot.sessions.push(session);
    snapshot.revision += 1;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(
        mounted.model.session.get_untracked(),
        format!("session-{}", original.request_id)
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::Sessions);
    assert_eq!(mounted.model.worker_prompt.get_untracked(), "Next draft");
    assert!(mounted.model.notice.get_untracked().is_empty());
    assert!(!mounted.model.can_retry());
    mounted.model.retry_pending();
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn snapshot_prompt_and_comment_ids_confirm_ambiguous_writes() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.comment_target.set("m1".into());
    mounted.model.comment_body.set("Original comment".into());
    mounted.model.save_comment();
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((original.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 1,
        outcome_ambiguous: true,
        ..Default::default()
    });
    mounted.model.comment_body.set("Edited comment".into());
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot
        .apply(original.command, &original.request_id, 1)
        .unwrap();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert_eq!(mounted.model.comment_body.get_untracked(), "Edited comment");
    assert!(!mounted.model.can_retry());
    let prompt = "Continue exact session".to_string();
    mounted.model.worker_prompt.set(prompt.clone());
    mounted.model.submit(
        Command::SendWorker {
            session_id: "session-plan".into(),
            prompt: prompt.clone(),
            approve_implementation: true,
        },
        mounted.model.snapshot.get_untracked().revision,
        crate::model::Saved::Send(prompt),
    );
    let send = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((send.request_id.clone(), Err("Response lost".into()))),
        outcome_serial: 2,
        outcome_ambiguous: true,
        ..Default::default()
    });
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.push(Message {
        id: format!("prompt-{}", send.request_id),
        session_id: "session-plan".into(),
        author: "User".into(),
        kind: "prompt".into(),
        body: "Continue exact session".into(),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    assert!(!mounted.model.can_retry());
    assert!(mounted.model.worker_prompt.get_untracked().is_empty());
}

#[test]
fn failed_launch_without_thread_offers_new_linked_worker_instead_of_continue() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.director_id = snapshot.directors[0].id.clone();
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Failed,
        thread_id: None,
        worktree: Some("/repo/failed-launch".into()),
        branch: Some("failed-launch".into()),
        base_commit: None,
        error: Some("Codex could not start. Check server CLI authentication.".into()),
        usage: None,
        changes: None,
    });
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted
        .model
        .worker_prompt
        .set("Retained recovery draft".into());
    mounted.settle();
    mounted.rect("Worker cannot continue");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Send worker prompt"))
    );
    mounted.focus("Start new worker from linked issue");
    mounted.click("Start new worker from linked issue");
    assert_eq!(mounted.model.page.get_untracked(), Page::Board);
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some("issue-2")
    );
    assert_eq!(
        mounted.model.worker_director.get_untracked(),
        mounted.model.snapshot.get_untracked().directors[0].id
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Retained recovery draft"
    );
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Approve")
    );
    mounted.rect("Start worker");
}

#[test]
fn inline_reply_and_next_message_share_edits_images_and_undo_without_mutating_history() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let original = mounted.model.snapshot.get_untracked().messages.clone();
    mounted.click("Message m1");
    mounted.key(Key::Character("First reply λ".into()), false);
    mounted.click("Message m2");
    mounted.key(Key::Character("Second reply".into()), false);
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert_eq!(
        parts
            .iter()
            .filter(|p| matches!(p.kind, PartKind::Reply { .. }))
            .count(),
        2
    );
    let PartKind::Reply { parts: nested, .. } = &parts[0].kind else {
        panic!()
    };
    let id = nested[0].id.clone();
    let label = format!("Draft text {id}");
    let targets = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .filter(|n| n.label.as_deref() == Some(&label))
        .map(|n| n.id)
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 2);
    for _ in 0..200 {
        mounted.key(Key::Tab, false);
        if mounted.ui.focused().is_some_and(|e| e.id() == targets[1]) {
            break;
        }
    }
    assert_eq!(mounted.ui.focused().unwrap().id(), targets[1]);
    mounted.key(Key::Character(" edited below".into()), false);
    assert!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan"))
            .contains("First reply λ edited below")
    );
    mounted.key(Key::Character("z".into()), true);
    assert!(
        !plain_text(&crate::buffer::parts(mounted.model, "session-plan")).contains("edited below")
    );
    let mut parts = crate::buffer::parts(mounted.model, "session-plan");
    let PartKind::Reply { parts: nested, .. } = &mut parts[0].kind else {
        panic!()
    };
    nested.push(Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Asset {
            asset: Asset {
                id: uuid::Uuid::new_v4().to_string(),
                name: "inline.txt".into(),
                media_type: "text/plain".into(),
                size: 1,
            },
        },
    });
    crate::buffer::edit(mounted.model, "session-plan", parts);
    mounted.settle();
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|n| n.label.as_deref() == Some("Preview inline.txt"))
            .count(),
        2
    );
    mounted.click("Remove inline file");
    assert!(assets(&crate::buffer::parts(mounted.model, "session-plan")).is_empty());
    assert_eq!(mounted.model.snapshot.get_untracked().messages, original);
}

fn buffer_worker(mounted: &Mounted) {
    let mut snapshot = live_snapshot();
    let s = &mut snapshot.sessions[0];
    s.fixture = false;
    s.role = SessionRole::Worker;
    s.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread".into()),
        worktree: Some("/worktree".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let id = s.id.clone();
    snapshot.messages.push(Message {
        id: "prompt-active-run".into(),
        session_id: id.clone(),
        author: "You".into(),
        kind: "prompt".into(),
        body: "Original".into(),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.model.buffer.update(|b| {
        b.initialized = true;
        b.connected = true;
    });
    mounted.model.worker_approval.set(true);
}

#[test]
fn retry_restores_recovered_content_when_the_current_draft_is_empty() {
    use crate::buffer;
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    buffer::edit(mounted.model, "session-plan", vec![]);
    mounted.model.buffer.update(|state| {
        state.documents.get_mut("session-plan").unwrap().recovery =
            vec![vec![Part::text("Recovered")]];
    });
    mounted.settle();
    mounted.click("Retry draft save");
    assert_eq!(
        plain_text(&buffer::parts(mounted.model, "session-plan")),
        "Recovered"
    );
    assert!(mounted.model.buffer.get_untracked().documents["session-plan"].recovery_reviewed);
    assert_eq!(
        mounted.model.notice.get_untracked(),
        "Local draft restored. Ready to send."
    );
}

#[test]
fn resolved_recovery_stays_resolved_after_reloading_the_local_journal() {
    use crate::buffer;
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let directory =
        std::env::temp_dir().join(format!("relay-recovery-choice-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let settings = directory.join("settings.toml");
    let config = crate::network::Config {
        endpoint: "http://127.0.0.1:7331/".parse().unwrap(),
        token: "recovery-choice-test".into(),
    };
    buffer::load_journal(mounted.model, &settings, &config);
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Current")]);
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut("session-plan").unwrap();
        doc.remote.parts = vec![Part::text("Shared")];
        doc.recovery = vec![vec![Part::text("Recovered")]];
    });
    buffer::resolve(mounted.model, false);
    mounted.model.buffer.set(Default::default());
    buffer::load_journal(mounted.model, &settings, &config);
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents["session-plan"];
    assert_eq!(plain_text(&doc.parts), "Shared");
    assert!(doc.recovery_reviewed);
    assert!(
        doc.recovery
            .iter()
            .any(|parts| plain_text(parts) == "Current")
    );
    assert!(
        doc.recovery
            .iter()
            .any(|parts| plain_text(parts) == "Recovered")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovery_buttons_apply_choices_hide_the_panel_and_keep_alternative_drafts() {
    use crate::buffer;
    for (button, expected, notice) in [
        ("Load shared draft", "Shared", "Shared draft loaded."),
        (
            "Restore local draft",
            "Recovered",
            "Local draft restored. Ready to send.",
        ),
        ("Retry draft save", "Current", "Draft ready to send."),
    ] {
        let mounted = mount(false, 1380.0);
        buffer_worker(&mounted);
        buffer::edit(mounted.model, "session-plan", vec![Part::text("Current")]);
        mounted.model.buffer.update(|state| {
            let doc = state.documents.get_mut("session-plan").unwrap();
            doc.remote.parts = vec![Part::text("Shared")];
            doc.recovery = vec![vec![Part::text("Recovered")]];
            doc.error = "Message already launching or missing".into();
            doc.editing_queue = Some("completed-message".into());
        });
        mounted.settle();
        mounted.click(button);
        let state = mounted.model.buffer.get_untracked();
        let doc = &state.documents["session-plan"];
        assert_eq!(plain_text(&doc.parts), expected);
        assert!(doc.error.is_empty());
        assert!(doc.recovery_reviewed);
        assert!(doc.editing_queue.is_none());
        assert!(!doc.recovery.is_empty());
        assert_eq!(mounted.model.notice.get_untracked(), notice);
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(button))
        );
    }
}

#[test]
fn recovered_queue_edit_sends_a_new_turn_without_first_repeating_the_rejection() {
    use crate::buffer;
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Recovered")]);
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut("session-plan").unwrap();
        doc.remote.parts = doc.parts.clone();
        doc.editing_queue = Some("completed-message".into());
    });
    buffer::send(mounted.model);
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(request.command, Command::SubmitTurn { parts, .. } if plain_text(&parts) == "Recovered")
    );
}

#[test]
fn rejected_queue_edit_retains_draft_and_releases_pending_request() {
    use crate::{buffer, model::Saved};
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    let draft = Draft {
        session_id: "session-plan".into(),
        revision: 1,
        parts: vec![Part::text("Correction")],
    };
    buffer::edit(mounted.model, &draft.session_id, draft.parts.clone());
    mounted.model.buffer.update(|state| {
        let doc = state.documents.get_mut(&draft.session_id).unwrap();
        doc.remote = draft.clone();
        doc.parts.clear();
        doc.editing_queue = Some("already-launched".into());
        doc.submitting = true;
    });
    mounted.model.submit(
        Command::EditQueuedTurn {
            submission_id: "already-launched".into(),
            draft_revision: draft.revision,
            parts: draft.parts.clone(),
            approve_implementation: true,
        },
        mounted.model.snapshot.get_untracked().revision,
        Saved::Buffer(draft.clone()),
    );
    let original = mounted.commands.try_recv().unwrap();
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((
            original.request_id.clone(),
            Err("Message already launching or missing".into()),
        )),
        outcome_serial: 1,
        ..Default::default()
    });
    assert!(!mounted.model.can_retry());
    assert!(!mounted.model.busy.get_untracked());
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&draft.session_id];
    assert_eq!(doc.parts, draft.parts);
    assert!(doc.editing_queue.is_none());
    assert!(!doc.submitting);
    buffer::retry_save(mounted.model);
    buffer::send(mounted.model);
    buffer::flush(mounted.model);
    let fresh = mounted.commands.try_recv().unwrap();
    assert_ne!(fresh.request_id, original.request_id);
    assert!(matches!(fresh.command, Command::SubmitTurn { parts, .. } if parts == draft.parts));
}

#[test]
fn rapid_second_send_waits_for_ack_then_promotes_exact_queue_and_keeps_newer_draft() {
    use crate::{
        buffer,
        buffer_network::{Outcome, Request, Update},
    };
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Correction")],
    );
    buffer::send(mounted.model);
    buffer::send(mounted.model);
    let Request::Save { session, request } = rx.try_recv().unwrap() else {
        panic!()
    };
    let saved = Draft {
        session_id: session.clone(),
        revision: 1,
        parts: request.parts.clone(),
    };
    buffer::receive(
        mounted.model,
        Update {
            serial: 1,
            initialized: true,
            connected: true,
            drafts: vec![saved.clone()],
            outcomes: std::collections::BTreeMap::from([(
                request.request_id.clone(),
                Outcome::Saved {
                    session,
                    request,
                    result: Ok(saved),
                },
            )]),
            ..Default::default()
        },
    );
    let submitted = mounted.commands.try_recv().unwrap();
    let Command::SubmitTurn { parts, .. } = &submitted.command else {
        panic!()
    };
    assert_eq!(plain_text(parts), "Correction");
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Newer next message")],
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 1;
    snapshot.submissions.push(Submission {
        id: submitted.request_id.clone(),
        session_id: "session-plan".into(),
        parts: parts.clone(),
        state: SubmissionState::Queued,
        approve_implementation: true,
        error: None,
        interrupts_run: None,
        last_edit_request: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    buffer::flush(mounted.model);
    let promoted = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(promoted.command,Command::PromoteTurn{submission_id,active_run_id:Some(run)} if submission_id==submitted.request_id && run=="active-run")
    );
    assert_eq!(
        plain_text(&buffer::parts(mounted.model, "session-plan")),
        "Newer next message"
    );
}

#[test]
fn shared_draft_conflicts_retain_local_content_and_streamed_receipts_confirm_ambiguous_saves() {
    use crate::{
        buffer,
        buffer_network::{Request, Update},
    };
    let mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    buffer::edit(mounted.model, "session-plan", vec![Part::text("Local")]);
    mounted.model.buffer.update(|s| {
        s.documents.get_mut("session-plan").unwrap().changed =
            std::time::Instant::now() - Duration::from_secs(1)
    });
    buffer::flush(mounted.model);
    let Request::Save { session, request } = rx.try_recv().unwrap() else {
        panic!()
    };
    buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Newer local")],
    );
    buffer::receive(
        mounted.model,
        Update {
            serial: 1,
            initialized: true,
            connected: true,
            drafts: vec![Draft {
                session_id: session.clone(),
                revision: 1,
                parts: request.parts,
            }],
            ..Default::default()
        },
    );
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert!(doc.saving.is_none());
    assert!(!doc.conflict);
    assert_eq!(plain_text(&doc.parts), "Newer local");
    buffer::receive(
        mounted.model,
        Update {
            serial: 2,
            initialized: true,
            connected: true,
            drafts: vec![Draft {
                session_id: session.clone(),
                revision: 2,
                parts: vec![Part::text("Other client")],
            }],
            ..Default::default()
        },
    );
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert!(doc.conflict);
    assert_eq!(plain_text(&doc.parts), "Newer local");
    buffer::resolve(mounted.model, false);
    let state = mounted.model.buffer.get_untracked();
    let doc = &state.documents[&session];
    assert_eq!(plain_text(&doc.parts), "Other client");
    assert_eq!(plain_text(&doc.recovery[0]), "Newer local");
}

#[test]
fn draft_ime_commits_unicode_and_local_recovery_preserves_pending_file_bytes_and_ids() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "仮".into(),
        cursor: Some((0, 3)),
    });
    mounted.settle();
    assert!(crate::buffer::parts(mounted.model, "session-plan").is_empty());
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定 λ".into()));
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "確定 λ"
    );
    let directory =
        std::env::temp_dir().join(format!("relay-draft-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let config = crate::network::Config {
        endpoint: "http://127.0.0.1:7331/".parse().unwrap(),
        token: "recovery-test-token".into(),
    };
    let settings = directory.join("settings.toml");
    crate::buffer::load_journal(mounted.model, &settings, &config);
    let part = crate::buffer::insert_asset(
        mounted.model,
        "notes.txt".into(),
        "text/plain".into(),
        b"Recovered context".to_vec(),
    )
    .unwrap();
    let PartKind::Asset { asset } = &part.kind else {
        panic!()
    };
    let asset = asset.clone();
    crate::buffer::edit(
        mounted.model,
        "session-plan",
        vec![Part::text("Before"), part, Part::text("After")],
    );
    mounted.model.buffer.set(Default::default());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(tx));
    crate::buffer::load_journal(mounted.model, &settings, &config);
    let crate::buffer_network::Request::Upload {
        asset: restored,
        bytes,
    } = rx.try_recv().unwrap()
    else {
        panic!()
    };
    assert_eq!(restored, asset);
    assert_eq!(bytes.as_slice(), b"Recovered context");
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Before[notes.txt]After"
    );
    assert!(mounted.commands.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn long_buffer_lines_wrap_within_the_viewport_and_leave_a_reachable_next_message() {
    let mounted = mount(false, 820.0);
    mounted
        .model
        .snapshot
        .update(|s| s.messages[0].body = "A long recorded passage with spaces. ".repeat(40));
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let rect = mounted.rect("Message m1");
    assert!(
        rect.origin.x + rect.size.width <= mounted.size.width,
        "{rect:?}"
    );
    assert!(rect.size.height > 200.0, "{rect:?}");
    let next = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&next);
}

#[test]
fn browser_keyboard_edits_preserve_the_draft_when_the_visible_viewport_shrinks() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 teh", 7, 7, false);
    mounted.settle();
    mounted.size.height = 460.0;
    mounted.settle();
    assert_eq!(mounted.ui.focused().unwrap().id(), field);
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the", 7, 7, false);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the"
    );
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你", 8, 8, true);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the"
    );
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你好", 9, 9, false);
    mounted.settle();
    crate::browser_text::update(&mounted.ui, field.raw(), "a🙂 the你好", 9, 9, false);
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "a🙂 the你好"
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan").len(), 1);
    assert_eq!(mounted.ui.focused().unwrap().id(), field);
    assert!(
        mounted.commands.try_recv().is_err(),
        "Typing must not submit an execution"
    );
}

#[test]
fn chat_reveals_the_caret_when_the_keyboard_resizes_without_a_new_edit() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    let text = (0..45)
        .map(|n| format!("Line {n}: a longer chat draft 🙂\n"))
        .collect::<String>();
    let end = text.encode_utf16().count();
    crate::browser_text::update(&mounted.ui, field.raw(), &text, end, end, false);
    mounted.settle();
    for height in [600.0, 460.0, 390.0, 844.0] {
        mounted.size.height = height;
        mounted.settle();
        let tree = mounted.ui.inspection_snapshot();
        let mut parent = tree.node(field).unwrap().parent;
        let viewport = loop {
            let node = tree.node(parent.unwrap()).unwrap();
            if node.role == Role::ScrollView {
                break node.rect;
            }
            parent = node.parent;
        };
        let caret = mounted.ui.ime_cursor_area().unwrap();
        assert!(
            caret.origin.y >= viewport.origin.y - 0.5,
            "caret above chat at height {height}: {caret:?} / {viewport:?}"
        );
        assert!(
            caret.origin.y + caret.size.height <= viewport.origin.y + viewport.size.height + 0.5,
            "caret below chat at height {height}: {caret:?} / {viewport:?}"
        );
        assert_eq!(mounted.ui.focused().unwrap().id(), field);
        assert_eq!(
            mounted.ui.text_input_contents().unwrap().selection,
            (text.len(), text.len())
        );
    }
    let before_scroll = mounted.ui.ime_cursor_area().unwrap();
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 180.0),
        },
        position: Vector2::new(190.0, 300.0),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let after_scroll = mounted.ui.ime_cursor_area().unwrap();
    assert!(
        after_scroll.origin.y > before_scroll.origin.y + 100.0,
        "manual transcript scrolling must not be pulled back to the caret"
    );
    mounted.settle();
    assert_eq!(mounted.ui.ime_cursor_area().unwrap(), after_scroll);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        text
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn browser_visual_viewport_reflows_chat_without_resizing_the_canvas() {
    let mut mounted = mount(false, 390.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|node| node.label.filter(|label| label.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let field = mounted.ui.focused().unwrap().id();
    let text = (0..40)
        .map(|n| format!("Keyboard and zoom, line {n}: 🙂\n"))
        .collect::<String>();
    let end = text.encode_utf16().count();
    crate::browser_text::update(&mounted.ui, field.raw(), &text, end, end, false);
    mounted.settle();
    for size in [
        Size::new(390.0, 460.0),
        Size::new(195.0, 230.0),
        Size::new(390.0, 844.0),
    ] {
        mounted.model.browser_viewport.set(Some(size));
        mounted.settle();
        let tree = mounted.ui.inspection_snapshot();
        let mut parent = tree.node(field).unwrap().parent;
        let viewport = loop {
            let node = tree.node(parent.unwrap()).unwrap();
            if node.role == Role::ScrollView {
                break node.rect;
            }
            parent = node.parent;
        };
        let caret = mounted.ui.ime_cursor_area().unwrap();
        assert!(viewport.origin.y + viewport.size.height <= size.height);
        assert!(viewport.size.width <= size.width);
        assert!(caret.origin.y >= viewport.origin.y - 0.5);
        assert!(
            caret.origin.y + caret.size.height <= viewport.origin.y + viewport.size.height + 0.5,
            "caret must fit the visible chat while the canvas stays 390×900: {caret:?} / {viewport:?}"
        );
        assert_eq!(mounted.ui.focused().unwrap().id(), field);
        assert_eq!(
            mounted.ui.text_input_contents().unwrap().selection,
            (text.len(), text.len())
        );
    }
    assert_eq!(mounted.size, Size::new(390.0, 900.0));
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        text
    );
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn first_character_keeps_draft_focus_and_plain_paste_keeps_the_same_text_surface() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&label);
    let focus = mounted.ui.focused().unwrap().id();
    for character in ["H", "e", "l", "l", "o"] {
        mounted.key(Key::Character(character.into()), false);
        assert_eq!(mounted.ui.focused().unwrap().id(), focus);
    }
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Hello"
    );
    mounted.ui.set_clipboard_text(" pasted λ");
    mounted.key(Key::Character("v".into()), true);
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, "session-plan")),
        "Hello pasted λ"
    );
    assert_eq!(crate::buffer::parts(mounted.model, "session-plan").len(), 1);
    assert_eq!(mounted.ui.focused().unwrap().id(), focus);
}

#[test]
fn composing_on_recorded_text_creates_one_reply_and_never_submits_an_empty_composition() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    mounted.focus("Message m1");
    let source = mounted.model.snapshot.get_untracked().messages[0]
        .body
        .clone();
    mounted.ui.dispatch_ime(ImeEvent::Preedit {
        text: "仮".into(),
        cursor: Some((0, 3)),
    });
    mounted.settle();
    assert!(!has_content(&crate::buffer::parts(
        mounted.model,
        "session-plan"
    )));
    mounted.ui.dispatch_ime(ImeEvent::Commit("確定".into()));
    mounted.settle();
    let parts = crate::buffer::parts(mounted.model, "session-plan");
    assert!(has_content(&parts));
    assert_eq!(
        parts
            .iter()
            .filter(|p| matches!(p.kind, PartKind::Reply { .. }))
            .count(),
        1
    );
    assert!(plain_text(&parts).ends_with("確定"));
    assert_eq!(
        mounted.model.snapshot.get_untracked().messages[0].body,
        source
    );
}

#[test]
fn harness_settings_refresh_and_saved_execution_override_use_server_state() {
    let mut mounted = mount(false, 1380.0);
    let (refresh, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.harness_refresh.set(Some(refresh));
    mounted.model.receive(NetworkState {
        snapshot: live_snapshot(),
        connected: true,
        harnesses: vec![HarnessStatus {
            harness: Harness::ClaudeCode,
            executable: "/server/claude".into(),
            version: Some("2.test".into()),
            state: "ready".into(),
            detail: "Installed and authenticated".into(),
            checked_at: 1,
        }],
        ..Default::default()
    });
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Refresh Claude Code status");
    assert!(requests.try_recv().is_ok());
    assert!(mounted.commands.try_recv().is_err());
    mounted.click("Claude Code executable and status details");
    mounted.settle();
    mounted.click("Save Claude Code executable and check status");
    let command = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(command.command, Command::ConfigureHarness {harness: Harness::ClaudeCode, executable} if executable == "/server/claude")
    );
}

#[test]
fn inline_permission_answers_exact_run_and_disappears_when_expired() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    let session = mounted.model.session.get_untracked();
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.retain(|m| m.session_id != session);
    snapshot.tool_permissions.push(ToolPermission {
        id: "permission-1".into(),
        session_id: session.clone(),
        run_id: "native-run-1".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    let description = mounted.rect("notes.txt");
    let request = mounted.rect("Approval request");
    assert!(
        description.size.height > 0.0,
        "approval details must be readable"
    );
    assert!(
        description.origin.y >= request.origin.y
            && description.origin.y + description.size.height
                <= request.origin.y + request.size.height,
        "{description:?} inside {request:?}"
    );
    mounted.click("Allow once tool request");
    let envelope = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(envelope.command,Command::RespondPermission{permission_id, run_id, allow:true} if permission_id == "permission-1" && run_id == "native-run-1")
    );
    snapshot.revision += 1;
    snapshot.tool_permissions[0].expired = true;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((envelope.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Allow once tool request"))
    );
}

fn local_project_snapshot() -> Snapshot {
    let mut snapshot = live_snapshot();
    snapshot.projects[0].github = None;
    snapshot.projects[0].root = Some("/server/projects/relay".into());
    snapshot.projects[0].defaults = DirectorProfile::default();
    for issue in &mut snapshot.issues {
        issue.reference = None;
    }
    snapshot.migrate_projects();
    snapshot
}

fn type_in(mounted: &Mounted, label: &str, text: &str) {
    mounted.focus(label);
    mounted.key(Key::Character("a".into()), true);
    mounted.ui.dispatch_ime(ImeEvent::Commit(text.into()));
    mounted.settle();
}

#[test]
fn new_project_preserves_full_setup_draft_and_appends_on_acknowledgement() {
    let mut mounted = mount(false, 1380.0);
    mounted.click("New Project");
    type_in(&mounted, "Project name", "Second project");
    type_in(&mounted, "Absolute project root on server", "/srv/second");
    mounted.click("Add connection");
    type_in(
        &mounted,
        "Connection address",
        "git@example.com:team/second.git",
    );
    mounted.click("Settings");
    mounted.click("New Project");
    assert_eq!(
        mounted.model.project_draft.get_untracked().name,
        "Second project"
    );
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connection_form
            .address,
        "git@example.com:team/second.git"
    );
    mounted.focus("Add connection");
    mounted.click("Add connection");
    assert_eq!(
        mounted
            .model
            .project_draft
            .get_untracked()
            .connections
            .len(),
        1
    );
    mounted.focus("Create project");
    mounted.click("Create project");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&request.command,Command::CreateProject{name,root,connections} if name=="Second project" && root=="/srv/second" && matches!(&connections[..],[ConnectionInput::Repository{remote}] if remote=="git@example.com:team/second.git"))
    );
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let mut project = snapshot.projects[0].clone();
    project.id = format!("project-{}", request.request_id);
    project.name = "Second project".into();
    project.fixture = false;
    snapshot.projects.push(project.clone());
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        outcome: Some((request.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    assert_eq!(mounted.model.project.get_untracked(), project.id);
    assert_eq!(mounted.model.snapshot.get_untracked().projects.len(), 2);
    assert!(mounted.model.project_draft.get_untracked().name.is_empty());
    assert!(mounted.model.notice.get_untracked().is_empty());
}

#[test]
fn board_memberships_selection_and_project_switching_preserve_running_sessions() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let mut second = snapshot.boards[0].clone();
    second.id = "second-board".into();
    second.name = "Second board".into();
    second.columns = local_columns();
    snapshot.boards.push(second);
    snapshot.memberships.push(BoardMembership {
        board_id: "second-board".into(),
        issue_id: "issue-2".into(),
        column_ids: vec!["done".into()],
        remote_item_id: None,
    });
    let mut other = snapshot.projects[0].clone();
    other.id = "other-project".into();
    other.name = "Other".into();
    snapshot.projects.push(other);
    let original_projects = snapshot.projects.clone();
    let original_sessions = snapshot.sessions.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    let task = mounted
        .model
        .snapshot
        .get_untracked()
        .issues
        .iter()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .clone();
    assert!(mounted.model.task_in_column(&task, &task.column_id));
    mounted.click("Select board Second board");
    assert!(mounted.model.task_in_column(&task, "done"));
    assert!(!mounted.model.task_in_column(&task, &task.column_id));
    mounted.model.select_project("other-project".into());
    mounted.model.select_project("demo".into());
    assert_eq!(mounted.model.selected_board().unwrap().id, "second-board");
    assert_eq!(
        mounted.model.snapshot.get_untracked().projects,
        original_projects
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions,
        original_sessions
    );
}

#[test]
fn local_tasks_render_without_remote_reference_and_move_on_selected_board() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    let task = mounted
        .model
        .snapshot
        .get_untracked()
        .issues
        .iter()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .clone();
    let label = format!("Open local task {}", task.title);
    mounted.click(&label);
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref().is_some_and(|s| s.contains("#0")))
    );
    mounted.focus("Edit task");
    mounted.click("Edit task");
    mounted.focus("Move task to In review");
    mounted.click("Move task to In review");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::MoveTask{board_id,issue_id,column_id} if board_id=="board-demo" && issue_id=="issue-2" && column_id=="review")
    );
}

#[test]
fn initial_workspace_choice_is_explicit_and_followup_keeps_recorded_resources() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    for (id, name) in [("repo", "Repository"), ("directory", "Directory")] {
        snapshot.connections.push(ProjectConnection {
            id: id.into(),
            project_id: "demo".into(),
            name: name.into(),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Directory {
                path: format!("/server/{id}"),
            },
        });
    }
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    // Director and resource choices are disclosed from the worker setup head.
    mounted.click("Choose director and resources");
    mounted.focus("Choose session resources");
    mounted.click("Choose session resources");
    mounted.focus("Workspace resource Directory");
    mounted.click("Workspace resource Directory");
    assert_eq!(
        mounted.model.workspace_selection.get_untracked(),
        Some(vec!["repo".into()])
    );
    mounted.model.worker_prompt.set("Run local task".into());
    mounted.model.run_worker(false);
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::StartSession{issue_id:Some(issue_id),connection_ids:Some(ids),role:SessionRole::Worker,..} if issue_id=="issue-2" && ids==vec!["repo"])
    );
}

#[test]
fn reconciliation_has_no_retry_and_tracks_live_operation_state() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.operations.push(ProjectOperation {
        id: "publish-1".into(),
        project_id: "demo".into(),
        kind: OperationKind::Sync {
            board_id: "board-demo".into(),
        },
        state: OperationState::NeedsReconciliation,
        error: Some("Unknown provider write outcome".into()),
        results: [("pending".into(), "membership/issue-2".into())]
            .into_iter()
            .collect(),
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Retry operation"))
    );
    mounted.rect("This recovery step does not support URL lookup. Ask the server administrator to inspect the provider result before continuing.");
    assert!(mounted.commands.try_recv().is_err());
    mounted
        .model
        .snapshot
        .update(|s| s.operations[0].state = OperationState::Running);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Use this result and continue"))
    );
}

#[test]
fn appearance_radios_arrow_keys_and_scale_stepper_keep_accessible_semantics() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.click("Settings");
    mounted.focus("Theme: Dark");
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Theme: Dark"))
            .unwrap()
            .role,
        Role::Radio
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::Light
    );
    assert_eq!(
        mounted
            .ui
            .inspection_snapshot()
            .node(mounted.ui.focused().unwrap().id())
            .unwrap()
            .label
            .as_deref(),
        Some("Theme: Light")
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
    mounted.focus("Interface scale percent");
    mounted.key(Key::ArrowUp, false);
    assert!((mounted.model.preferences.get_untracked().scale - 1.1).abs() < 0.001);
    mounted.focus("Reset interface scale");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.0);
}

#[test]
fn first_director_prompt_uses_planning_session_without_issue_or_implementation_approval() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted
        .model
        .snapshot
        .update(|s| s.sessions.retain(|s| s.role != SessionRole::Director));
    mounted
        .model
        .worker_prompt
        .set("Unrelated worker draft".into());
    mounted.model.open_director("director-main".into());
    mounted.settle();
    assert_eq!(mounted.model.page.get_untracked(), Page::DirectorStart);
    let draft_label = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
            .unwrap()
    };
    type_in(&mounted, &draft_label(&mounted), "Plan the project");
    mounted.model.open_director("director-review".into());
    mounted.settle();
    type_in(&mounted, &draft_label(&mounted), "Review plan");
    mounted.model.open_director("director-main".into());
    mounted.settle();
    assert_eq!(
        plain_text(&crate::buffer::parts(
            mounted.model,
            "director-draft-director-main"
        )),
        "Plan the project"
    );
    assert_eq!(
        mounted.model.worker_prompt.get_untracked(),
        "Unrelated worker draft"
    );
    mounted.click("Send message");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::StartDirector{director_id,prompt,parts,approve_implementation:false} if director_id=="director-main" && prompt=="Plan the project" && plain_text(&parts)==prompt)
    );
}

#[test]
fn sidebar_hover_surface_spans_sibling_controls_and_new_director_marker_aligns() {
    let mounted = mount(false, 1380.0);
    let sidebar = mounted.rect("Sidebar");
    let project = mounted.rect("Project row Relay · demo");
    assert!((project.origin.x - sidebar.origin.x).abs() < 0.1);
    assert!((project.size.width - sidebar.size.width).abs() < 0.1);
    let first = mounted.rect("Director row Project director");
    let second = mounted.rect("Director row Review director");
    assert!((first.origin.x + first.size.width - sidebar.size.width).abs() < 0.1);
    assert!((first.origin.y + first.size.height - second.origin.y).abs() < 0.1);
    for label in [
        "Profile for Project director",
        "Project defaults for Relay · demo",
    ] {
        let action = mounted.rect(label);
        assert!((action.origin.x + action.size.width - sidebar.size.width).abs() < 0.1);
        assert!((action.size.height - first.size.height).abs() < 0.1);
    }
    let snapshot = mounted.ui.inspection_snapshot();
    let row = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Director row Project director"))
        .unwrap();
    let fill = || {
        mounted
            .ui
            .inspection_details(row.id)
            .unwrap()
            .attributes
            .into_iter()
            .find(|a| a.name == "fill")
            .map(|a| a.value)
    };
    let baseline = fill();
    for label in [
        "Toggle director Project director",
        "Open director Project director",
        "Profile for Project director",
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect(label).center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        assert_ne!(fill(), baseline, "Hovering {label} must fill the whole row");
        for sibling in [
            "Toggle director Project director",
            "Open director Project director",
            "Profile for Project director",
        ] {
            let rect = mounted.rect(sibling);
            assert!(
                rect.origin.x >= row.rect.origin.x
                    && rect.origin.x + rect.size.width
                        <= row.rect.origin.x + row.rect.size.width + 0.01
            );
        }
    }
    let create = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Create director in Relay · demo"))
        .unwrap();
    for (target, surface) in [
        ("New Project", "New Project"),
        (
            "Create director in Relay · demo",
            "New director row Relay · demo",
        ),
        (
            "Project defaults for Relay · demo",
            "New director row Relay · demo",
        ),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect("Sidebar header").center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        let id = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(surface))
            .unwrap()
            .id;
        let fill = || {
            mounted
                .ui
                .inspection_details(id)
                .unwrap()
                .attributes
                .into_iter()
                .find(|a| a.name == "fill")
                .map(|a| a.value)
        };
        let baseline = fill();
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Move,
            position: mounted.rect(target).center(),
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::ZERO,
        });
        mounted.settle();
        assert_ne!(fill(), baseline, "Hovering {target} must fill {surface}");
    }
    let marker = snapshot.node(create.children[0]).unwrap();
    assert!(
        (marker.rect.center().x - mounted.rect("Toggle director Project director").center().x)
            .abs()
            < 0.1
    );
    assert!(
        mounted.rect("New Project").origin.y
            > mounted.rect("Create director in Relay · demo").origin.y
    );
}

#[test]
fn nonempty_columns_cannot_be_deleted_without_explicit_task_moves() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    mounted.click("Board actions");
    mounted.click("Manage columns");
    mounted.focus("Column title");
    mounted.click("Delete empty column");
    assert!(mounted.commands.is_empty());
    assert_eq!(mounted.model.selected_board().unwrap().columns.len(), 3);
}

#[test]
fn new_project_form_remains_keyboard_reachable_at_two_hundred_percent() {
    let mut mounted = mount(false, 820.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| {
        p.scale = 2.0;
        p.sidebar_width = 160.0;
    });
    mounted.size = Size::new(820.0, 600.0);
    mounted.model.page.set(Page::NewProject);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Connection address"))
    );
    mounted.focus("Add connection");
    mounted.click("Add connection");
    for label in [
        "Project name",
        "Absolute project root on server",
        "Connection address",
        "Add connection",
        "Create project",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 320.0 && rect.origin.x + rect.size.width <= 820.01,
            "{label}: {rect:?}"
        );
    }
}

#[test]
fn connection_states_update_and_retry_and_remove_use_stable_connection_ids() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot
        .connections
        .retain(|c| !matches!(c.kind, ConnectionKind::Board { .. }));
    snapshot.connections.push(ProjectConnection {
        id: "repo-1".into(),
        project_id: "demo".into(),
        name: "Repository".into(),
        enabled: true,
        state: ConnectionState::Failed,
        error: Some("Clone failed".into()),
        kind: ConnectionKind::Repository {
            remote: "git@example.com:team/repo.git".into(),
            checkout: None,
            owned: true,
        },
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    mounted.click("Retry connection");
    let retry = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(retry.command,Command::RetryConnection{connection_id} if connection_id=="repo-1")
    );
    mounted.model.receive(NetworkState {
        snapshot: mounted.model.snapshot.get_untracked(),
        connected: true,
        outcome: Some((retry.request_id, Ok(()))),
        outcome_serial: 1,
        ..Default::default()
    });
    mounted
        .model
        .snapshot
        .update(|s| s.connections[0].state = ConnectionState::Ready);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Retry connection"))
    );
    mounted.click("Remove connection");
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::RemoveConnection{connection_id} if connection_id=="repo-1")
    );
    mounted.model.snapshot.update(|s| s.connections.clear());
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Remove connection"))
    );
}

#[test]
fn publish_maps_columns_and_task_repositories_without_pretending_board_is_remote() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.connections.push(ProjectConnection {
        id: "repository-1".into(),
        project_id: "demo".into(),
        name: "Repository".into(),
        enabled: true,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Repository {
            remote: "git@example.com:team/repo.git".into(),
            checkout: Some("/srv/repo".into()),
            owned: true,
        },
    });
    let tasks = snapshot.issues.clone();
    let columns = snapshot.boards[0].columns.clone();
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination title", "Published work");
    let source = BoardSource::Github {
        owner: "team".into(),
        number: 0,
        url: String::new(),
    };
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source,
                name: "New board defaults".into(),
                columns: vec![BoardColumn {
                    id: "provider-default-done".into(),
                    title: "Done".into(),
                }],
            })),
        });
    mounted.settle();
    for column in columns {
        let label = format!("Map {} to Done", column.title);
        mounted.focus(&label);
        mounted.click(&label);
    }
    for task in tasks {
        let label = format!("Issue repository Repository for {}", task.title);
        mounted.focus(&label);
        mounted.click(&label);
    }
    mounted.focus("Confirm board publication");
    mounted.click("Confirm board publication");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(request.command,Command::PublishBoard{board_id,target,columns,tasks} if board_id=="board-demo" && matches!(&target.source,BoardSource::Github{owner,number:0,..} if owner=="team") && target.name=="Published work" && columns.len()==3 && tasks.len()==4 && tasks.iter().all(|t|t.repository_connection_id=="repository-1"))
    );
    assert_eq!(
        mounted.model.selected_board().unwrap().source,
        BoardSource::Local
    );
}

#[test]
fn multi_repository_review_and_followup_keep_recorded_workspace_selection() {
    let mut mounted = mount(false, 1380.0);
    buffer_worker(&mounted);
    mounted.model.snapshot.update(|s| {
        let session = &mut s.sessions[0];
        session.connection_ids = vec!["repo-one".into(), "repo-two".into()];
        session.worker.as_mut().unwrap().status = WorkerStatus::Completed;
        session.workspaces = vec![SessionWorkspace {
            connection_id: "repo-two".into(),
            path: "/srv/repo-two/worktree".into(),
            repository: true,
            branch: Some("worker/two".into()),
            base_commit: Some("base-two".into()),
            changes: Some(ChangeSet {
                files: vec!["two.rs".into()],
                diff: "second repository diff".into(),
                truncated: false,
            }),
        }];
    });
    mounted.settle();
    mounted.click("Session actions");
    mounted.click("Toggle change review");
    assert!(has_label(&mounted, "repo-two"));
    assert!(mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|text| text.contains("two.rs") && text.contains("second repository diff"))
    }));
    assert!(has_label(&mounted, "worker/two"));
    mounted
        .model
        .workspace_selection
        .set(Some(vec!["unrelated".into()]));
    mounted
        .model
        .worker_prompt
        .set("Continue recorded resources".into());
    mounted.model.worker_approval.set(true);
    mounted.model.run_worker(true);
    assert!(
        matches!(mounted.commands.try_recv().unwrap().command,Command::SendWorker{session_id,..} if session_id=="session-plan")
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions[0].connection_ids,
        vec!["repo-one", "repo-two"]
    );
}

#[test]
fn destination_discovery_uses_server_metadata_and_ignores_a_different_destination() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.discovery_requests.set(Some(sender));
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination number (0 creates new)", "7");
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    let source = requests.try_recv().unwrap();
    assert!(matches!(&source,BoardSource::Github{owner,number:7,..} if owner=="team"));
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source: source.clone(),
                name: "Remote destination".into(),
                columns: vec![BoardColumn {
                    id: "real-status-id".into(),
                    title: "Ready".into(),
                }],
            })),
        });
    mounted.settle();
    mounted.focus("Map Backlog to Ready");
    mounted.click("Map Backlog to Ready");
    type_in(&mounted, "Destination owner or path", "other-team");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Map Backlog to Ready"))
    );
    assert_eq!(
        mounted.model.selected_board().unwrap().source,
        BoardSource::Local
    );
}

#[test]
fn new_destination_defaults_come_from_server_and_publish_draft_survives_navigation_errors() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.discovery_requests.set(Some(sender));
    type_in(&mounted, "Destination owner or path", "team");
    type_in(&mounted, "Destination title", "New destination");
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    let source = requests.try_recv().unwrap();
    assert!(matches!(&source, BoardSource::Github { number: 0, .. }));
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Err("Destination access denied".into())),
        });
    mounted.settle();
    mounted.rect("Destination access denied");
    mounted.click("Settings");
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    let draft = mounted
        .model
        .publish_drafts
        .get_untracked()
        .get("board-demo")
        .unwrap()
        .clone();
    assert_eq!(draft.path, "team");
    assert_eq!(draft.title, "New destination");
    mounted.rect("Destination access denied");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Enter status IDs"))
    );
    mounted
        .model
        .discovery
        .set(crate::project_network::DiscoveryUpdate {
            source: Some(source.clone()),
            result: Some(Ok(BoardDiscovery {
                source,
                name: "Default statuses".into(),
                columns: vec![BoardColumn {
                    id: "opaque-provider-id".into(),
                    title: "Ready".into(),
                }],
            })),
        });
    mounted.settle();
    mounted.focus("Map Backlog to Ready");
    mounted.click("Map Backlog to Ready");
    mounted.click("Settings");
    mounted.model.page.set(Page::Publish);
    mounted.settle();
    assert_eq!(
        mounted
            .model
            .publish_drafts
            .get_untracked()
            .get("board-demo")
            .unwrap()
            .mappings
            .get("backlog")
            .unwrap(),
        "opaque-provider-id"
    );
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_deref()
            .is_some_and(|label| label.contains("opaque-provider-id"))
    }));
    drop(requests);
    mounted.focus("Read destination statuses");
    mounted.click("Read destination statuses");
    mounted.rect("Cannot read destination statuses. Reconnect and try again.");
}

#[test]
fn board_reconciliation_accepts_a_url_and_builds_the_typed_result_for_the_pending_step() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.operations.push(ProjectOperation {
        id: "publish-url".into(),
        project_id: "demo".into(),
        kind: OperationKind::Publish {
            board_id: "board-demo".into(),
            target: PublishTarget {
                source: BoardSource::Github {
                    owner: "team".into(),
                    number: 0,
                    url: String::new(),
                },
                name: "Work".into(),
            },
            columns: vec![],
            tasks: vec![],
        },
        state: OperationState::NeedsReconciliation,
        error: Some("Connection lost after board creation".into()),
        results: [("pending".into(), "board".into())].into_iter().collect(),
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.page.set(Page::Connections);
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Provider operation key"))
    );
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.recovery_requests.set(Some(sender));
    type_in(
        &mounted,
        "Created board or issue URL",
        "https://github.com/orgs/team/projects/9",
    );
    mounted.focus("Check result");
    mounted.click("Check result");
    let request = requests.try_recv().unwrap();
    let result = ReconciliationResult {
        key: "board".into(),
        result: serde_json::to_string(&BoardSource::Github {
            owner: "team".into(),
            number: 9,
            url: request.input.url.clone(),
        })
        .unwrap(),
        description: "Found board Work in team".into(),
    };
    type_in(
        &mounted,
        "Created board or issue URL",
        "https://github.com/orgs/team/projects/10",
    );
    mounted
        .model
        .recovery
        .set(crate::project_network::RecoveryUpdate {
            request: Some(request.clone()),
            result: Some(Ok(result.clone())),
        });
    mounted.settle();
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Use this result and continue"))
    );
    type_in(&mounted, "Created board or issue URL", &request.input.url);
    mounted.focus("Check result");
    mounted.click("Check result");
    let request = requests.try_recv().unwrap();
    mounted
        .model
        .recovery
        .set(crate::project_network::RecoveryUpdate {
            request: Some(request),
            result: Some(Ok(result)),
        });
    mounted.settle();
    mounted.rect("Found board Work in team");
    mounted.focus("Use this result and continue");
    mounted.click("Use this result and continue");
    let Command::ReconcileOperation { key, result, .. } =
        mounted.commands.try_recv().unwrap().command
    else {
        panic!("Expected reconciliation");
    };
    assert_eq!(key, "board");
    assert!(
        matches!(serde_json::from_str::<BoardSource>(&result).unwrap(),BoardSource::Github{owner,number:9,url} if owner=="team" && url=="https://github.com/orgs/team/projects/9")
    );
}

#[test]
fn disabled_board_connections_hide_selectors_and_block_new_work_without_losing_history() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    snapshot.boards[0].name = "Disabled historical board".into();
    let board = snapshot.boards[0].clone();
    snapshot.connections.retain(
        |c| !matches!(&c.kind, ConnectionKind::Board { board_id } if board_id == &board.id),
    );
    snapshot.connections.push(ProjectConnection {
        id: "disabled-board".into(),
        project_id: "demo".into(),
        name: "Disabled board".into(),
        enabled: false,
        state: ConnectionState::Ready,
        error: None,
        kind: ConnectionKind::Board {
            board_id: board.id.clone(),
        },
    });
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), board.id.clone());
    });
    mounted.model.snapshot.set(snapshot.clone());
    mounted.settle();
    assert!(mounted.model.selected_board().is_none());
    assert!(mounted.model.board_columns().is_empty());
    assert!(
        !mounted
            .model
            .task_in_column(&snapshot.issues[0], &snapshot.issues[0].column_id)
    );
    assert!(!snapshot.visible_task(&snapshot.issues[0].id));
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some(&board.name))
    );
    mounted.model.action(Command::CreateTask {
        board_id: board.id.clone(),
        title: "Blocked".into(),
        body: String::new(),
        repository_connection_id: None,
    });
    mounted.model.sync_project();
    assert!(mounted.commands.try_recv().is_err());
    assert_eq!(
        mounted.model.snapshot.get_untracked().memberships,
        snapshot.memberships
    );
    snapshot.connections.clear();
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    assert_eq!(mounted.model.selected_board().unwrap().id, board.id);
}

#[test]
fn imported_task_recovery_marker_is_hidden_and_preserved_when_editing() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let marker = "<!-- relay-operation:publish-1:task:issue-2 -->";
    snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == "issue-2")
        .unwrap()
        .body = format!("Ordinary task description\n\n{marker}");
    mounted.model.snapshot.set(snapshot);
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    mounted.rect("Ordinary task description");
    assert!(!mounted.ui.inspection_snapshot().nodes.iter().any(|n| {
        n.label
            .as_ref()
            .is_some_and(|l| l.contains("relay-operation:"))
    }));
    mounted.focus("Edit task");
    mounted.key(Key::Enter, false);
    type_in(&mounted, "Task body", "Updated task description");
    mounted.focus("Save task");
    mounted.click("Save task");
    let Command::UpdateTask { body, .. } = mounted.commands.try_recv().unwrap().command else {
        panic!("Expected edit")
    };
    assert_eq!(body, format!("Updated task description\n\n{marker}"));
    assert_eq!(
        crate::projects::task_body(&body),
        "Updated task description"
    );
    assert_eq!(
        crate::projects::task_body("User <!-- ordinary comment --> text"),
        "User <!-- ordinary comment --> text"
    );
    assert_eq!(
        crate::projects::task_body("<!-- relay-operation:unfinished"),
        "<!-- relay-operation:unfinished"
    );
    assert_eq!(crate::projects::preserve_task_markers(&body, &body), body);
}

#[test]
fn director_execution_does_not_consume_worker_capacity_or_block_director_continuation() {
    let mounted = mount(false, 1380.0);
    mounted.model.worker_prompt.set("Continue planning".into());
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults.max_workers = 1;
    snapshot.projects[0]
        .defaults
        .permissions
        .insert(Task::Implement, Permission::Allow);
    let director_id = snapshot.directors[0].id.clone();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.director_id = director_id.clone();
    session.issue_id = Some("issue-2".into());
    session.role = SessionRole::Director;
    session.worker = Some(WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status: WorkerStatus::Running,
        thread_id: Some("thread".into()),
        worktree: Some("/server/work".into()),
        branch: None,
        base_commit: None,
        error: None,
        usage: None,
        changes: None,
    });
    let director_session = session.id.clone();
    mounted.model.snapshot.set(snapshot.clone());
    mounted.model.worker_director.set(director_id);
    mounted.model.issue.set(Some("issue-2".into()));
    assert_eq!(mounted.model.worker_profile(false).unwrap().1, 0);
    assert!(mounted.model.worker_gate(false).is_ok());
    let mut worker = snapshot.sessions[0].clone();
    worker.id = "active-worker".into();
    worker.role = SessionRole::Worker;
    snapshot.sessions.push(worker);
    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Completed;
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session(director_session);
    assert_eq!(mounted.model.worker_profile(true).unwrap().1, 1);
    assert!(mounted.model.worker_gate(true).is_ok());
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("slots")
    );
}

#[test]
fn session_and_issue_navigation_choose_matching_active_boards_and_keep_current_matches() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let original = snapshot.boards[0].clone();
    let mut second = original.clone();
    second.id = "second-board".into();
    second.name = "Second board".into();
    snapshot.boards.push(second.clone());
    snapshot.memberships.retain(|m| m.issue_id != "issue-2");
    snapshot.memberships.push(BoardMembership {
        board_id: second.id.clone(),
        issue_id: "issue-2".into(),
        column_ids: vec![second.columns[0].id.clone()],
        remote_item_id: None,
    });
    snapshot.sessions[0].issue_id = Some("issue-2".into());
    let session_id = snapshot.sessions[0].id.clone();
    mounted.model.snapshot.set(snapshot.clone());
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_session(session_id);
    assert_eq!(mounted.model.selected_board().unwrap().id, second.id);
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_worker_issue();
    assert_eq!(mounted.model.selected_board().unwrap().id, second.id);
    snapshot.memberships.push(BoardMembership {
        board_id: original.id.clone(),
        issue_id: "issue-2".into(),
        column_ids: vec![original.columns[0].id.clone()],
        remote_item_id: None,
    });
    mounted.model.snapshot.set(snapshot);
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert("demo".into(), original.id.clone());
    });
    mounted.model.open_worker_issue();
    assert_eq!(mounted.model.selected_board().unwrap().id, original.id);
}

#[test]
fn harness_cards_keep_summary_and_actions_compact_and_responsive() {
    for width in [1380.0, 820.0] {
        let mounted = mount(false, width);
        mounted.model.receive(NetworkState {
            snapshot: live_snapshot(),
            connected: true,
            harnesses: vec![HarnessStatus {
                harness: Harness::ClaudeCode,
                executable: "/server/claude".into(),
                version: Some("2.test".into()),
                state: "ready".into(),
                detail: "Lengthy private diagnostic".into(),
                checked_at: 1,
            }],
            ..Default::default()
        });
        mounted.model.page.set(Page::Settings);
        if width < 1000.0 {
            mounted.model.preferences.update(|p| p.scale = 2.0);
            mounted
                ._scope
                .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        }
        mounted.settle();
        let refresh = mounted.rect("Refresh Claude Code status");
        let details = mounted.rect("Claude Code executable and status details");
        assert!((refresh.origin.y - details.origin.y).abs() < 1.0);
        assert!(details.origin.x + details.size.width <= mounted.size.width);
        assert!(
            !mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("Lengthy private diagnostic"))
        );
    }
}

#[test]
fn published_aliases_preserve_selected_board_scope_and_linked_sessions() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = local_project_snapshot();
    let canonical = snapshot.issues[0].id.clone();
    let mut historical = snapshot.issues[0].clone();
    historical.id = "imported-history".into();
    snapshot.issues.push(historical);
    snapshot
        .issue_aliases
        .insert("imported-history".into(), canonical.clone());
    snapshot.sessions[0].issue_id = Some("imported-history".into());
    let director = snapshot.directors[0].id.clone();
    snapshot.directors[0].overrides.scope = Some(DirectorScope::Issues {
        issue_ids: vec!["imported-history".into()],
    });
    let mut old_board = snapshot.boards[0].clone();
    let current_board = old_board.id.clone();
    old_board.id = "existing-destination".into();
    snapshot.boards.push(old_board);
    snapshot
        .board_aliases
        .insert("existing-destination".into(), current_board.clone());
    mounted.model.preferences.update(|p| {
        p.selected_boards.insert(
            snapshot.projects[0].id.clone(),
            "existing-destination".into(),
        );
    });
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some(canonical.clone()));
    mounted.model.worker_director.set(director);
    mounted.model.worker_prompt.set("Continue this task".into());
    mounted.settle();
    assert_eq!(mounted.model.selected_board().unwrap().id, current_board);
    assert!(mounted.model.worker_gate(false).is_ok());
    assert!(
        mounted
            .model
            .sessions_for_task(&canonical)
            .iter()
            .any(|s| s.id == snapshot.sessions[0].id)
    );
    mounted.model.open_session(snapshot.sessions[0].id.clone());
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some(canonical.as_str())
    );
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions[0]
            .issue_id
            .as_deref(),
        Some("imported-history")
    );
}

#[test]
fn every_palette_keeps_text_and_controls_readable() {
    use crate::theme::{Palette, colors, contrast};
    for palette in Palette::ALL {
        let c = colors(palette);
        let text = [
            ("ink", c.ink),
            ("muted", c.muted),
            ("run text", c.run_text),
            ("attention text", c.attention_text),
            ("danger", c.danger),
            ("success", c.success),
            ("warning", c.warning),
        ];
        for (name, foreground) in text {
            for background in c.neutrals() {
                let ratio = contrast(foreground, background);
                assert!(ratio >= 4.5, "{palette:?} {name}: {ratio:.2}");
            }
        }
        for (name, foreground, background) in [
            ("on inverse", c.on_inverse, c.inverse),
            ("on inverse hover", c.on_inverse, c.inverse_hover),
            ("on inverse pressed", c.on_inverse, c.inverse_pressed),
            ("on run", c.on_run, c.run),
            ("on attention", c.on_attention, c.attention),
            ("ink on accent soft", c.ink, c.accent_soft),
        ]
        .into_iter()
        .chain(c.tints.map(|tint| ("ink on tint", c.ink, tint)))
        {
            let ratio = contrast(foreground, background);
            assert!(ratio >= 4.5, "{palette:?} {name}: {ratio:.2}");
        }
        for background in c.neutrals() {
            for (name, mark) in [
                ("focus ring", c.accent),
                ("rule", c.line),
                ("run glyph", c.run_text),
                ("attention glyph", c.attention_text),
            ] {
                let ratio = contrast(mark, background);
                assert!(ratio >= 3.0, "{palette:?} {name}: {ratio:.2}");
            }
        }
        for (name, fill) in [
            ("rest", c.inverse),
            ("hover", c.inverse_hover),
            ("pressed", c.inverse_pressed),
        ] {
            let indicator = contrast(fill, c.surface);
            assert!(
                indicator >= 3.0,
                "{palette:?} selector {name}: {indicator:.2}"
            );
        }
        assert!(
            contrast(c.inverse, c.inverse_hover) >= 1.2,
            "{palette:?} hover must be visible"
        );
        assert!(
            contrast(c.inverse_hover, c.inverse_pressed) >= 1.2,
            "{palette:?} press must be visible"
        );
        if matches!(
            palette,
            Palette::Paper | Palette::Warm | Palette::Slate | Palette::Neutral
        ) {
            for value in [c.base, c.surface, c.ink, c.inverse, c.on_inverse] {
                let [r, g, b, _] = value.to_srgb8();
                assert!(
                    [r, g, b] != [0, 0, 0] && [r, g, b] != [255, 255, 255],
                    "{palette:?} uses pure black or white"
                );
            }
        }
    }
}

#[test]
fn contrast_check_rejects_the_concept_text_colors() {
    let hex = mosaic::prelude::Color::from_rgb_hex;
    let warm = hex(0xE8E5DF);
    for (concept, expected) in [(0xC46A4A, 3.03), (0x4F8A6A, 3.23), (0x8E9095, 2.54)] {
        let ratio = crate::theme::contrast(hex(concept), warm);
        assert!((ratio - expected).abs() < 0.02, "{concept:06X}: {ratio:.2}");
        assert!(ratio < 4.5);
    }
    assert!(crate::theme::contrast(hex(0x62666D), hex(0xDAD5CC)) < 4.5);
}

fn settings_directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("relay-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn high_contrast_settings_are_additive_and_omitted_while_off() {
    use crate::settings::{Preferences, ThemeMode};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(
        &path,
        "mode = \"light\"\nlight_warm = true\ndark_neutral = true\nscale = 1.25\nsidebar_width = 300.0\n\n[selected_boards]\ndemo = \"board-1\"\n",
    )
    .unwrap();
    let mut preferences = Preferences::load(&path).unwrap();
    assert_eq!(preferences.mode, ThemeMode::Light);
    assert!(preferences.light_warm && preferences.dark_neutral);
    assert!(!preferences.light_high_contrast && !preferences.dark_high_contrast);
    assert_eq!(preferences.scale, 1.25);
    assert_eq!(preferences.sidebar_width, 300.0);
    assert_eq!(preferences.selected_boards["demo"], "board-1");
    preferences.save(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(!written.contains("high_contrast"), "{written}");
    preferences.dark_high_contrast = true;
    preferences.save(&path).unwrap();
    let reloaded = Preferences::load(&path).unwrap();
    assert!(reloaded.dark_high_contrast && reloaded.dark_neutral);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn backups_never_replace_existing_files() {
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(&path, b"newer = true\n").unwrap();
    let taken = directory.join("settings.toml.unreadable-20000229T000000Z");
    std::fs::write(&taken, b"older backup").unwrap();
    let backup =
        crate::settings::write_backup(&path, b"newer = true\n", "20000229T000000Z").unwrap();
    assert_ne!(backup, taken);
    assert_eq!(std::fs::read(&taken).unwrap(), b"older backup");
    assert_eq!(std::fs::read(&backup).unwrap(), b"newer = true\n");
    assert_eq!(crate::settings::utc_stamp(0), "19700101T000000Z");
    assert_eq!(crate::settings::utc_stamp(951_782_400), "20000229T000000Z");
    assert_eq!(crate::settings::utc_stamp(951_868_799), "20000229T235959Z");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unreadable_settings_are_never_overwritten_until_explicitly_recovered() {
    use crate::settings::{Persistence, Store};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let original = b"mode = \"dark\"\nfuture_setting = 3\n".to_vec();
    std::fs::write(&path, &original).unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    assert!(matches!(persistence, Persistence::Suspended { .. }));

    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 1.5);
    mounted
        .model
        .preferences
        .update(|p| p.sidebar_width = 300.0);
    mounted.settle();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    mounted.key(Key::Character(",".into()), true);
    mounted.rect("Display settings not saved");

    // Retrying a still-invalid file changes nothing.
    mounted.click("Retry reading settings file");
    assert!(matches!(
        mounted.model.settings_store.get_untracked().persistence,
        Persistence::Suspended { .. }
    ));
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.5);
    assert_eq!(std::fs::read(&path).unwrap(), original);

    mounted.click("Back up file and save current settings");
    let store = mounted.model.settings_store.get_untracked();
    assert_eq!(store.persistence, Persistence::Enabled);
    let backup = store.backup.unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let saved = crate::settings::Preferences::load(&path).unwrap();
    assert_eq!(saved.scale, 1.5);
    assert_eq!(saved.sidebar_width, 300.0);

    // Persistence resumes once recovered.
    mounted.model.preferences.update(|p| p.scale = 1.25);
    mounted.settle();
    assert_eq!(
        crate::settings::Preferences::load(&path).unwrap().scale,
        1.25
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_settings_backup_keeps_everything_and_stays_suspended() {
    use crate::settings::{Persistence, Store};
    // A directory where the settings file should be cannot be read for a
    // backup on any platform or user, so recovery must fail without writing.
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep.txt"), b"untouched").unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    assert!(matches!(persistence, Persistence::Suspended { .. }));
    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    crate::settings::recover_by_backup(mounted.model);
    let store = mounted.model.settings_store.get_untracked();
    assert!(matches!(store.persistence, Persistence::Suspended { .. }));
    assert!(store.backup.is_none());
    assert!(path.is_dir());
    assert_eq!(std::fs::read(path.join("keep.txt")).unwrap(), b"untouched");
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn backup_that_cannot_be_created_keeps_the_original_file() {
    use crate::settings::{Persistence, Store};
    use std::os::unix::fs::PermissionsExt;
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let original = b"future_setting = 3\n".to_vec();
    std::fs::write(&path, &original).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Privileged users can still write; there is nothing to verify then.
    let denied = std::fs::write(directory.join("probe"), b"").is_err();
    if denied {
        let (preferences, persistence) = crate::settings::open(&path);
        let mounted = mount(false, 1380.0);
        mounted.model.preferences.set(preferences);
        mounted.model.settings_store.set(Store {
            path: Some(path.clone()),
            persistence,
            backup: None,
        });
        crate::settings::recover_by_backup(mounted.model);
        assert!(matches!(
            mounted.model.settings_store.get_untracked().persistence,
            Persistence::Suspended { .. }
        ));
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn retry_reading_adopts_a_repaired_settings_file() {
    use crate::settings::{Persistence, Store};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    std::fs::write(&path, "broken syntax").unwrap();
    let (preferences, persistence) = crate::settings::open(&path);
    let mounted = mount(false, 1380.0);
    mounted.model.preferences.set(preferences);
    mounted.model.settings_store.set(Store {
        path: Some(path.clone()),
        persistence,
        backup: None,
    });
    std::fs::write(&path, "scale = 1.5\n").unwrap();
    crate::settings::retry_reading(mounted.model);
    assert_eq!(
        mounted.model.settings_store.get_untracked().persistence,
        Persistence::Enabled
    );
    assert_eq!(mounted.model.preferences.get_untracked().scale, 1.5);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn palette_selectors_slide_between_families_and_high_contrast() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.key(Key::Character(",".into()), true);
    mounted.click("Light palette: Warm");
    mounted.click("Light palette: High contrast");
    let preferences = mounted.model.preferences.get_untracked();
    assert!(preferences.light_high_contrast && preferences.light_warm);
    mounted.click("Light palette: Warm");
    let preferences = mounted.model.preferences.get_untracked();
    assert!(!preferences.light_high_contrast && preferences.light_warm);

    mounted.focus("Dark palette: Slate");
    mounted.key(Key::End, false);
    assert!(mounted.model.preferences.get_untracked().dark_high_contrast);
    mounted.key(Key::ArrowRight, false);
    let preferences = mounted.model.preferences.get_untracked();
    assert!(!preferences.dark_high_contrast && !preferences.dark_neutral);
    assert_eq!(
        mosaic::core::theme::color(crate::theme::surface.base),
        crate::theme::colors(crate::theme::Palette::Slate).base
    );
}

#[test]
fn usage_split_separates_cached_input_and_survives_bad_reports() {
    use crate::labels::UsageSplit;
    let usage = |input, cached, output| TokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
    };
    let split = UsageSplit::new(&usage(48_213, 31_004, 2_910));
    assert_eq!(
        (split.uncached, split.cached, split.output),
        (17_209, 31_004, 2_910)
    );
    assert!(!split.clamped);
    let [uncached, cached, output] = split.fractions().unwrap();
    assert!((uncached + cached + output - 1.0).abs() < 1e-5);
    assert!(cached > uncached && uncached > output);

    assert_eq!(UsageSplit::new(&usage(0, 0, 0)).fractions(), None);

    let inconsistent = UsageSplit::new(&usage(100, 250, 10));
    assert!(inconsistent.clamped);
    assert_eq!((inconsistent.uncached, inconsistent.cached), (0, 100));

    let huge = UsageSplit::new(&usage(u64::MAX, u64::MAX / 2, u64::MAX));
    for fraction in huge.fractions().unwrap() {
        assert!((0.0..=1.0).contains(&fraction));
    }
    assert_eq!(crate::labels::grouped(1_234_567), "1,234,567");
    assert_eq!(crate::labels::grouped(999), "999");
}

#[test]
fn issue_status_prefers_waiting_and_active_work_then_the_latest_outcome() {
    use crate::labels::{RunState, issue_status};
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    let template = snapshot.sessions[1].clone();
    let session = |id: &str, status: WorkerStatus| {
        let mut session = template.clone();
        session.id = id.into();
        session.fixture = false;
        session.worker = Some(worker_run(status));
        session
    };
    assert_eq!(issue_status(&snapshot, &[]), RunState::Ready);
    let older_failure = session("a", WorkerStatus::Failed);
    let newer_success = session("b", WorkerStatus::Completed);
    assert_eq!(
        issue_status(&snapshot, &[older_failure.clone(), newer_success.clone()]),
        RunState::Completed
    );
    assert_eq!(
        issue_status(&snapshot, &[newer_success.clone(), older_failure.clone()]),
        RunState::Failed
    );
    let queued = session("c", WorkerStatus::Queued);
    let running = session("d", WorkerStatus::Running);
    assert_eq!(
        issue_status(
            &snapshot,
            &[running.clone(), queued.clone(), newer_success.clone()]
        ),
        RunState::Running
    );
    assert_eq!(
        issue_status(&snapshot, &[queued.clone(), newer_success.clone()]),
        RunState::Queued
    );
    snapshot.tool_permissions.push(ToolPermission {
        id: "p".into(),
        session_id: "c".into(),
        run_id: "r".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    assert_eq!(
        issue_status(&snapshot, &[running, queued, newer_success]),
        RunState::Waiting
    );
}

#[test]
fn sidebar_rows_describe_worker_state_and_director_capacity() {
    let mounted = mount(false, 1380.0);
    mounted.click("Toggle director Project director");
    let description = |label: &str| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .and_then(|n| n.description.clone())
            .unwrap_or_default()
    };
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let index = snapshot
        .sessions
        .iter()
        .position(|s| s.id == "session-worker")
        .unwrap();
    let title = snapshot.sessions[index].title.clone();
    snapshot.sessions[index].fixture = false;
    for (status, word) in [
        (WorkerStatus::Queued, "Queued"),
        (WorkerStatus::Running, "Running"),
        (WorkerStatus::Interrupted, "Interrupted"),
    ] {
        snapshot.sessions[index].worker = Some(worker_run(status));
        mounted.model.receive(NetworkState {
            snapshot: snapshot.clone(),
            connected: true,
            ..Default::default()
        });
        mounted.settle();
        let worker = description(&format!("Open worker {title}"));
        assert!(worker.ends_with(word), "{worker}");
    }
    snapshot.sessions[index].worker = Some(worker_run(WorkerStatus::Running));
    snapshot.tool_permissions.push(ToolPermission {
        id: "permission-sidebar".into(),
        session_id: snapshot.sessions[index].id.clone(),
        run_id: "run".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(description(&format!("Open worker {title}")).ends_with("Waiting for approval"));
    let director = description("Open director Project director");
    assert!(
        director.contains("of 4 workers active, 1 running"),
        "{director}"
    );
}

fn worker_run(status: WorkerStatus) -> WorkerRun {
    WorkerRun {
        model: None,
        context_tokens: None,
        context_window: None,
        last_usage: None,
        harness: Harness::Codex,
        execution: None,
        status,
        thread_id: Some("thread".into()),
        worktree: Some("/repo/worktrees/issue".into()),
        branch: Some("worker/issue".into()),
        base_commit: Some("abc123".into()),
        error: None,
        usage: None,
        changes: None,
    }
}

#[test]
fn session_header_usage_and_execution_mode_use_measured_state() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.issue_id = Some("issue-2".into());
    session.director_id = snapshot.directors[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.usage = Some(TokenUsage {
        input_tokens: 48_213,
        cached_input_tokens: 31_004,
        output_tokens: 2_910,
    });
    session.worker = Some(run);
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let status = || {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Session status"))
            .and_then(|n| n.description.clone())
            .unwrap()
    };
    assert_eq!(status(), "Running");
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    mounted.rect("Latest turn usage");
    mounted.rect("Usage meter");

    mounted.focus("Worker approval: Ask");
    mounted.key(Key::Enter, false);
    let command = mounted.commands.try_recv().unwrap().command;
    assert!(matches!(
        command,
        Command::SetWorkerExecution { session_id: ref id, execution: Some(ExecutionSettings { approval: ApprovalMode::Ask }) } if *id == session_id
    ));

    snapshot.sessions[0].worker.as_mut().unwrap().status = WorkerStatus::Queued;
    snapshot.sessions[0].worker.as_mut().unwrap().usage = None;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(status(), "Queued");
    assert!(
        !mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.label.as_deref() == Some("Latest turn usage"))
    );
}

#[test]
fn permission_rows_set_exact_values_for_every_action_by_click_and_keyboard() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let permission = |task: Task| {
        mounted
            .model
            .editor_profile
            .get_untracked()
            .permissions
            .get(&task)
            .copied()
    };
    mounted.focus("Merge permission: Allow");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Allow));
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Ask));
    mounted.key(Key::Home, false);
    assert_eq!(permission(Task::Merge), Some(Permission::Deny));
    mounted.focus("Deploy permission: Ask");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Deploy), Some(Permission::Ask));
    mounted.focus("Implement permission: Deny");
    mounted.key(Key::Enter, false);
    assert_eq!(permission(Task::Implement), Some(Permission::Deny));

    let responsible = mounted
        .model
        .editor_profile
        .get_untracked()
        .responsibilities
        .contains(&Task::Merge);
    mounted.focus("Merge responsibility");
    mounted.key(Key::Enter, false);
    assert_ne!(
        mounted
            .model
            .editor_profile
            .get_untracked()
            .responsibilities
            .contains(&Task::Merge),
        responsible
    );
}

// Regression for https://github.com/jens-hj/relay/issues/1: accessibility
// bindings must die with the profile controls they update.
#[test]
fn director_options_survive_reopening_after_profile_controls_are_removed() {
    let mounted = mount(false, 1380.0);
    for _ in 0..3 {
        mounted.click("Profile for Project director");
        mounted
            .model
            .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
        mounted
            .model
            .modify_profile("responsibilities", |p| p.responsibilities.clear());
        mounted
            .model
            .modify_profile("completion", |p| p.completion.clear());
        mounted.model.modify_profile("scope", |p| {
            p.scope = DirectorScope::Issues {
                issue_ids: vec!["issue-1".into()],
            }
        });
        mounted.settle();
        mounted.click("Open director Project director");
        // Reloading defaults changes every toggle after the old page is gone.
        mounted.click("Profile for Review director");
        assert_eq!(
            mounted.model.editor.get_untracked(),
            EditTarget::Director("director-review".into())
        );
        mounted.click("Open director Project director");
    }
}

fn has_label(mounted: &Mounted, label: &str) -> bool {
    mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .any(|n| n.label.as_deref() == Some(label))
}

#[test]
fn usage_readout_follows_new_reports_for_the_same_session() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.sessions[0].fixture = false;
    snapshot.sessions[0].issue_id = Some("issue-2".into());
    snapshot.sessions[0].director_id = snapshot.directors[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.usage = Some(TokenUsage {
        input_tokens: 1_000,
        cached_input_tokens: 400,
        output_tokens: 50,
    });
    snapshot.sessions[0].worker = Some(run);
    let session_id = snapshot.sessions[0].id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    mounted.click("Session actions");
    mounted.click("Session usage and provenance");
    assert!(has_label(&mounted, "1,000"));
    assert!(has_label(&mounted, "400"));

    snapshot.sessions[0].worker.as_mut().unwrap().usage = Some(TokenUsage {
        input_tokens: 48_213,
        cached_input_tokens: 31_004,
        output_tokens: 2_910,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "48,213"));
    assert!(has_label(&mounted, "31,004"));
    assert!(has_label(&mounted, "2,910"));
    assert!(!has_label(&mounted, "1,000"));
    mounted.rect("Usage meter");
}

#[test]
fn provenance_names_connections_and_omits_unrecorded_values() {
    let mut snapshot = live_snapshot();
    let mut session = snapshot.sessions[0].clone();
    session.workspaces = vec![SessionWorkspace {
        connection_id: "workspace-project-1".into(),
        path: "/srv/project/workspace".into(),
        repository: false,
        branch: None,
        base_commit: None,
        changes: None,
    }];
    let mut run = worker_run(WorkerStatus::Completed);
    run.branch = None;
    run.base_commit = None;
    session.worker = Some(run);
    snapshot.sessions[0] = session.clone();
    let groups = crate::conversation::provenance(&snapshot, &session, false);
    assert_eq!(groups[0].title, "Project workspace");
    assert_eq!(
        groups[0].rows,
        vec![
            ("Kind", "Directory".to_string()),
            ("Path", "/srv/project/workspace".to_string())
        ]
    );
    let worker = &groups[1];
    assert_eq!(worker.title, "Worker run");
    assert!(
        worker
            .rows
            .iter()
            .all(|(key, _)| *key != "Branch" && *key != "Base")
    );
    assert!(worker.changes.is_none());
    // Groups keep their identity when recorded values change, so the details
    // pane updates them in place.
    let mut moved = session.clone();
    moved.workspaces[0].path = "/srv/project/other".into();
    moved.worker.as_mut().unwrap().worktree = Some("/srv/worktrees/next".into());
    let keys = |groups: &[crate::conversation::ProvenanceGroup]| {
        groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>()
    };
    let changed = crate::conversation::provenance(&snapshot, &moved, false);
    assert_eq!(keys(&groups), ["workspace-project-1", "worker"]);
    assert_eq!(keys(&changed), keys(&groups));
    assert_ne!(changed, groups);
}

#[test]
fn card_preview_strips_markers_and_collapses_whitespace() {
    let marker = "<!-- relay-operation:publish-1:task:issue-2 -->";
    assert_eq!(
        crate::ui::body_preview(&format!("Short   body\n\nwith lines\n\n{marker}")),
        "Short body with lines"
    );
    assert_eq!(crate::ui::body_preview(marker), "");
}

#[test]
fn board_chrome_aligns_with_the_sidebar_and_frames_cards() {
    let mounted = mount(false, 1600.0);
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    let bottom = |r: Rect| r.origin.y + r.size.height;
    let header = mounted.rect("Page header");
    let brand = mounted.rect("Sidebar header");
    assert!(near(header.size.height, 56.0), "{header:?}");
    assert!(near(bottom(header), bottom(brand)), "{header:?} {brand:?}");
    // Column heads start on the header rule, with no bar in between.
    let columns: Vec<Rect> = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .iter()
        .filter(|n| n.label.as_deref().is_some_and(|l| l.starts_with("Column ")))
        .map(|n| n.rect)
        .collect();
    assert!(!columns.is_empty());
    for column in &columns {
        assert!(near(column.origin.y, bottom(header)), "{column:?}");
        assert!(near(column.size.height, 40.0), "{column:?}");
    }
    // The board summary and the sidebar connection footer share a rule.
    let summary = mounted.rect("Board summary");
    let connection = mounted.rect("Connection");
    assert!(
        near(summary.origin.y, connection.origin.y),
        "{summary:?} {connection:?}"
    );
    assert!(near(summary.size.height, 52.0));
    // Card previews are clipped to three laid-out lines.
    for node in mounted.ui.inspection_snapshot().nodes {
        if node.label.as_deref() == Some("Task preview") {
            assert!(node.rect.size.height <= 51.5, "{:?}", node.rect);
        }
    }
    // The inspector opens on its identifier header, which holds Close.
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    let issue = mounted.rect("Issue header");
    let close = mounted.rect("Close issue details");
    assert!(issue.origin.y < 1.0, "{issue:?}");
    assert!(issue.size.height >= 91.5, "{issue:?}");
    assert!(close.origin.y >= issue.origin.y && bottom(close) <= bottom(issue));
}

#[test]
fn wide_profiles_show_the_action_matrix_beside_the_controls() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    let matrix = mounted.rect("Action matrix");
    let harness = mounted.rect("Harness choices");
    assert!(matrix.origin.y + mounted.rect("Deploy permission").size.height < mounted.size.height);
    assert!(harness.origin.x > matrix.origin.x + matrix.size.width - 1.0);
    let codex = mounted.rect("Agent harness: Codex");
    let claude = mounted.rect("Agent harness: Claude Code");
    assert!(
        (codex.size.width + claude.size.width - harness.size.width).abs() <= 1.0,
        "harness={harness:?}, codex={codex:?}, claude={claude:?}"
    );
    assert!(
        (claude.origin.x + claude.size.width - harness.origin.x - harness.size.width).abs() <= 1.0
    );
    assert!(mounted.rect("Deploy permission").origin.y < mounted.size.height);

    let narrow = mount(false, 820.0);
    narrow.model.open_profile(EditTarget::Defaults);
    narrow.settle();
    let matrix = narrow.rect("Action matrix");
    let harness = narrow.rect("Harness choices");
    assert!(harness.origin.y > matrix.origin.y + matrix.size.height - 1.0);
    // Same column: the selector sits inside its module's 12px padding.
    assert!(harness.origin.x >= matrix.origin.x && harness.origin.x <= matrix.origin.x + 13.0);
}

#[test]
fn inverse_controls_show_hover_and_press_in_every_palette() {
    for (light, alternate, high_contrast) in [
        (true, false, false),
        (true, true, false),
        (true, false, true),
        (false, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mounted = mount(light, 1380.0);
        install_theme(&theme::configured_palette(
            light,
            alternate,
            high_contrast,
            1.0,
        ));
        mounted.model.open_director_profile("director-main".into());
        mounted.settle();
        let fill = |label: &str| {
            let id = mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(label))
                .unwrap()
                .id;
            mounted
                .ui
                .inspection_details(id)
                .unwrap()
                .attributes
                .into_iter()
                .find(|a| a.name == "fill")
                .map(|a| a.value)
        };
        let pointer = |kind: PointerEventKind, position: Vector2| {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
            mounted.settle();
        };
        // Real pointer events must change the fill and restore it on leaving.
        let hold = |target: &str, filled: &str| {
            let rest = fill(filled);
            let center = mounted.rect(target).center();
            pointer(PointerEventKind::Move, center);
            let hovered = fill(filled);
            assert_ne!(hovered, rest, "{filled} while hovered");
            pointer(PointerEventKind::Down(PointerButton::Primary), center);
            assert_ne!(fill(filled), rest, "{filled} while pressed");
            assert_ne!(fill(filled), hovered, "{filled} press differs from hover");
            let away = Vector2::new(700.0, 880.0);
            pointer(PointerEventKind::Move, away);
            pointer(PointerEventKind::Up(PointerButton::Primary), away);
            assert_eq!(fill(filled), rest, "{filled} after leaving");
            rest
        };
        let unselected = fill("Director row Review director");
        let selected = hold(
            "Open director Project director",
            "Director row Project director",
        );
        assert_ne!(selected, unselected);
        hold("Save profile", "Save profile");
        hold("Agent harness: Codex", "Agent harness: Codex");
        pointer(
            PointerEventKind::Move,
            mounted.rect("Open director Review director").center(),
        );
        let hovered = fill("Director row Review director");
        assert_ne!(hovered, unselected, "unselected rows still show hover");
        assert_ne!(hovered, selected);
        mounted.click("New Project");
        hold("Create project", "Create project");
        mounted.click("Add connection");
        hold("Connection type: Repository", "Connection type: Repository");
    }
}

#[test]
fn fixture_projects_read_as_fixtures_and_local_projects_keep_their_status() {
    let mounted = mount(false, 1380.0);
    let card_states = |mounted: &Mounted| {
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .filter(|n| {
                n.label.as_deref().is_some_and(|l| {
                    l.starts_with("Open issue #") || l.starts_with("Open local task")
                })
            })
            .map(|n| n.description.clone().unwrap_or_default())
            .collect::<Vec<_>>()
    };
    assert_eq!(crate::ui::source_label(mounted.model), "Fixture");
    let fixture_cards = card_states(&mounted);
    assert!(!fixture_cards.is_empty());
    assert!(
        fixture_cards.iter().all(|d| d.starts_with("Fixture ·")),
        "{fixture_cards:?}"
    );
    // A fixture project cannot run, so its summary shows only the task count.
    assert!(has_label(&mounted, "Tasks"));
    assert!(!has_label(&mounted, "Running"));

    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.sessions.clear();
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(crate::ui::source_label(mounted.model), "Local board");
    let local_cards = card_states(&mounted);
    assert!(!local_cards.is_empty());
    assert!(
        local_cards
            .iter()
            .all(|d| d.starts_with("Ready to scope ·")),
        "{local_cards:?}"
    );
    for cell in ["Tasks", "Running", "Waiting", "Project workers"] {
        assert!(has_label(&mounted, cell), "{cell}");
    }
}

#[test]
fn board_actions_menu_closes_on_escape_and_outside_clicks_and_reopens_at_once() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    // Closing waits out the tooltip's hide delay.
    let closed = |mounted: &Mounted| {
        mounted.ui.tick(Duration::from_millis(250));
        mounted.settle();
        !has_label(mounted, "Manage columns")
    };
    mounted.settle();
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    // Escape from inside the menu closes it and returns focus to the trigger.
    mounted.focus("Manage columns");
    mounted.key(Key::Escape, false);
    assert!(!has_label(&mounted, "Manage columns"));
    let focused = mounted.ui.inspection_snapshot();
    let trigger = focused
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Board actions"))
        .unwrap();
    assert_eq!(mounted.ui.focused().map(|e| e.id()), Some(trigger.id));
    // One click reopens it.
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    // A click elsewhere closes it; a click on the trigger toggles it.
    mounted.click("Page header");
    assert!(closed(&mounted), "outside click");
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    mounted.click("Board actions");
    assert!(closed(&mounted), "trigger toggle");
    // Pressing another control closes the menu and still activates it.
    mounted.click("Board actions");
    assert!(has_label(&mounted, "Manage columns"));
    mounted.click("New task");
    assert!(closed(&mounted), "another control");
    assert!(has_label(&mounted, "New task title"));
    // Menu items act before the menu closes.
    mounted.click("Board actions");
    mounted.click("Operation history");
    assert!(closed(&mounted));
    assert!(has_label(&mounted, "Completed operations"));
}

#[test]
fn profile_header_counts_real_overrides_and_keeps_its_geometry() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_director_profile("director-main".into());
    mounted.settle();
    let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
    assert!(near(mounted.rect("Profile header").origin.y, 0.0));
    assert!(near(mounted.rect("Profile header").size.height, 74.0));
    assert!(near(mounted.rect("Inheritance").size.height, 54.0));
    assert!(near(mounted.rect("Save bar").size.height, 50.0));
    assert!(has_label(&mounted, "Overrides"));
    let before = crate::ui::overridden_fields(&mounted.model.editor_overrides.get_untracked());
    mounted
        .model
        .modify_profile("harness", |p| p.harness = Harness::ClaudeCode);
    mounted.settle();
    let after = crate::ui::overridden_fields(&mounted.model.editor_overrides.get_untracked());
    assert_eq!(
        after.len(),
        before.len() + usize::from(!before.contains(&"Harness"))
    );
    assert!(after.contains(&"Harness"));
    // Project defaults have no overrides to count.
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    assert!(!has_label(&mounted, "Overrides"));
    assert!(has_label(&mounted, "Directors"));
}

#[test]
fn profile_field_headers_contain_their_text_and_controls() {
    for (width, scale) in [(1380.0, 1.0), (1180.0, 1.0), (820.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        mounted.model.open_director_profile("director-main".into());
        for overrides in [false, true] {
            if overrides {
                mounted.model.modify_profile("harness", |_| {});
                mounted.model.modify_profile("execution", |_| {});
                mounted.model.modify_profile("scope", |_| {});
                mounted.model.modify_profile("max_workers", |_| {});
            }
            mounted.settle();
            let snapshot = mounted.ui.inspection_snapshot();
            for title in [
                "Agent harness",
                "Execution approval",
                "Scope",
                "Concurrent workers",
            ] {
                let label = format!("{title} field header");
                let head = snapshot
                    .nodes
                    .iter()
                    .find(|n| n.label.as_deref() == Some(&label))
                    .unwrap();
                for node in &snapshot.nodes {
                    let mut parent = node.parent;
                    while let Some(id) = parent {
                        if id == head.id {
                            let rect = node.rect;
                            if rect.size.width > 0.0 && rect.size.height > 0.0 {
                                assert!(
                                    rect.origin.x >= head.rect.origin.x - 1.0
                                        && rect.origin.x + rect.size.width
                                            <= head.rect.origin.x + head.rect.size.width + 1.0
                                        && rect.origin.y >= head.rect.origin.y - 1.0
                                        && rect.origin.y + rect.size.height
                                            <= head.rect.origin.y + head.rect.size.height + 1.0,
                                    "{label} at {width}px, scale {scale}, overrides {overrides}: {:?} {rect:?} outside {:?}",
                                    node.label,
                                    head.rect
                                );
                            }
                            break;
                        }
                        parent = snapshot.nodes.iter().find(|n| n.id == id).unwrap().parent;
                    }
                }
            }
        }
    }
}

#[test]
fn session_header_and_run_strip_show_only_recorded_values() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.director_id = snapshot.directors[0].id.clone();
    let mut run = worker_run(WorkerStatus::Running);
    run.thread_id = None;
    run.worktree = Some("/srv/relay/worktrees/issue-2-a41c".into());
    session.worker = Some(run);
    let session_id = session.id.clone();
    let cells = crate::conversation::run_cells(&snapshot, &snapshot.sessions[0]);
    let keys: Vec<_> = cells.iter().map(|c| c.0).collect();
    assert_eq!(
        keys,
        [
            "Harness",
            "Model",
            "Context",
            "Last prompt",
            "Cache",
            "Re-cache",
            "Worktree",
            "Next-turn approval"
        ],
        "no thread was recorded"
    );
    assert_eq!(cells[1].1, "—");
    assert_eq!(cells[2].1, "— / —");
    assert_eq!(cells[6].1, "…/worktrees/issue-2-a41c");
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    let header = mounted.rect("Session header");
    assert!((header.size.height - 74.0).abs() <= 1.0, "{header:?}");
    let strip = mounted.rect("Session run");
    assert!((strip.origin.y - (header.origin.y + header.size.height)).abs() <= 1.0);
    assert!(has_label(&mounted, "Worktree"));
    assert!(!has_label(&mounted, "Thread"));
    // The draft sits beside the 92px author column.
    let draft = mounted.rect("Next message");
    let send = mounted.rect("Send message");
    assert!(send.origin.x < draft.origin.x + 92.0);
    assert!(has_label(&mounted, "Stop worker"));
}

#[test]
fn session_run_model_and_context_follow_reported_updates() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.worker = Some(worker_run(WorkerStatus::Running));
    let id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.settle();
    assert!(has_label(&mounted, "Model"));
    assert!(has_label(&mounted, "Context"));
    let worker = snapshot.sessions[0].worker.as_mut().unwrap();
    worker.model = Some("reported-model".into());
    worker.context_tokens = Some(12345);
    worker.context_window = Some(200000);
    snapshot.revision += 1;
    let cells = crate::conversation::run_cells(&snapshot, &snapshot.sessions[0]);
    assert!(cells.contains(&("Model", "reported-model".into())));
    assert!(cells.contains(&("Context", "12,345 / 200,000".into())));
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(has_label(&mounted, "reported-model"));
    assert!(has_label(&mounted, "12,345 / 200,000"));
}

#[test]
fn live_readout_values_keep_their_container_typography_after_updates() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = live_snapshot();
    snapshot.projects[0].defaults.max_workers = 4;
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.issue.set(Some("issue-2".into()));
    mounted.settle();
    // The text leaf showing `value`, with its resolved type attributes.
    let typography = |value: &str| {
        let snapshot = mounted.ui.inspection_snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|n| {
                n.element_name.as_deref() == Some("text") && n.label.as_deref() == Some(value)
            })
            .unwrap_or_else(|| panic!("Missing text: {value}"));
        let details = mounted.ui.inspection_details(node.id).unwrap();
        ["font-size", "font-family", "text-color", "font-weight"].map(|name| {
            details
                .attributes
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.value.clone())
                .unwrap_or_default()
        })
    };
    let before = typography("0 / 4 active");
    assert_eq!(before[0], "13.0");
    assert_eq!(before[1], "Monospace");
    snapshot.revision += 1;
    snapshot.projects[0].defaults.max_workers = 5;
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(typography("0 / 5 active"), before);
}

#[test]
fn profile_fields_remain_reachable_at_double_scale_without_rebuilding_the_draft() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    mounted.model.open_director_profile("director-main".into());
    mounted.settle();
    type_in(&mounted, "Director name", "Retained director draft");
    mounted.focus("Deploy permission: Allow");
    let control = mounted.rect("Deploy permission: Allow");
    assert!(
        control.origin.x >= 0.0
            && control.origin.x + control.size.width <= mounted.size.width + 1.0,
        "{control:?}"
    );
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().permissions[&Task::Deploy],
        Permission::Allow
    );
    mounted.focus("Increase worker limit");
    mounted.key(Key::Enter, false);
    assert!(
        mounted
            .model
            .editor_overrides
            .get_untracked()
            .max_workers
            .is_some()
    );
    for label in [
        "Increase worker limit",
        "Inherit max_workers",
        "Scope: Whole project",
    ] {
        mounted.focus(label);
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= mounted.size.width + 1.0,
            "{label}: {rect:?}"
        );
    }
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    type_in(&mounted, "Profile TOML", "retained TOML draft");
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.toml.get_untracked(), "retained TOML draft");
    assert_eq!(
        mounted.model.editor_name.get_untracked(),
        "Retained director draft"
    );
    mounted.model.open_profile(EditTarget::Defaults);
    mounted.settle();
    mounted.focus("Toggle profile TOML");
    mounted.key(Key::Enter, false);
    assert_eq!(
        mounted.model.toml.get_untracked(),
        mounted.model.editor_profile.get_untracked().to_toml()
    );
    assert_ne!(mounted.model.toml.get_untracked(), "retained TOML draft");
}

#[test]
fn profile_scope_reacts_to_publication_and_harness_choice_reports_actual_availability() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    let issue_id = snapshot.issues[0].id.clone();
    snapshot.issues[0].reference = None;
    snapshot.issues[0].title = "Local task title".into();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.model.open_director_profile("director-main".into());
    mounted.settle();
    assert!(has_label(&mounted, "Scope Local task title"));
    type_in(&mounted, "Director name", "Unsubmitted edit");
    let task = snapshot
        .issues
        .iter_mut()
        .find(|i| i.id == issue_id)
        .unwrap();
    task.title = "Published task title".into();
    task.reference = Some(IssueRef {
        provider: Provider::Github,
        repository: "team/repo".into(),
        number: 456,
        url: "https://github.com/team/repo/issues/456".into(),
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.harnesses.set(vec![HarnessStatus {
        harness: Harness::ClaudeCode,
        executable: "claude".into(),
        version: None,
        state: "missing".into(),
        detail: "Not installed on this server".into(),
        checked_at: 1,
    }]);
    mounted.settle();
    assert!(!has_label(&mounted, "Scope Local task title"));
    assert!(has_label(&mounted, "Scope #456"));
    let chooser = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| n.label.as_deref() == Some("Agent harness: Claude Code"))
        .unwrap();
    assert_eq!(chooser.description.as_deref(), Some("Not installed"));
    mounted.click("Agent harness: Claude Code");
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
    assert_eq!(
        mounted.model.editor_overrides.get_untracked().harness,
        Some(Harness::ClaudeCode)
    );
    assert_eq!(
        mounted.model.editor_name.get_untracked(),
        "Unsubmitted edit"
    );
    mounted.focus("Agent harness: Claude Code");
    mounted.key(Key::Home, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::Codex
    );
    assert_eq!(
        mounted.ui.focused().unwrap().id(),
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| n.label.as_deref() == Some("Agent harness: Codex"))
            .unwrap()
            .id
    );
    mounted.key(Key::ArrowRight, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
    mounted.key(Key::ArrowLeft, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::Codex
    );
    mounted.key(Key::End, false);
    assert_eq!(
        mounted.model.editor_profile.get_untracked().harness,
        Harness::ClaudeCode
    );
}

#[test]
fn new_task_draft_survives_closing_and_reopening_the_form() {
    let mut mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    mounted.click("New task");
    type_in(&mounted, "New task title", "Kept title");
    type_in(&mounted, "New task body", "Kept body");
    mounted.click("New task");
    assert!(!has_label(&mounted, "New task title"));
    mounted.click("New task");
    mounted.click("Create task");
    let request = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(&request.command, Command::CreateTask { title, body, .. } if title == "Kept title" && body == "Kept body"),
        "{:?}",
        request.command
    );
}

#[test]
fn wide_sessions_turn_narrow_at_double_scale_without_rebuilding_the_draft() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.director_id = snapshot.directors[0].id.clone();
    session.worker = Some(worker_run(WorkerStatus::Running));
    let session_id = session.id.clone();
    snapshot.tool_permissions.push(ToolPermission {
        id: "p".into(),
        session_id: session_id.clone(),
        run_id: "r".into(),
        tool: "Write".into(),
        description: "notes.txt".into(),
        decision: None,
        expired: false,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    mounted.focus(&draft);
    mounted
        .ui
        .dispatch_ime(ImeEvent::Commit("kept draft".into()));
    mounted.settle();
    let focused = mounted.ui.focused().map(|e| e.id());
    // Doubling the scale switches the conversation to its narrow layout.
    mounted.model.preferences.update(|p| p.scale = 2.0);
    mounted.settle();
    assert_eq!(
        mounted.ui.focused().map(|e| e.id()),
        focused,
        "draft surface kept"
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &session_id)),
        "kept draft"
    );
    for label in [
        "Allow once tool request",
        "Deny tool request",
        "Send message",
        "Session actions",
    ] {
        let rect = mounted.rect(label);
        assert!(
            rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= 1381.0,
            "{label}: {rect:?}"
        );
    }
    // Stop moves into the session actions when the header is narrow.
    assert!(!has_label(&mounted, "Stop worker"));
    mounted.click("Session actions");
    assert!(has_label(&mounted, "Stop worker"));
}

/// Pinned Mosaic `text_dyn_styled` binds a live text leaf's content and style
/// in separate effects. Typography from leaf attributes, leaf classes and
/// styled parent containers stays the same at mount, after content updates
/// and after a theme switch.
#[test]
fn live_text_typography_survives_content_and_theme_updates() {
    use crate::{styles::relay, theme::ink};
    mosaic::core::builtins::install();
    install_theme(&theme::palette(false));
    let scope = Scope::new(|| {});
    let ui = scope.run(Ui::new);
    let mut fonts = FontContext::embedded_only();
    crate::fonts::configure(&mut fonts);
    ui.set_fonts(fonts);
    let value = State::new("one".to_string());
    scope.run(|| {
        let _ambient = ui.enter();
        ui.mount(&view! {
            col font-size:20px {
                row font-size:13px {
                    text {format!("container {}", value.get())}
                }
                text font-size:13px {format!("leaf {}", value.get())}
                text #relay.caption {format!("class {}", value.get())}
                row #relay.caption {
                    text {format!("parent class {}", value.get())}
                }
                text font-color:ink.muted {format!("leaf color {}", value.get())}
                row font-color:ink.muted {
                    text {format!("container color {}", value.get())}
                }
            }
        });
    });
    let attribute = |label: &str, name: &str| {
        flush();
        ui.frame(Size::new(400.0, 300.0), 1.0);
        let snapshot = ui.inspection_snapshot();
        let node = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("Missing text: {label}"));
        ui.inspection_details(node.id)
            .unwrap()
            .attributes
            .into_iter()
            .find(|a| a.name == name)
            .map(|a| a.value)
            .unwrap_or_default()
    };
    let size = |label: &str| attribute(label, "font-size");
    let colors = |n: &str| {
        [
            attribute(&format!("leaf color {n}"), "text-color"),
            attribute(&format!("container color {n}"), "text-color"),
        ]
    };
    let read = |n: &str| {
        [
            size(&format!("container {n}")),
            size(&format!("leaf {n}")),
            size(&format!("class {n}")),
            size(&format!("parent class {n}")),
        ]
    };
    let initial = read("one");
    let initial_colors = colors("one");
    value.set("two".into());
    let updated = read("two");
    let updated_colors = colors("two");
    install_theme(&theme::palette(true));
    let themed = read("two");
    let themed_colors = colors("two");
    assert_eq!(initial, ["13.0", "13.0", "12.0", "12.0"].map(String::from));
    assert_eq!(updated, initial, "content update");
    assert_eq!(themed, initial, "theme switch");
    assert_eq!(updated_colors, initial_colors, "content update");
    assert_eq!(themed_colors, initial_colors, "theme switch");
}

/// A focused control must also survive its local scroll clip, not just have a
/// rectangle inside the window. Hit testing uses Mosaic's real clip chain.
fn assert_control_hit(mounted: &Mounted, label: &str) {
    mounted.focus(label);
    let snapshot = mounted.ui.inspection_snapshot();
    let target = snapshot
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some(label))
        .unwrap();
    let rect = target.rect;
    assert!(
        rect.origin.x >= 0.0 && rect.origin.x + rect.size.width <= mounted.size.width + 1.0,
        "{label}: {rect:?}"
    );
    let mut hit = mounted.ui.hit_test(rect.center()).map(|e| e.id());
    while let Some(id) = hit {
        if id == target.id {
            return;
        }
        hit = snapshot
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.parent);
    }
    panic!("Focused control clipped or obscured: {label}: {rect:?}");
}

#[test]
fn bounded_session_disclosures_keep_controls_reachable_and_draft_mounted() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    let mut snapshot = live_snapshot();
    let session = &mut snapshot.sessions[0];
    session.fixture = false;
    session.director_id = snapshot.directors[0].id.clone();
    let mut worker = worker_run(WorkerStatus::Running);
    worker.error = Some("A long recorded diagnostic. ".repeat(80));
    session.worker = Some(worker);
    let session_id = session.id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id.clone());
    mounted.settle();
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Draft text"))
        })
        .unwrap();
    mounted.focus(draft.label.as_deref().unwrap());
    mounted.ui.dispatch_ime(ImeEvent::Commit(
        "Draft retained through disclosures".into(),
    ));
    mounted.settle();
    mounted.model.buffer.update(|b| {
        b.approval_needed = true;
        let doc = b.documents.get_mut(&session_id).unwrap();
        doc.error = "Draft save failed with a long diagnostic. ".repeat(80);
        doc.recovery.push(doc.parts.clone());
    });
    mounted.settle();
    mounted.click("Session actions");
    assert_control_hit(&mounted, "Session usage and provenance");
    mounted.key(Key::Enter, false);
    for label in [
        "Stop worker",
        "Inherit worker execution settings",
        "Load shared draft",
        "Restore local draft",
        "Retry draft save",
        "Approve this turn and send",
        "Send message",
    ] {
        assert_control_hit(&mounted, label);
    }
    assert!(
        mounted
            .ui
            .inspection_snapshot()
            .nodes
            .iter()
            .any(|n| n.id == draft.id),
        "editor remains mounted"
    );
    assert_eq!(
        plain_text(&crate::buffer::parts(mounted.model, &session_id)),
        "Draft retained through disclosures"
    );
}

#[test]
fn bounded_worker_setup_scrolls_long_choices_without_losing_prompt() {
    for (width, scale) in [(1600.0, 1.0), (760.0, 2.0)] {
        let mounted = mount(false, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        let mut snapshot = local_project_snapshot();
        for i in 0..24 {
            let mut director = snapshot.directors[0].clone();
            director.id = format!("director-extra-{i}");
            director.name = format!("Director {i}");
            snapshot.directors.push(director);
            snapshot.connections.push(ProjectConnection {
                id: format!("directory-{i}"),
                project_id: "demo".into(),
                name: format!("Directory {i}"),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Directory {
                    path: format!("/server/{i}"),
                },
            });
        }
        mounted.model.snapshot.set(snapshot);
        mounted.model.issue.set(Some("issue-2".into()));
        mounted.settle();
        mounted
            .model
            .worker_prompt
            .set("Retained worker prompt".into());
        assert_control_hit(&mounted, "Choose director and resources");
        mounted.key(Key::Enter, false);
        assert_control_hit(&mounted, "Choose session resources");
        mounted.key(Key::Enter, false);
        for label in [
            "Workspace resource Directory 23",
            "Worker prompt",
            "Start worker",
        ] {
            assert_control_hit(&mounted, label);
        }
        assert_eq!(
            mounted.model.worker_prompt.get_untracked(),
            "Retained worker prompt"
        );
    }
}

#[test]
fn bounded_operation_history_exposes_recorded_results_and_keeps_close_reachable() {
    let mounted = mount(false, 760.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
    mounted.model.preferences.update(|p| p.scale = 2.0);
    let mut snapshot = local_project_snapshot();
    for i in 0..50 {
        snapshot.operations.push(ProjectOperation {
            id: format!("operation-{i}"),
            project_id: "demo".into(),
            kind: OperationKind::CreateTask {
                board_id: "board-demo".into(),
                issue_id: "issue-2".into(),
            },
            state: OperationState::Completed,
            error: None,
            results: [(
                "issue/issue-2".into(),
                serde_json::to_string(&IssueRef {
                    provider: Provider::Github,
                    repository: "team/repo".into(),
                    number: 456,
                    url: "https://github.com/team/repo/issues/456".into(),
                })
                .unwrap(),
            )]
            .into_iter()
            .collect(),
        });
    }
    mounted.model.snapshot.set(snapshot);
    mounted.settle();
    mounted.click("Board actions");
    assert_control_hit(&mounted, "Operation history");
    mounted.key(Key::Enter, false);
    assert_control_hit(&mounted, "Operation details operation-49");
    mounted.key(Key::Enter, false);
    assert!(has_label(
        &mounted,
        "https://github.com/team/repo/issues/456"
    ));
    assert!(has_label(&mounted, "Operation: operation-49"));
    assert_control_hit(&mounted, "Close operation history");
    mounted.key(Key::Enter, false);
    assert!(!has_label(&mounted, "Operation details operation-49"));
    assert_control_hit(&mounted, "New task");
}

#[test]
fn page_header_eyebrow_follows_navigation_between_shared_header_pages() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = demo_snapshot(DirectorProfile::default());
    snapshot.projects[0].fixture = false;
    snapshot.migrate_projects();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    // The eyebrow is the first text in the page header.
    let eyebrow = |mounted: &Mounted| {
        let snapshot = mounted.ui.inspection_snapshot();
        let header = snapshot
            .nodes
            .iter()
            .find(|n| n.label.as_deref() == Some("Page header"))
            .unwrap()
            .rect;
        snapshot
            .nodes
            .iter()
            .filter(|n| {
                n.element_name.as_deref() == Some("text") && header.contains(n.rect.center())
            })
            .min_by(|a, b| a.rect.origin.y.total_cmp(&b.rect.origin.y))
            .and_then(|n| n.label.clone())
            .unwrap_or_default()
    };
    for (page, expected) in [
        (Page::Settings, "Relay"),
        (Page::Connections, "Project Connections"),
        (Page::Publish, "Publish board"),
        (Page::NewProject, "Relay"),
    ] {
        mounted.model.page.set(page);
        mounted.settle();
        assert_eq!(eyebrow(&mounted), expected, "{page:?}");
    }
}

#[test]
fn conversation_tracks_constrain_recorded_text_and_draft_to_the_reading_width() {
    let mounted = mount(false, 1600.0);
    let mut snapshot = live_snapshot();
    let session_id = snapshot.sessions[0].id.clone();
    snapshot.messages.push(Message {
        id: "reading-width".into(),
        session_id: session_id.clone(),
        author: "Codex".into(),
        kind: "agent_message".into(),
        body: "A recorded paragraph should wrap inside the reading column. ".repeat(10),
        parts: vec![],
        tool: None,
    });
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(session_id);
    mounted.settle();
    let text = mounted.rect("Message reading-width");
    assert!(text.size.width <= 760.0 + 1.0, "{text:?}");
    assert!(text.size.height > 40.0, "long text must wrap: {text:?}");
    let draft = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find(|n| {
            n.label
                .as_deref()
                .is_some_and(|l| l.starts_with("Draft text"))
        })
        .unwrap();
    assert!(draft.rect.size.width <= 760.0 + 1.0, "{:?}", draft.rect);
    assert!(
        (draft.rect.origin.x - text.origin.x).abs() < 1.0,
        "draft and recorded text share a reading column"
    );
}

#[test]
fn clicking_below_the_draft_focuses_its_end_across_the_buffer_width() {
    for width in [820.0, 1600.0] {
        let mounted = mount(false, width);
        buffer_worker(&mounted);
        mounted.settle();
        let draft = mounted
            .ui
            .inspection_snapshot()
            .nodes
            .into_iter()
            .find(|n| {
                n.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("Draft text"))
            })
            .unwrap();
        let label = draft.label.unwrap();
        // The empty draft is reachable without hitting its text-height field.
        for x in [draft.rect.origin.x - 5.0, width - 50.0] {
            mounted.focus("Session actions");
            let rect = mounted.rect(&label);
            let position = Vector2::new(x, rect.origin.y + rect.size.height + 60.0);
            for kind in [
                PointerEventKind::Down(PointerButton::Primary),
                PointerEventKind::Up(PointerButton::Primary),
            ] {
                mounted.ui.dispatch_pointer(PointerEvent {
                    kind,
                    position,
                    pointer_type: PointerType::Mouse,
                    modifiers: Modifiers::default(),
                    timestamp: Duration::ZERO,
                });
            }
            mounted.settle();
            assert_eq!(mounted.ui.focused().unwrap().id(), draft.id);
        }
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit("First line\nLast line".into()));
        mounted.settle();
        mounted.focus(&label);
        mounted.key(Key::Home, true);
        mounted.focus("Session actions");
        let rect = mounted.rect(&label);
        let position = Vector2::new(width - 50.0, rect.origin.y + rect.size.height + 60.0);
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
        }
        mounted.settle();
        mounted
            .ui
            .dispatch_ime(ImeEvent::Commit(" appended".into()));
        mounted.settle();
        assert_eq!(
            plain_text(&crate::buffer::parts(
                mounted.model,
                &mounted.model.session.get_untracked()
            )),
            "First line\nLast line appended"
        );
    }
}

#[test]
fn display_settings_write_sparse_overrides_and_reset_independently() {
    use crate::settings::{Preferences, Setting, ThemeMode};
    let directory = settings_directory();
    let path = directory.join("settings.toml");
    let mut p = Preferences::default();
    p.save(&path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().trim(),
        "version = 2"
    );
    p.mode = ThemeMode::Light;
    p.light_warm = true;
    p.dark_high_contrast = true;
    p.scale = 1.6;
    p.sidebar_width = 310.0;
    p.selected_boards.insert("project".into(), "board".into());
    p.save(&path).unwrap();
    assert_eq!(Preferences::load(&path).unwrap(), p);
    for setting in [
        Setting::Mode,
        Setting::LightPalette,
        Setting::DarkPalette,
        Setting::Scale,
    ] {
        assert!(p.overridden(setting));
        p.reset(setting);
        assert!(!p.overridden(setting));
    }
    p.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for key in ["mode", "light_palette", "dark_palette", "scale"] {
        assert!(!text.contains(&format!("{key} =")), "{text}");
    }
    let loaded = Preferences::load(&path).unwrap();
    assert_eq!(loaded.sidebar_width, 310.0);
    assert_eq!(loaded.selected_boards["project"], "board");
    std::fs::write(&path, "version = 99\nscale = 1.2\n").unwrap();
    assert!(Preferences::load(&path).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "version = 99\nscale = 1.2\n"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sidebar_edge_double_click_clears_retained_resize_and_preserves_other_settings() {
    let mounted = mount(false, 1380.0);
    mounted.model.preferences.update(|p| {
        p.sidebar_width = 310.0;
        p.dark_neutral = true;
    });
    mounted.settle();
    let edge = mounted.rect("Sidebar");
    let position = Vector2::new(edge.origin.x + edge.size.width - 1.0, 120.0);
    for _ in 0..2 {
        for kind in [
            PointerEventKind::Down(PointerButton::Primary),
            PointerEventKind::Up(PointerButton::Primary),
        ] {
            mounted.ui.dispatch_pointer(PointerEvent {
                kind,
                position,
                pointer_type: PointerType::Mouse,
                modifiers: Modifiers::default(),
                timestamp: Duration::ZERO,
            });
            mounted.settle();
        }
    }
    assert_eq!(
        mounted.model.preferences.get_untracked().sidebar_width,
        220.0
    );
    assert!(mounted.model.preferences.get_untracked().dark_neutral);
    assert!((mounted.rect("Sidebar").size.width - 220.0).abs() < 1.0);
}

#[test]
fn direct_project_connections_and_header_actions_use_full_cells() {
    let mut mounted = mount(false, 1380.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    mounted.click("Project Connections for Relay · demo");
    assert_eq!(mounted.model.page.get_untracked(), Page::Connections);
    mounted.model.page.set(Page::Board);
    mounted.settle();
    let new = mounted.rect("New task");
    let more = mounted.rect("Board actions");
    assert_eq!(new.origin.y, more.origin.y);
    assert_eq!(new.size.height, more.size.height);
    assert!(more.size.height > 34.0);
    let tree = mounted.rect("Project Connections for Relay · demo");
    assert_eq!(tree.size.width, tree.size.height);
    let nodes = mounted.ui.inspection_snapshot();
    let button = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Project Connections for Relay · demo"))
        .unwrap();
    let icon = nodes
        .nodes
        .iter()
        .find(|n| n.parent == Some(button.id))
        .unwrap();
    assert!((icon.rect.center().x - tree.center().x).abs() < 0.1);
    assert!((icon.rect.center().y - tree.center().y).abs() < 0.1);
    let count = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("Task session count"))
        .unwrap();
    let footer = nodes.node(count.parent.unwrap()).unwrap();
    assert_eq!(count.rect.size.height, footer.rect.size.height);
    mounted
        .model
        .snapshot
        .set(demo_snapshot(DirectorProfile::default()));
    mounted.settle();
    mounted.size = Size::new(820.0, 900.0);
    mounted.settle();
    mounted.size = Size::new(1600.0, 900.0);
    mounted.settle();
    assert_eq!(
        mounted.rect("Source").origin.x + mounted.rect("Source").size.width,
        mounted.rect("Board actions").origin.x
    );
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.settle();
    let nodes = mounted.ui.inspection_snapshot();
    let new = nodes
        .nodes
        .iter()
        .find(|n| n.label.as_deref() == Some("New task"))
        .unwrap();
    let details = mounted.ui.inspection_details(new.id).unwrap();
    let expected =
        mosaic::render::PaintSpec::solid(mosaic::core::theme::color(crate::theme::ink.inverse));
    assert_eq!(
        details
            .attributes
            .iter()
            .find(|a| a.name == "fill")
            .unwrap()
            .value,
        format!("{expected:?}")
    );
}

#[test]
fn transcript_sender_and_content_share_the_first_glyph_baseline() {
    let mounted = mount(false, 1380.0);
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.messages.retain(|m| m.session_id == "session-plan");
    snapshot.messages.truncate(1);
    snapshot.messages[0].kind = "message".into();
    snapshot.messages[0].author = "Agent".into();
    snapshot.messages[0].body = "Agent output".into();
    snapshot.messages[0].parts.clear();
    mounted.model.snapshot.set(snapshot);
    mounted.model.open_session("session-plan".into());
    mounted.settle();
    let body = mounted.rect("Message m1");
    let glyphs: Vec<_> = mounted
        .ui
        .scene()
        .cmds
        .iter()
        .filter_map(|cmd| match cmd {
            mosaic::render::PaintCmd::Glyphs(run) => run.glyphs.first().map(|g| (g.dest, g.key)),
            _ => None,
        })
        .filter(|(r, _)| {
            r.origin.y >= body.origin.y - 10.0
                && r.origin.y < body.origin.y + body.size.height
                && r.origin.x > 220.0
        })
        .collect();
    assert_eq!(glyphs.len(), 2, "{glyphs:?}");
    assert_eq!(glyphs[0].1, glyphs[1].1);
    assert!(
        (glyphs[0].0.origin.y - glyphs[1].0.origin.y).abs() < 1.0,
        "{glyphs:?}"
    );
    mounted.model.snapshot.update(|s| {
        s.messages[0].author = "Personal name".into();
        s.messages[0].kind = "prompt".into();
        s.messages[0].body = "You asked for this".into();
    });
    mounted.settle();
    assert!(has_label(&mounted, "You"));
    assert!(!has_label(&mounted, "Personal name"));
    assert_eq!(
        mounted.model.snapshot.get_untracked().messages[0].author,
        "Personal name"
    );
}

#[test]
fn removed_settings_selectors_do_not_react_to_later_preferences() {
    let mounted = mount(false, 1380.0);
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Theme: Light");
    mounted.model.page.set(Page::Board);
    mounted.settle();
    mounted.model.preferences.update(|p| {
        p.mode = crate::settings::ThemeMode::Dark;
        p.scale = 1.3;
    });
    mounted.settle();
    mounted.model.page.set(Page::Settings);
    mounted.settle();
    mounted.click("Theme: System");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::System
    );
}

#[test]
fn first_director_message_uses_inline_assets_shortcuts_and_retains_edits_on_acknowledgement() {
    let mut mounted = mount(false, 600.0);
    mounted.model.snapshot.set(local_project_snapshot());
    mounted.model.snapshot.update(|s| s.sessions.clear());
    mounted.model.open_director("director-main".into());
    mounted.settle();
    let label = mounted
        .ui
        .inspection_snapshot()
        .nodes
        .into_iter()
        .find_map(|n| n.label.filter(|l| l.starts_with("Draft text")))
        .unwrap();
    type_in(&mounted, &label, "Before λ");
    mounted.key(Key::Enter, false);
    mounted.ui.set_clipboard_text("After");
    mounted.key(Key::Character("v".into()), true);
    let session = mounted.model.session.get_untracked();
    let mut parts = crate::buffer::parts(mounted.model, &session);
    assert_eq!(plain_text(&parts), "Before λ\nAfter");
    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    mounted.model.buffer_requests.set(Some(requests));
    mounted.model.buffer.update(|s| {
        s.initialized = true;
        s.connected = true;
    });
    crate::buffer::flush(mounted.model);
    assert!(received.try_recv().is_err());
    mounted.model.buffer_requests.set(None);
    let asset = Asset {
        id: uuid::Uuid::new_v4().to_string(),
        name: "first.png".into(),
        media_type: "image/png".into(),
        size: 1,
    };
    parts.push(Part {
        id: uuid::Uuid::new_v4().to_string(),
        kind: PartKind::Asset {
            asset: asset.clone(),
        },
    });
    crate::buffer::edit(mounted.model, &session, parts.clone());
    mounted.model.buffer.update(|s| {
        s.uploads.insert(asset.id.clone(), asset.clone());
    });
    mounted.settle();
    mounted.rect("Preview first.png");
    mounted.click("Send message");
    assert!(mounted.commands.try_recv().is_err());
    mounted.model.buffer.update(|s| {
        s.uploads.clear();
    });
    mounted.settle();
    mounted.focus(&label);
    mounted.key(Key::Enter, true);
    let command = mounted.commands.try_recv().unwrap();
    assert!(
        matches!(command.command, Command::StartDirector { parts: ref sent, approve_implementation: false, .. } if sent == &parts)
    );
    // Typing while the request is in flight must survive switching to the real session.
    let mut later = parts.clone();
    later.push(Part::text("more"));
    crate::buffer::edit(mounted.model, &session, later.clone());
    crate::buffer::acknowledge_first(
        mounted.model,
        &Draft {
            session_id: session.clone(),
            revision: 0,
            parts,
        },
        "session-created",
    );
    assert_eq!(
        crate::buffer::parts(mounted.model, "session-created"),
        later
    );
    assert!(crate::buffer::parts(mounted.model, &session).is_empty());
}

#[test]
fn platform_chrome_keeps_native_macos_buttons_and_client_controls_elsewhere() {
    let config = crate::window_chrome::window_config();
    assert_eq!(config.size, Size::new(1380.0, 900.0));
    #[cfg(target_os = "macos")]
    {
        assert_eq!(config.decorations, WindowDecorations::Native);
        assert!(config.macos.transparent_titlebar);
        assert!(config.macos.title_hidden);
        assert!(!config.macos.hidden_titlebar);
        assert!(config.macos.full_size_content_view);
        assert!(config.macos.traffic_lights_visible);
        assert_eq!(config.macos.traffic_light_inset, Some(28.0));
    }
    #[cfg(not(target_os = "macos"))]
    assert_eq!(config.decorations, WindowDecorations::Client);
}

#[test]
fn merged_headers_keep_window_controls_at_the_right_across_pages_and_scales() {
    for (light, width, scale) in [
        (false, 1380.0, 1.0),
        (true, 820.0, 1.0),
        (false, 760.0, 2.0),
    ] {
        let mounted = mount(light, width);
        mounted
            ._scope
            .run(|| crate::settings::bind(mounted.model, AppContext::detached()));
        mounted.model.preferences.update(|p| p.scale = scale);
        for page in [
            Page::Board,
            Page::Sessions,
            Page::Directors,
            Page::Settings,
            Page::NewProject,
            Page::Connections,
            Page::Publish,
            Page::DirectorStart,
        ] {
            mounted.model.page.set(page);
            mounted.model.notice.set("A connection notice".into());
            mounted.settle();
            #[cfg(not(target_os = "macos"))]
            {
                for label in [
                    "Minimize window",
                    "Maximize or restore window",
                    "Close window",
                ] {
                    assert_control_hit(&mounted, label);
                    assert!(
                        mounted.rect(label).origin.y.abs() < 1.0,
                        "{page:?}: {label}"
                    );
                }
                let close = mounted.rect("Close window");
                assert!(
                    (close.origin.x + close.size.width - width).abs() < 1.0,
                    "{page:?}: {close:?}"
                );
                assert_eq!(
                    mounted
                        .ui
                        .inspection_snapshot()
                        .nodes
                        .iter()
                        .filter(|n| n.label.as_deref() == Some("Close window"))
                        .count(),
                    1
                );
            }
            #[cfg(target_os = "macos")]
            {
                assert!(!has_label(&mounted, "Close window"));
                assert_eq!(mounted.rect("Sidebar header").size.height, 56.0);
            }
        }
        mounted.model.page.set(Page::Board);
        mounted.model.issue.set(Some("issue-1".into()));
        mounted.settle();
        #[cfg(not(target_os = "macos"))]
        {
            assert_control_hit(&mounted, "Close window");
            let close = mounted.rect("Close window");
            assert!(close.origin.y.abs() < 1.0);
            assert!((close.origin.x + close.size.width - width).abs() < 1.0);
            assert_eq!(
                mounted
                    .ui
                    .inspection_snapshot()
                    .nodes
                    .iter()
                    .filter(|n| n.label.as_deref() == Some("Close window"))
                    .count(),
                1
            );
        }
        mounted.model.receive(NetworkState::default());
        mounted.model.snapshot.set(Snapshot::default());
        mounted.settle();
        #[cfg(not(target_os = "macos"))]
        assert_control_hit(&mounted, "Close window");
    }
}

#[test]
fn header_drag_targets_include_text_and_gaps_but_exclude_interactive_descendants() {
    let mounted = mount(false, 1380.0);
    let draggable = |header: &str, position: Vector2| {
        let nodes = mounted.ui.inspection_snapshot().nodes;
        let header = nodes
            .iter()
            .find(|n| n.label.as_deref() == Some(header))
            .unwrap()
            .id;
        let target = mounted.ui.hit_test(position).unwrap().id();
        crate::window_chrome::is_drag_target(&nodes, target, header)
    };
    assert!(draggable(
        "Sidebar header",
        mounted.rect("Sidebar header").center()
    ));
    let header = mounted.rect("Page header");
    for offset in [10.0, 100.0, 240.0] {
        assert!(draggable(
            "Page header",
            header.origin + Vector2::new(offset, 20.0)
        ));
    }
    assert!(!draggable(
        "Page header",
        mounted.rect("Open command palette").center()
    ));
    assert!(!draggable(
        "Page header",
        mounted.rect("Board actions").center()
    ));
    #[cfg(not(target_os = "macos"))]
    assert!(!draggable(
        "Page header",
        mounted.rect("Close window").center()
    ));
    mounted.click("Open command palette");
    assert!(mounted.model.palette.get_untracked());
    mounted.model.palette.set(false);
    mounted.model.open_director_profile("director-main".into());
    mounted.settle();
    assert!(!draggable(
        "Profile header",
        mounted.rect("Director name").center()
    ));
    mounted.click("Director name");
    mounted.key(Key::Character("X".into()), false);
    assert!(mounted.model.editor_name.get_untracked().contains('X'));
}

#[test]
fn client_resize_targets_cover_edges_without_covering_header_controls() {
    if cfg!(target_os = "macos") {
        return;
    }
    let mounted = mount(false, 1380.0);
    for (label, point) in [
        ("Resize window North", Vector2::new(500.0, 2.0)),
        ("Resize window East", Vector2::new(1378.0, 400.0)),
        ("Resize window South", Vector2::new(500.0, 898.0)),
        ("Resize window West", Vector2::new(2.0, 400.0)),
        ("Resize window NorthWest", Vector2::new(2.0, 2.0)),
        ("Resize window SouthEast", Vector2::new(1378.0, 898.0)),
    ] {
        let target = mounted.ui.hit_test(point).unwrap().id();
        assert_eq!(
            mounted
                .ui
                .inspection_snapshot()
                .nodes
                .iter()
                .find(|n| n.id == target)
                .unwrap()
                .label
                .as_deref(),
            Some(label)
        );
    }
    assert_control_hit(&mounted, "Close window");
}

#[test]
fn native_gesture_handoff_releases_capture_when_the_os_consumes_mouse_up() {
    let mounted = mount(false, 1380.0);
    let header = mounted.rect("Page header");
    let positions = std::iter::once(header.origin + Vector2::new(100.0, 20.0)).chain(
        if cfg!(target_os = "macos") {
            vec![]
        } else {
            vec![Vector2::new(1378.0, 400.0), Vector2::new(1378.0, 898.0)]
        },
    );
    for (index, position) in positions.enumerate() {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind: PointerEventKind::Down(PointerButton::Primary),
            position,
            pointer_type: PointerType::Mouse,
            modifiers: Modifiers::default(),
            timestamp: Duration::from_secs(index as u64 + 1),
        });
        // No Up: the window manager owns that release. Flush the handoff
        // before the next pointer press, as the native runtime does.
        mounted.settle();
        assert!(mounted.model.window_gesture.get_untracked().is_none());
        mounted.click("Open command palette");
        assert!(
            mounted.model.palette.get_untracked(),
            "capture retained at {position:?}"
        );
        mounted.model.palette.set(false);
        mounted.settle();
    }
}

#[test]
fn streamed_messages_keep_the_bottom_visible_and_preserve_manual_reading_position() {
    let mounted = mount(false, 1100.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|s| {
        s.messages.push(Message {
            id: "stream-follow".into(),
            session_id: "session-plan".into(),
            author: "Agent".into(),
            kind: "assistant".into(),
            body: "line\n".repeat(80),
            parts: vec![],
            tool: None,
        })
    });
    mounted.settle();
    let next = mounted.rect("Next message");
    assert!(next.origin.y + next.size.height < mounted.size.height);
    mounted.model.snapshot.update(|s| {
        s.messages
            .iter_mut()
            .find(|m| m.id == "stream-follow")
            .unwrap()
            .body
            .push_str(&"new line\n".repeat(40))
    });
    mounted.settle();
    let following = mounted.rect("Next message");
    assert!((following.origin.y - next.origin.y).abs() < 2.0);
    mounted.ui.dispatch_pointer(PointerEvent {
        kind: PointerEventKind::Wheel {
            delta: Vector2::new(0.0, 400.0),
        },
        position: Vector2::new(800.0, 400.0),
        pointer_type: PointerType::Mouse,
        modifiers: Modifiers::default(),
        timestamp: Duration::ZERO,
    });
    mounted.settle();
    let reading = mounted.rect("Message stream-follow");
    mounted.model.snapshot.update(|s| {
        s.messages
            .iter_mut()
            .find(|m| m.id == "stream-follow")
            .unwrap()
            .body
            .push_str(&"more line\n".repeat(40))
    });
    mounted.settle();
    assert!((mounted.rect("Message stream-follow").origin.y - reading.origin.y).abs() < 2.0);
}

#[test]
fn stop_control_fills_the_session_header_cell() {
    let mounted = mount(false, 1380.0);
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|session| session.id == "session-worker")
            .unwrap()
            .worker = Some(worker_run(WorkerStatus::Running));
    });
    mounted.model.open_session("session-worker".into());
    mounted.settle();
    let header = mounted.rect("Session header");
    let stop = mounted.rect("Stop worker");
    assert!((stop.origin.y - header.origin.y).abs() < 0.1);
    assert!((stop.size.height - header.size.height).abs() < 0.1);
    assert!(
        (stop.origin.x + stop.size.width - mounted.rect("Session actions").origin.x).abs() < 0.1
    );
}

#[test]
fn reopened_session_retains_model_and_measured_context_without_claiming_live_cache() {
    let mounted = mount(false, 2400.0);
    let mut snapshot = live_snapshot();
    let mut run = worker_run(WorkerStatus::Completed);
    run.model = Some("recorded-model".into());
    run.context_window = Some(200_000);
    run.last_usage = Some(TokenUsage {
        input_tokens: 42_000,
        cached_input_tokens: 40_000,
        output_tokens: 1000,
    });
    snapshot.sessions[0].worker = Some(run);
    let snapshot: Snapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    let cells = crate::conversation::run_cells(&snapshot, &snapshot.sessions[0]);
    for expected in [
        ("Model", "recorded-model"),
        ("Last prompt", "42000 tokens"),
        ("Context", "— / 200,000"),
        ("Cache", "Unknown"),
        ("Re-cache", "Unknown"),
    ] {
        assert!(
            cells
                .iter()
                .any(|(key, value)| *key == expected.0 && value == expected.1)
        );
    }
    let id = snapshot.sessions[0].id.clone();
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.model.open_session(id);
    mounted.settle();
    let strip = mounted.rect("Session run");
    let last = mounted.rect("Next-turn approval");
    let header = mounted.rect("Session header");
    assert!(
        (strip.size.width - header.size.width).abs() <= 1.0,
        "strip: {strip:?}, header: {header:?}"
    );
    assert!(
        (last.origin.x + last.size.width - strip.origin.x - strip.size.width).abs() <= 1.0,
        "strip: {strip:?}, last: {last:?}"
    );
}

#[test]
fn tool_activity_updates_in_place_and_discloses_input_at_narrow_widths() {
    for light in [false, true] {
        let mounted = mount(light, 820.0);
        mounted.model.open_session("session-plan".into());
        mounted.model.snapshot.update(|s| {
            s.messages.retain(|m| m.session_id != "session-plan");
            s.messages.push(Message {
                id: "terminal-call".into(),
                session_id: "session-plan".into(),
                author: "Codex".into(),
                kind: "tool_call".into(),
                body: "first output\n".into(),
                parts: vec![],
                tool: Some(ToolCall {
                    run_id: "turn".into(),
                    name: "commandExecution".into(),
                    kind: ToolKind::Terminal,
                    state: ToolState::Running,
                    input: "printf hello".into(),
                    target: "printf hello".into(),
                    directory: Some("/workspace".into()),
                    exit_code: None,
                }),
            });
        });
        mounted.settle();
        assert!(has_label(&mounted, "Terminal: Running"));
        mounted.model.snapshot.update(|s| {
            s.tool_permissions.push(ToolPermission {
                id: "tool-approval".into(),
                session_id: "session-plan".into(),
                run_id: "turn".into(),
                tool: "commandExecution".into(),
                description: "printf hello".into(),
                decision: None,
                expired: false,
            })
        });
        mounted.settle();
        assert!(has_label(&mounted, "Terminal: Waiting for approval"));
        mounted
            .model
            .snapshot
            .update(|s| s.tool_permissions.clear());
        mounted.settle();
        let header = mounted.rect("Toggle Terminal activity terminal-call");
        assert!(header.size.width > 200.0);
        assert!(header.origin.x + header.size.width <= mounted.size.width);
        mounted.click("Toggle tool output terminal-call");
        assert!(has_label(&mounted, "Message terminal-call"));
        mounted.click("Toggle tool output terminal-call");
        mounted.click("Toggle tool input terminal-call");
        mounted.model.snapshot.update(|s| {
            let m = s
                .messages
                .iter_mut()
                .find(|m| m.id == "terminal-call")
                .unwrap();
            m.body.push_str("failed output\n");
            let t = m.tool.as_mut().unwrap();
            t.state = ToolState::Failed;
            t.exit_code = Some(1);
        });
        mounted.settle();
        assert!(has_label(&mounted, "Terminal: Failed"));
        mounted.click("Toggle Terminal activity terminal-call");
        assert!(!has_label(&mounted, "Toggle tool input terminal-call"));
        assert!(has_label(&mounted, "Terminal: Failed"));
        mounted.click("Toggle Terminal activity terminal-call");
        assert!(has_label(&mounted, "Toggle tool input terminal-call"));
    }
}

#[test]
fn tool_categories_keep_distinct_status_labels_and_file_changes_render() {
    let mounted = mount(false, 1380.0);
    mounted.model.open_session("session-plan".into());
    mounted.model.snapshot.update(|s| {
        s.messages.retain(|m| m.session_id != "session-plan");
        for (i, kind) in [
            ToolKind::Edit,
            ToolKind::Read,
            ToolKind::Search,
            ToolKind::Web,
            ToolKind::Agent,
            ToolKind::Other,
        ]
        .into_iter()
        .enumerate()
        {
            s.messages.push(Message {
                id: format!("tool-{i}"),
                session_id: "session-plan".into(),
                author: "Agent".into(),
                kind: "tool_call".into(),
                body: "result".into(),
                parts: vec![],
                tool: Some(ToolCall {
                    run_id: "run".into(),
                    name: "tool".into(),
                    kind,
                    state: ToolState::Completed,
                    input: r#"[{"path":"src/main.rs","diff":"@@ -1 +1 @@\n-old\n+new"}]"#.into(),
                    target: "src/main.rs".into(),
                    directory: None,
                    exit_code: None,
                }),
            });
        }
    });
    mounted.settle();
    for label in [
        "File changes",
        "Read file",
        "Search",
        "Web",
        "Agent",
        "Tool",
    ] {
        assert!(has_label(&mounted, &format!("{label}: Completed")));
    }
}
