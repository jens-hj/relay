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
fn sidebar_tree_disclosure_keyboard_and_secondary_actions_are_independent() {
    let mounted = mount(false, 1380.0);
    mounted
        ._scope
        .run(|| crate::settings::bind(mounted.model, AppContext::detached(), None));
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
        assert_eq!(mounted.model.page.get_untracked(), Page::Directors);
        assert_eq!(
            mounted.model.editor.get_untracked(),
            EditTarget::Director("director-other".into())
        );
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
        .run(|| crate::settings::bind(mounted.model, AppContext::detached(), None));
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
        .run(|| crate::settings::bind(mounted.model, AppContext::detached(), None));
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
        .run(|| crate::settings::bind(mounted.model, AppContext::detached(), None));
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
    mounted.click("Theme: Light");
    assert_eq!(
        mounted.model.preferences.get_untracked().mode,
        crate::settings::ThemeMode::Light
    );
    let paper = mosaic::core::theme::color(theme::base);
    mounted.click("Light palette: Warm");
    assert_ne!(mosaic::core::theme::color(theme::base), paper);
    mounted.click("Theme: Dark");
    let slate = mosaic::core::theme::color(theme::base);
    mounted.click("Dark palette: Neutral");
    assert_ne!(mosaic::core::theme::color(theme::base), slate);
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
        360.0
    );
    assert!((mounted.rect("Sidebar").size.width - 360.0).abs() < 1.0);
    mounted.size = Size::new(820.0, 360.0);
    mounted.settle();
    mounted.focus("Settings");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.page.get_untracked(), Page::Settings);
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
        .run(|| crate::settings::bind(mounted.model, AppContext::detached(), None));
    mounted.model.preferences.update(|p| {
        p.scale = 2.0;
        p.sidebar_width = 160.0;
    });
    mounted.size = Size::new(820.0, 600.0);
    mounted.key(Key::Character(",".into()), true);
    for label in [
        "Theme: System",
        "Decrease interface scale",
        "Reset interface scale",
        "Reset sidebar width",
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
    assert_eq!(
        mounted.model.preferences.get_untracked().sidebar_width,
        220.0
    );
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
            .unwrap()
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
                    rect.origin.y >= 0.0 && rect.origin.y + rect.size.height <= self.size.height
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
    assert!(delegate.origin.y >= plan.origin.y + plan.size.height + 6.0);
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
    let mut snapshot = demo_snapshot(DirectorProfile::default());
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
        Command::StartWorker {
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
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Codex")
    );
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
        outcome: Some((send.request_id.clone(), Err("Continuation failed".into()))),
        outcome_serial: 2,
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
fn unmodified_default_director_can_delegate_after_explicit_approval() {
    let mut mounted = mount(false, 1380.0);
    let snapshot = live_snapshot();
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
    assert!(
        mounted
            .model
            .worker_gate(false)
            .unwrap_err()
            .contains("Approve")
    );
    mounted.model.worker_approval.set(true);
    assert!(mounted.model.worker_gate(false).is_ok());
    mounted.model.run_worker(false);
    assert!(matches!(
        mounted.commands.try_recv().unwrap().command,
        Command::StartWorker {
            approve_implementation: true,
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
