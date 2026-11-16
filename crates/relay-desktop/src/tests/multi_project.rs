use super::*;

fn two_projects() -> Snapshot {
    let mut snapshot = local_project_snapshot();
    snapshot.tool_permissions.clear();
    snapshot.submissions.clear();
    snapshot.sessions.retain(|s| s.id == "session-plan");
    snapshot.sessions[0].fixture = false;
    snapshot.sessions[0].worker = Some(worker_run(WorkerStatus::Completed));
    snapshot.sessions[0].workspaces = vec![SessionWorkspace {
        connection_id: "test-workspace".into(),
        path: "/srv/test/workspace".into(),
        repository: false,
        branch: None,
        base_commit: None,
        changes: None,
    }];
    snapshot
        .agent_configs
        .retain(|c| c.id == snapshot.sessions[0].agent_config_id);
    let mut project = snapshot.projects[0].clone();
    project.id = "other".into();
    project.name = "Other project".into();
    project.defaults.max_workers = 7;
    snapshot.projects.push(project);
    let mut config = snapshot.agent_configs[0].clone();
    config.id = "other-config".into();
    config.project_id = "other".into();
    config.name = "Other director".into();
    snapshot.agent_configs.push(config);
    let mut session = snapshot.sessions[0].clone();
    session.id = "other-session".into();
    session.project_id = "other".into();
    session.agent_config_id = "other-config".into();
    session.title = "Other agent".into();
    session.issue_id = Some("other-task".into());
    session.parent_session_id = None;
    snapshot.sessions.push(session);
    let mut task = snapshot.issues[0].clone();
    task.id = "other-task".into();
    task.project_id = "other".into();
    snapshot.issues.push(task);
    let mut board = snapshot.boards[0].clone();
    board.id = "other-board".into();
    board.project_id = "other".into();
    let column = board.columns[0].id.clone();
    snapshot.boards.push(board);
    snapshot.memberships.push(BoardMembership {
        board_id: "other-board".into(),
        issue_id: "other-task".into(),
        column_ids: vec![column],
        remote_item_id: None,
    });
    snapshot
}

