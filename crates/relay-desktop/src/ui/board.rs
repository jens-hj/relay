use super::*;

#[component]
pub(crate) fn Board(model: Model, narrow: Derived<bool>) -> Element {
    let columns = Derived::new(move || model.board_columns());
    view! {
        col width:1fr pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) gap:{px(12.0)}px {
            scroll {
                col height:min-content gap:{px(14.0)}px {
                    row height:min-content min-height:{px(36.0)}px align:center gap:{px(12.0)}px
                        pad:(bottom:{px(8.0)}px)
                        stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                        text width:1fr font-size:{px(12.0)}px font-color:{color(ink.muted)}
                            label:"Board source"
                            {
                if let Some(board)=model.selected_board() {board_source_caption(&board) } else {
                    model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| match &p.github {
                        Some(g) => with_board_error(format!("{} · board {} #{} · {}",g.url,g.owner,g.number,sync_label(g.last_synced_at)),g.sync_error.as_deref()),
                        None => "Fixture board".into()
                    }).unwrap_or_default()
                }
            }
                        if model.selected_board().is_some_and(|b| b.source != BoardSource::Local) || (model.selected_board().is_none() && model.snapshot.get().projects.iter().any(|p| p.id==model.project.get() && p.github.is_some()) && !model.snapshot.get().boards.iter().any(|b|b.project_id==model.project.get())) {
                            button #relay.action @click:{ model.sync_project(); }
                                label:"Sync project"
                                disabled:{ model.busy.get() || !model.connected.get() }
                                "Sync project"
                        }
                    }
                    BoardActions model:(model)
                    if narrow.get() {
                        col height:min-content gap:{px(18.0)}px {
                            for (_, column) in { columns.get().into_iter().map(|c| (c.id.clone(), c)) } {
                                col width:1fr height:min-content {
                                    BoardColumnView model:(model) column:(column.clone())
                                }
                            }
                        }
                    } else {
                        row height:min-content gap:{px(16.0)}px align:start {
                            for (_, column) in { columns.get().into_iter().map(|c| (c.id.clone(), c)) } {
                                col width:1fr height:min-content {
                                    BoardColumnView model:(model) column:(column.clone())
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn BoardColumnView(model: Model, column: BoardColumn) -> Element {
    let id = column.id.clone();
    let column_id = State::new(column.id.clone());
    let issues = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .filter(|i| model.task_in_column(i, &id))
            .collect::<Vec<_>>()
    });
    view! {
        col height:min-content width:1fr gap:{px(12.0)}px {
            row height:min-content justify:between align:center
                pad:(horizontal:{px(2.0)}px vertical:{px(10.0)}px)
                stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                text font-size:{px(13.0)}px font-weight:{650} font-family:{FontFamily::SansSerif}
                    label:{ format!("Column {}", column_id.get()) }
                    { model.board_columns().iter().find(|c| c.id == column_id.get()).map(|c| c.title.clone()).unwrap_or_default() }
                text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                    { format!("{:02}", issues.get().len()) }
            }
            for (_, issue) in { issues.get().into_iter().map(|i| (i.id.clone(), i)) } {
                IssueCard model:(model) issue:(issue.clone())
            }
        }
    }
}

#[component]
pub(crate) fn IssueCard(model: Model, issue: Issue) -> Element {
    let id = issue.id.clone();
    let current_id = issue.id.clone();
    let current = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .find(|i| i.id == current_id)
            .unwrap_or_else(|| issue.clone())
    });
    let count_id = id.clone();
    let session_count = Derived::new(move || model.sessions_for_task(&count_id).len());
    let status_id = id.clone();
    let state = Derived::new(move || {
        crate::labels::issue_status(&model.snapshot.get(), &model.sessions_for_task(&status_id))
    });
    let selected_id = id.clone();
    let selected = Derived::new(move || model.issue.get().as_deref() == Some(selected_id.as_str()));
    view! {
        button @click:{ model.issue.set(Some(id.clone())); model.worker_approval.set(false); }
            width:fill height:min-content fill:surface.panel pad:0px radius:0px
            label:{ current.get().reference.map(|r|format!("Open issue #{}",r.number)).unwrap_or_else(||format!("Open local task {}",current.get().title)) }
            description:{ format!("{} · {} linked sessions", state.get().label(), session_count.get()) }
            stroke:(width:{px(if selected.get() {2.0} else {1.0})} color:{color(if selected.get() {ink.fg} else {rule.line})} offset:{px(-1.0)})
            hover { fill:surface.raised }
            focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(2.0)}) } {
            col height:min-content gap:0px {
                row height:{px(24.0)}px align:center
                    stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                    row width:max-content align:center pad:(horizontal:{px(8.0)}px vertical:0px)
                        fill:ink.inverse {
                        text text-wrap:none font-size:{px(12.0)}px font-weight:{700}
                            font-color:{color(ink.on_inverse)} { issue_number(&current.get()) }
                    }
                    row width:1fr align:center gap:{px(4.0)}px
                        pad:(horizontal:{px(6.0)}px vertical:0px) clip {
                        for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                            Tag text:(label.clone())
                        }
                    }
                }
                col height:min-content gap:{px(6.0)}px
                    pad:(horizontal:{px(12.0)}px vertical:{px(12.0)}px) {
                    text font-size:{px(15.0)}px font-weight:{600}
                        font-family:{FontFamily::SansSerif} font-color:{color(ink.fg)}
                        label:{ current.get().title } { current.get().title }
                    if !body_preview(&current.get().body).is_empty() {
                        col height:min-content max-height:{px(52.0)}px clip label:"Task preview" {
                            text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                                { body_preview(&current.get().body) }
                        }
                    }
                }
                row height:{px(28.0)}px align:center gap:{px(6.0)}px
                    pad:(horizontal:{px(10.0)}px vertical:0px)
                    stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                    if state.get() != RunState::Ready {
                        StatusGlyph state:(state)
                    }
                    row width:1fr min-width:0px align:center clip {
                        text width:max-content text-wrap:none font-size:{px(11.0)}px
                            font-color:{color(state.get().text_color())} { state.get().label() }
                    }
                    if session_count.get() > 0 {
                        text width:max-content shrink:0 text-wrap:none font-size:{px(11.0)}px
                            font-color:{color(ink.muted)}
                            { format!("{} {}", session_count.get(), if session_count.get() == 1 { "session" } else { "sessions" }) }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn IssueDetail(model: Model) -> Element {
    let issue = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .find(|i| Some(&i.id) == model.issue.get().as_ref())
    });
    view! {
        col width:{px(340.0)} shrink:1 fill:surface.panel
            stroke:(width:{px(1.0)} color:rule.line edges:left) {
            row height:min-content align:center justify:between
                pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px)
                stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                text font-size:{px(11.0)}px font-color:ink.muted text-transform:uppercase
                    letter-spacing:{px(0.6)}px "Issue details"
                button #relay.action @click:{ model.issue.set(None); } label:"Close issue details"
                    "Close"
            }
            scroll {
                for (_, detail) in { issue.get().into_iter().map(|i| (i.id.clone(), i)) } {
                    let detail_id = State::new(detail.id.clone());
                    let fallback = detail.clone();
                    let current = Derived::new(move || model.snapshot.get().issue(&detail_id.get()).cloned().unwrap_or_else(|_| fallback.clone()));
                    col height:min-content gap:{px(18.0)}px
                        pad:(horizontal:{px(18.0)}px vertical:{px(16.0)}px) selectable {
                        row height:min-content gap:{px(12.0)}px align:start {
                            col width:max-content height:min-content min-height:{px(56.0)}px
                                min-width:{px(56.0)}px align:center justify:center
                                pad:(horizontal:{px(8.0)}px vertical:0px) fill:ink.inverse {
                                text text-wrap:none font-size:{px(20.0)}px font-weight:{700}
                                    font-color:{color(ink.on_inverse)}
                                    { issue_number(&current.get()) }
                            }
                            col width:1fr height:min-content gap:{px(8.0)}px {
                                text font-size:{px(19.0)}px font-weight:{650}
                                    font-family:{FontFamily::SansSerif}
                                    label:{ current.get().title } { current.get().title }
                                row height:min-content gap:{px(4.0)}px clip {
                                    for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                                        Tag text:(label.clone())
                                    }
                                }
                            }
                        }
                        text font-size:{px(14.0)}px
                            label:{ crate::projects::task_body(&current.get().body) }
                            { crate::projects::task_body(&current.get().body) }
                        col height:min-content gap:{px(3.0)}px pad:(top:{px(8.0)}px)
                            stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                            text font-size:{px(11.0)}px font-color:{color(ink.muted)}
                                text-transform:{TextTransform::Uppercase} letter-spacing:{px(0.6)}px
                                { if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "Source" } else { "Issue" } }
                            text font-size:{px(12.0)}px font-color:{color(ink.fg)}
                                { if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "Fixture issue · execution unavailable".to_string() } else { current.get().reference.map(|r| r.url).unwrap_or_else(|| "Local task".into()) } }
                        }
                        if !model.snapshot.get().visible_task(&current.get().id) {
                            col height:min-content pad:{px(10.0)}px fill:attention.fill
                                stroke:(width:{px(4.0)} color:attention.text edges:left) {
                                text font-size:{px(12.0)}px font-color:attention.on
                                    label:"Issue removed from board"
                                    "No longer on this board · history retained. Restore and sync before starting or continuing a worker. Active turns may finish or be stopped."
                            }
                        }
                        WorkerForm model:(model) continuation:false
                        row height:min-content justify:between pad:(top:{px(8.0)}px)
                            stroke:(width:{px(1.0)} color:rule.line edges:top) {
                            text font-size:{px(11.0)}px font-color:ink.muted
                                text-transform:uppercase letter-spacing:{px(0.6)}px
                                "Linked sessions"
                            text font-size:{px(11.0)}px font-color:{color(ink.muted)}
                                { format!("{:02}", model.sessions_for_task(&detail_id.get()).len()) }
                        }
                        for (_, session) in { model.sessions_for_task(&detail_id.get()).into_iter().map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                            let id = State::new(session.id.clone());
                            let linked_state = Derived::new(move || model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| crate::labels::session_state(&model.snapshot.get(), s)).unwrap_or(RunState::Unavailable));
                            button #relay.tree-control
                                @click:{ model.open_session(id.get_untracked()); } width:fill
                                justify:start gap:{px(8.0)}px
                                pad:(horizontal:{px(8.0)}px vertical:0px)
                                stroke:(width:{px(1.0)} color:rule.hair edges:bottom)
                                label:{ model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                                description:{ linked_state.get().label() }
                                hover { fill:surface.raised } {
                                StatusGlyph state:(linked_state)
                                row width:1fr min-width:0px align:center clip {
                                    text width:max-content text-wrap:none font-size:{px(12.0)}px
                                        font-color:{color(ink.fg)}
                                        { model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                                }
                                text width:max-content shrink:0 text-wrap:none
                                    font-size:{px(11.0)}px
                                    font-color:{color(linked_state.get().text_color())}
                                    { linked_state.get().label() }
                            }
                        }
                        TaskEditor model:(model)
                        if current.get().result.is_some() {
                            col height:min-content gap:{px(6.0)}px pad:(top:{px(8.0)}px)
                                stroke:(width:{px(1.0)} color:rule.line edges:top) {
                                text font-size:{px(11.0)}px font-color:ink.muted
                                    text-transform:uppercase letter-spacing:{px(0.6)}px "Result"
                                text font-size:{px(14.0)}px
                                    { current.get().result.unwrap_or_default() }
                            }
                        }
                    }
                }
            }
        }
    }
}
#[component]
pub(crate) fn WorkerApproval(model: Model) -> Element {
    let root = view! {
        col height:min-content {}
    };
    root.switch(
        move || {
            use mosaic::core::theme::{color, scalar};
            (
                scalar(ui_scale),
                color(surface.panel),
                color(accent.focus),
                color(rule.hair),
                color(surface.base),
            )
        },
        move |parent, &(scale, fill, accent_color, edge_color, mark)| {
            mosaic::widgets::checkbox_styled(
                parent,
                model.worker_approval,
                Some("Approve implementation for this turn"),
                CheckboxStyle {
                    fill,
                    fill_checked: accent_color,
                    stroke: edge_color,
                    mark,
                    focus: accent_color,
                    size: 18.0 * scale,
                    radius: 0.0,
                    label: TextStyle::inherited(),
                    gap: 8.0 * scale,
                },
            );
        },
    );
    root
}

#[component]
pub(crate) fn WorkerForm(model: Model, continuation: bool) -> Element {
    view! {
        col height:min-content gap:{px(10.0)}px {
            text font-size:{px(13.0)}px font-weight:{650} font-family:{FontFamily::SansSerif}
                { if continuation { "Continue this session" } else { "Start task worker" } }
            if !continuation {
                WorkspaceChoices model:(model)
                for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                    let id = State::new(director.id.clone());
                    button #relay.action
                        @click:{ model.worker_director.set(id.get_untracked()); model.worker_approval.set(false); }
                        width:fill justify:start role:radio
                        fill:if model.worker_director.get() == id.get() {surface.selected} else {surface.panel}
                        stroke:(width:{px(if model.worker_director.get() == id.get() {3.0} else {1.0})} color:{color(if model.worker_director.get() == id.get() {ink.fg} else {rule.line})} edges:left)
                        { model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| format!("Director: {}", d.name)).unwrap_or_default() }
                }
            }
            if model.worker_profile(continuation).is_ok() {
                grid
                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(120.0).into(), GridTrack::fr(1.0)))}
                    height:min-content gap:{px(10.0)}px pad:{px(10.0)}px
                    stroke:(width:{px(1.0)} color:rule.line offset:{px(-1.0)})
                    label:"Worker policy" {
                    for (_, key) in { worker_policy(model, continuation).into_iter().map(|(k, _)| (k, k)).collect::<Vec<_>>() } {
                        let field: &'static str = key;
                        Readout key:(field.to_string())
                            value:(Derived::new(move || worker_policy(model, continuation).into_iter().find(|(k, _)| *k == field).map(|(_, v)| v).unwrap_or_default()))
                    }
                }
            } else {
                text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                    { model.worker_profile(continuation).err().unwrap_or_default() }
            }
            input #relay.field label:"Worker prompt" placeholder:"Prompt for this turn…"
                model.worker_prompt
            if model.worker_profile(continuation).is_ok_and(|(p, _)| p.permissions.get(&Task::Implement) == Some(&Permission::Ask)) {
                WorkerApproval model:(model)
            }
            text font-size:{px(11.0)}px font-color:{color(ink.muted)}
                { model.worker_gate(continuation).err().unwrap_or_else(|| "Ready · server rechecks policy and revision".into()) }
            button #relay.primary @click:{ model.run_worker(continuation); }
                label:if continuation { "Send worker prompt" } else { "Start worker" }
                disabled:{ model.worker_gate(continuation).is_err() }
                { if continuation { "Send / continue" } else { "Start worker" } }
        }
    }
}