fn mount_two_projects() -> Mounted {
    let mounted = mount(false, 1380.0);
    mounted.model.receive(NetworkState {
        snapshot: two_projects(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    mounted
}

#[test]
fn multiple_project_sections_keep_agents_visible_and_collapse_independently() {
    let mounted = mount_two_projects();
    assert!(has_row(&mounted, "Profile design"));
    assert!(!has_row(&mounted, "Other agent"));
    mounted.click("Toggle agents for Other project");
    assert!(has_row(&mounted, "Profile design"));
    assert!(has_row(&mounted, "Other agent"));
    assert_eq!(mounted.model.project.get_untracked(), "demo");
    mounted.click("Toggle agents for Relay · demo");
    assert!(!has_row(&mounted, "Profile design"));
    assert!(has_row(&mounted, "Other agent"));
    mounted.model.select_project("other".into());
    mounted.model.select_project("demo".into());
    mounted.settle();
    assert!(
        !has_row(&mounted, "Profile design"),
        "Selection respects explicit collapse"
    );
    mounted.click("Toggle agents for Relay · demo");
    assert!(has_row(&mounted, "Profile design"));
    assert!(has_row(&mounted, "Other agent"));
}

#[test]
fn project_agent_switching_preserves_sessions_boards_and_drafts() {
    let mounted = mount_two_projects();
    assert_eq!(mounted.model.issue.get_untracked(), None);
    mounted.click("Toggle agents for Other project");
    mounted.model.open_session("session-plan".into());
    let first_draft = vec![Part::text("First draft")];
    let second_draft = vec![Part::text("Second draft")];
    crate::buffer::edit(mounted.model, "session-plan", first_draft.clone());
    let board = mounted.model.selected_board().unwrap().id;
    let sessions = mounted.model.snapshot.get_untracked().sessions;
    mounted.settle();
    mounted.click("Other agent");
    assert_eq!(mounted.model.project.get_untracked(), "other");
    assert_eq!(mounted.model.session.get_untracked(), "other-session");
    assert_eq!(
        mounted.model.issue.get_untracked().as_deref(),
        Some("other-task")
    );
    assert_eq!(mounted.model.selected_board().unwrap().id, "other-board");
    crate::buffer::edit(mounted.model, "other-session", second_draft.clone());
    mounted.click("Profile design");
    assert_eq!(mounted.model.project.get_untracked(), "demo");
    assert_eq!(mounted.model.selected_board().unwrap().id, board);
    assert_eq!(
        crate::buffer::parts(mounted.model, "session-plan"),
        first_draft
    );
    assert_eq!(
        crate::buffer::parts(mounted.model, "other-session"),
        second_draft
    );
    assert_eq!(mounted.model.expanded_projects.get_untracked().len(), 2);
    mounted.model.select_project("other".into());
    assert_eq!(mounted.model.session.get_untracked(), "other-session");
    mounted.model.select_project("demo".into());
    assert_eq!(mounted.model.session.get_untracked(), "session-plan");
    assert_eq!(mounted.model.snapshot.get_untracked().sessions, sessions);
    // Remember a selection other than the first session in snapshot order.
    mounted.model.snapshot.update(|snapshot| {
        let mut session = snapshot.sessions[0].clone();
        session.id = "remembered-session".into();
        snapshot.sessions.push(session);
    });
    mounted.model.open_session("remembered-session".into());
    let selected_task = mounted.model.snapshot.get_untracked().issues[0].id.clone();
    mounted.model.issue.set(Some(selected_task.clone()));
    mounted.model.select_project("other".into());
    mounted.model.select_project("demo".into());
    assert_eq!(mounted.model.session.get_untracked(), "remembered-session");
    assert_eq!(mounted.model.issue.get_untracked(), Some(selected_task));
}

#[test]
fn project_sections_survive_snapshots_and_forget_removed_projects() {
    let mounted = mount_two_projects();
    mounted.click("Toggle agents for Other project");
    mounted.click("Toggle agents for Relay · demo");
    let mut snapshot = mounted.model.snapshot.get_untracked();
    snapshot.revision += 1;
    snapshot
        .sessions
        .iter_mut()
        .find(|s| s.id == "other-session")
        .unwrap()
        .title = "Updated other agent".into();
    mounted.model.receive(NetworkState {
        snapshot: snapshot.clone(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(!has_row(&mounted, "Profile design"));
    assert!(has_row(&mounted, "Updated other agent"));
    mounted.model.open_session("other-session".into());
    snapshot.projects.retain(|p| p.id != "other");
    snapshot.sessions.retain(|s| s.project_id != "other");
    snapshot.agent_configs.retain(|c| c.project_id != "other");
    mounted.model.receive(NetworkState {
        snapshot,
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert_eq!(mounted.model.project.get_untracked(), "demo");
    assert!(
        !mounted
            .model
            .expanded_projects
            .get_untracked()
            .contains("other")
    );
    assert!(!has_label(&mounted, "Toggle agents for Other project"));
    assert!(!has_row(&mounted, "Profile design"));
    mounted.model.receive(NetworkState {
        snapshot: Snapshot::default(),
        connected: true,
        ..Default::default()
    });
    mounted.settle();
    assert!(mounted.model.project.get_untracked().is_empty());
    assert!(mounted.model.session.get_untracked().is_empty());
}

#[test]
fn project_section_keyboard_collapse_keeps_focus_on_its_header() {
    let mounted = mount_two_projects();
    mounted.focus("Toggle agents for Other project");
    mounted.key(Key::ArrowRight, false);
    assert!(has_row(&mounted, "Other agent"));
    assert_eq!(
        focused_label(&mounted).as_deref(),
        Some("Toggle agents for Other project")
    );
    mounted.key(Key::ArrowLeft, false);
    assert!(!has_row(&mounted, "Other agent"));
    assert_eq!(
        focused_label(&mounted).as_deref(),
        Some("Toggle agents for Other project")
    );
    mounted.key(Key::Enter, false);
    assert!(has_row(&mounted, "Other agent"));
}

#[test]
fn inactive_project_profile_and_section_actions_use_their_owner() {
    for action in ["edit", "new", "defaults", "connections", "board", "worker"] {
        let mounted = mount_two_projects();
        mounted.click("Toggle agents for Other project");
        assert_eq!(mounted.model.project.get_untracked(), "demo");
        match action {
            "edit" => {
                focus_row(&mounted, "Other agent");
                mounted.focus("Edit Other agent");
                mounted.key(Key::Enter, false);
                assert_eq!(
                    mounted.model.editor.get_untracked(),
                    EditTarget::Config("other-config".into())
                );
                assert_eq!(mounted.model.editor_base.get_untracked().max_workers, 7);
            }
            "new" => {
                mounted.click_nth("New agent", 1);
                assert_eq!(mounted.model.editor.get_untracked(), EditTarget::New);
                assert_eq!(mounted.model.editor_base.get_untracked().max_workers, 7);
            }
            "defaults" => {
                mounted.click("Defaults for Other project");
                assert_eq!(mounted.model.editor.get_untracked(), EditTarget::Defaults);
                assert_eq!(mounted.model.editor_base.get_untracked().max_workers, 7);
            }
            "connections" => {
                mounted.click("Connections for Other project");
                assert_eq!(mounted.model.page.get_untracked(), Page::Connections);
            }
            "board" => {
                mounted.click("Project board for Other project");
                assert_eq!(mounted.model.page.get_untracked(), Page::Board);
                assert_eq!(mounted.model.selected_board().unwrap().id, "other-board");
            }
            "worker" => {
                focus_row(&mounted, "Other agent");
                mounted.focus("New worker for Other agent");
                mounted.key(Key::Enter, false);
                assert_eq!(
                    mounted.model.editor_parent.get_untracked().as_deref(),
                    Some("other-session")
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(mounted.model.project.get_untracked(), "other", "{action}");
        assert_eq!(
            mounted.model.expanded_projects.get_untracked().len(),
            2,
            "{action}"
        );
    }
}

#[test]
fn inactive_project_permission_approval_uses_the_clicked_sessions_request() {
    let mut mounted = mount_two_projects();
    let mut permission = orchestration().tool_permissions[0].clone();
    permission.id = "other-permission".into();
    permission.session_id = "other-session".into();
    permission.run_id = "other-run".into();
    mounted
        .model
        .snapshot
        .update(|s| s.tool_permissions.push(permission));
    mounted.click("Toggle agents for Other project");
    mounted.focus("Approve");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.project.get_untracked(), "demo");
    assert!(matches!(mounted.commands.try_recv().unwrap().command,
        Command::RespondPermission { permission_id, run_id, allow: true }
        if permission_id == "other-permission" && run_id == "other-run"));
    assert!(mounted.commands.try_recv().is_err());
}

#[test]
fn inactive_project_unstarted_config_opens_its_own_retained_draft() {
    let mounted = mount_two_projects();
    mounted
        .model
        .snapshot
        .update(|snapshot| snapshot.sessions.retain(|s| s.project_id != "other"));
    mounted.click("Toggle agents for Other project");
    mounted.click("Other director");
    assert_eq!(mounted.model.project.get_untracked(), "other");
    assert_eq!(
        mounted.model.session.get_untracked(),
        "agent-draft-other-config"
    );
    assert_eq!(mounted.model.page.get_untracked(), Page::AgentStart);
    let draft = vec![Part::text("A separate planning draft")];
    crate::buffer::edit(mounted.model, "agent-draft-other-config", draft.clone());
    mounted.click("Profile design");
    mounted.click("Other director");
    assert_eq!(
        crate::buffer::parts(mounted.model, "agent-draft-other-config"),
        draft
    );
    assert_eq!(mounted.model.expanded_projects.get_untracked().len(), 2);
}

#[test]
fn cross_project_archive_unarchive_and_stop_keep_their_session_target() {
    let mut mounted = mount_two_projects();
    mounted.click("Toggle agents for Other project");
    focus_row(&mounted, "Other agent");
    mounted.focus("Archive Other agent");
    mounted.key(Key::Enter, false);
    assert_eq!(mounted.model.project.get_untracked(), "demo");
    assert!(matches!(mounted.commands.try_recv().unwrap().command,
        Command::SetSessionArchived { session_id, archived: true } if session_id == "other-session"));
    drop(mounted);

    let mut mounted = mount_two_projects();
    mounted.model.snapshot.update(|snapshot| {
        snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == "other-session")
            .unwrap()
            .archived = true;
    });
    mounted.model.show_archived.set(true);
    mounted.click("Toggle agents for Other project");
    mounted.click("Other agent");
    mounted.click("Session actions");
    mounted.click("Toggle chat archive");
    assert!(matches!(mounted.commands.try_recv().unwrap().command,
        Command::SetSessionArchived { session_id, archived: false } if session_id == "other-session"));
    drop(mounted);

    let mut mounted = mount_two_projects();
    mounted.model.snapshot.update(|snapshot| {
        for session in &mut snapshot.sessions {
            session.worker = Some(worker_run(WorkerStatus::Running));
        }
    });
    mounted.click("Toggle agents for Other project");
    mounted.click("Other agent");
    seed_stop_targets(&mounted);
    mounted.click("Stop");
    assert!(matches!(mounted.commands.try_recv().unwrap().command,
        Command::StopWorkerRuns { session_id, include_descendants: false, runs, .. }
        if session_id == "other-session" && runs.len() == 1 && runs.contains_key("other-session")));
    assert_eq!(mounted.model.expanded_projects.get_untracked().len(), 2);
    assert_eq!(
        mounted.model.snapshot.get_untracked().sessions[0]
            .worker
            .as_ref()
            .unwrap()
            .status,
        WorkerStatus::Running
    );
}

#[test]
fn multiple_project_sections_fit_light_dark_scaled_and_mobile_navigation() {
    for light in [false, true] {
        for scale in [1.0, 1.5, 2.0] {
            for width in [1380.0, 390.0] {
                let mut mounted = mount_two_projects();
                mounted.size.width = width;
                install_theme(&theme(ThemeVariant::select(light, false), scale));
                mounted.model.set_project_expanded("other".into(), true);
                mounted.settle();
                if mounted.model.mobile_navigation.get_untracked() {
                    mounted.click("Open navigation");
                }
                let sidebar = mounted.rect("Sidebar");
                assert!(sidebar.size.width <= width);
                for name in [
                    "Toggle agents for Relay · demo",
                    "Toggle agents for Other project",
                ] {
                    mounted.focus(name);
                    let rect = mounted.rect(name);
                    assert!(rect.origin.x >= sidebar.origin.x - 0.1);
                    assert!(
                        rect.origin.x + rect.size.width
                            <= sidebar.origin.x + sidebar.size.width + 0.1
                    );
                }
                assert!(has_row(&mounted, "Profile design"));
                assert!(has_row(&mounted, "Other agent"));
                mounted.focus("Other agent");
                mounted.key(Key::Enter, false);
                assert_eq!(mounted.model.project.get_untracked(), "other");
                assert_eq!(mounted.model.session.get_untracked(), "other-session");
                if mounted.model.mobile_navigation.get_untracked() {
                    assert!(!mounted.model.sidebar_open.get_untracked());
                    mounted.click("Open navigation");
                    assert!(has_row(&mounted, "Profile design"));
                    assert!(has_row(&mounted, "Other agent"));
                }
            }
        }
    }
}

#[test]
#[ignore = "manual: requires a GPU adapter; writes native multi-project frames"]
fn multiple_project_native_frames() {
    let directory =
        std::env::var("RELAY_SCREENSHOTS").expect("Set a protected screenshot directory");
    for (name, light, scale, width) in [
        ("dark", false, 1.0, 1380.0),
        ("light", true, 1.0, 1380.0),
        ("scaled", false, 1.5, 1380.0),
        ("mobile", true, 1.0, 390.0),
    ] {
        let mut mounted = mount_two_projects();
        mounted.size.width = width;
        install_theme(&theme(ThemeVariant::select(light, false), scale));
        mounted.model.set_project_expanded("other".into(), true);
        mounted.settle();
        if mounted.model.mobile_navigation.get_untracked() {
            mounted.click("Open navigation");
        }
        crate::screenshots::render(
            &mounted.ui,
            mounted.size,
            1.0,
            &format!("{directory}/projects-{name}.png"),
        );
    }
}