/// The board's source without repeating its name, which the page header and
/// board selector already show.
pub(crate) fn board_source_caption(board: &Board) -> String {
    let caption = match &board.source {
        BoardSource::Local => "Local board".to_string(),
        BoardSource::Github { owner, number, .. } => {
            format!(
                "GitHub {owner} · board {number} · {}",
                sync_label(board.last_synced_at)
            )
        }
        BoardSource::Gitlab {
            host,
            path,
            number,
            group,
            ..
        } => format!(
            "GitLab {host}/{path} · {} board {number} · {}",
            if *group { "group" } else { "project" },
            sync_label(board.last_synced_at)
        ),
    };
    with_board_error(caption, board.error.as_deref())
}

/// A short card preview of a task body: recovery markers removed, whitespace
/// collapsed and long text shortened at a word boundary.
pub(crate) fn body_preview(body: &str) -> String {
    const LIMIT: usize = 150;
    let text = crate::projects::task_body(body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT).collect();
    let trimmed = cut.rsplit_once(' ').map(|(head, _)| head).unwrap_or(&cut);
    format!("{}…", trimmed.trim_end_matches(['.', ',', ';', ':']))
}

pub(crate) fn sync_label(synced_at: Option<u64>) -> String {
    let Some(synced_at) = synced_at else {
        return "Not synced yet".into();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(synced_at);
    if age < 60 {
        return "Last synced just now".into();
    }
    let (count, unit) = if age < 3600 {
        (age / 60, "minute")
    } else if age < 86400 {
        (age / 3600, "hour")
    } else {
        (age / 86400, "day")
    };
    format!(
        "Last synced {count} {unit}{} ago",
        if count == 1 { "" } else { "s" }
    )
}

pub(crate) fn scope_label(scope: &DirectorScope, snapshot: &Snapshot) -> String {
    match scope {
        DirectorScope::Project => "All project issues".into(),
        DirectorScope::Issues { issue_ids } => format!(
            "Selected issues: {}",
            issue_ids
                .iter()
                .map(|id| {
                    snapshot
                        .issue(id)
                        .ok()
                        .map(|i| {
                            i.reference
                                .as_ref()
                                .map(|r| format!("#{}", r.number))
                                .unwrap_or_else(|| i.title.clone())
                        })
                        .unwrap_or_else(|| "unavailable issue".into())
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn issue_number(issue: &Issue) -> String {
    issue
        .reference
        .as_ref()
        .map(|r| format!("#{}", r.number))
        .unwrap_or_else(|| "Task".into())
}

pub(crate) fn with_board_error(mut caption: String, error: Option<&str>) -> String {
    if let Some(error) = error.filter(|e| !e.trim().is_empty()) {
        caption.push('\n');
        caption.push_str(error);
    }
    caption
}

pub(crate) fn worker_policy(model: Model, continuation: bool) -> Vec<(&'static str, String)> {
    let Ok((profile, active)) = model.worker_profile(continuation) else {
        return Vec::new();
    };
    vec![
        (
            "Harness",
            match profile.harness {
                Harness::Codex => "Codex".into(),
                Harness::ClaudeCode => "Claude Code".into(),
            },
        ),
        (
            "Implement",
            profile
                .permissions
                .get(&Task::Implement)
                .copied()
                .unwrap_or(Permission::Deny)
                .label()
                .to_string(),
        ),
        ("Scope", scope_label(&profile.scope, &model.snapshot.get())),
        (
            "Workers",
            format!("{active} / {} active", profile.max_workers),
        ),
    ]
}
