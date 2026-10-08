use crate::{
    controls::{AppearanceSegments, AppearanceSegmentsProps, ButtonStyle, button},
    conversation::{Conversation, ConversationProps},
    model::{EditTarget, Model, Page},
    projects::*,
    sidebar::{Sidebar, SidebarProps},
    theme::*,
};
use mosaic::prelude::*;
use relay_core::*;

#[component]
fn Settings(model: Model) -> Element {
    let scale = State::new(model.preferences.get_untracked().scale * 100.0);
    Effect::new(move || {
        let value = (scale.get() / 100.0).clamp(0.8, 2.0);
        if model.preferences.get_untracked().scale != value {
            model.preferences.update(|p| p.scale = value);
        }
    });
    Effect::new(move || {
        let value = model.preferences.get().scale * 100.0;
        if (scale.get_untracked() - value).abs() > 0.01 {
            scale.set(value);
        }
    });
    view! {
        scroll {
            col height:min-content pad:{px(28.0)}px gap:{px(24.0)}px {
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Harnesses"
                    text font-size:{px(12.0)}px font-color:muted
                        "Installed on the connected server. Existing harness logins and model settings are used."
                    if !model.connected.get() {
                        text font-size:{px(12.0)}px font-color:muted
                            "Disconnected · last checked status retained"
                    }
                    if !model.harness_error.get().is_empty() {
                        text font-size:{px(12.0)}px font-color:muted {model.harness_error.get()}
                    }
                    for (_, harness) in [("codex",Harness::Codex),("claude",Harness::ClaudeCode)] {
                        HarnessCard model:(model) harness:(harness)
                    }
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Appearance"
                    AppearanceSegments model:(model) field:(0usize)
                    text font-family:sans-serif font-size:{px(14.0)}px "Light palette"
                    AppearanceSegments model:(model) field:(1usize)
                    text font-family:sans-serif font-size:{px(14.0)}px "Dark palette"
                    AppearanceSegments model:(model) field:(2usize)
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650
                        "Interface scale"
                    text font-size:{px(12.0)}px font-color:muted
                        "Display scaling follows your operating system. Adjust the interface size here."
                    row height:min-content gap:{px(8.0)}px align:center {
                        stepper #scale-stepper min:80 max:200 step:10 label:"Interface scale percent" scale as scale_control
                        {scale_control.decrement().label("Decrease interface scale");scale_control.increment().label("Increase interface scale");}
                        text "%"
                    }
                    button #action @click:{model.preferences.update(|p|p.scale=1.0);}
                        label:"Reset interface scale" "Reset"
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Sidebar"
                    text font-size:{px(12.0)}px font-color:muted "Drag its right edge to resize."
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px {
                        button #action
                            @click:{model.preferences.update(|p| p.sidebar_width = (p.sidebar_width - 20.0).max(160.0));}
                            width:fill label:"Narrower sidebar" "Narrower"
                        button #action
                            @click:{model.preferences.update(|p| p.sidebar_width = (p.sidebar_width + 20.0).min(360.0));}
                            width:fill label:"Wider sidebar" "Wider"
                        button #action
                            @click:{model.preferences.update(|p| p.sidebar_width = 220.0);}
                            width:fill label:"Reset sidebar width" "Reset"
                    }
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Fonts"
                    text font-size:{px(12.0)}px "Titles: Reddit Sans · Text: Zed Mono"
                }
                text font-size:{px(12.0)}px font-color:muted
                    "Appearance is saved on this machine. Projects and harness configuration are saved on the server."
            }
        }
    }
}

fn sync_label(synced_at: Option<u64>) -> String {
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

fn scope_label(scope: &DirectorScope, snapshot: &Snapshot) -> String {
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

pub fn shell(model: Model) -> Element {
    let width = State::new(1280.0f32);
    let root = view! {
        stack fill:base font-family:monospace font-color:ink font-size:{px(14.0)}px {
            row @layout:{ move |rect: Rect| width.set(rect.size.width) } {
                Sidebar model:(model) viewport:(width)
                col width:1fr {
                    if model.page.get() != Page::Sessions {
                        row height:min-content min-height:{px(84.0)}px
                            pad:(horizontal:{px(28.0)}px vertical:{px(18.0)}px) align:center
                            justify:between shrink:0 {
                            col height:min-content gap:{px(4.0)}px {
                                text font-size:{px(22.0)}px font-weight:650 font-family:sans-serif
                                    {
                                match model.page.get() { Page::Board => "Project board", Page::Sessions => "Sessions", Page::Directors => "Directors", Page::Settings => "Settings", Page::NewProject => "New Project", Page::Connections => "Project Connections", Page::Publish => "Publish board", Page::DirectorStart => "Director conversation" }
                            }
                                text font-size:{px(12.0)}px font-color:muted font-family:sans-serif
                                    { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| p.name.clone()).unwrap_or_default() }
                            }
                            if width.get() < px(900.0) {
                                button #action @click:{ model.palette.set(true); }
                                    width:{px(36.0)}px label:"Open command palette" {
                                    icon size:{px(16.0)}px command-icon
                                }
                            } else {
                                button #action @click:{ model.palette.set(true); }
                                    label:"Open command palette" "Commands"
                            }
                        }
                    }
                    if !model.notice.get().is_empty() {
                        row height:min-content fill:accent-soft pad:{px(12.0)}px gap:{px(12.0)}px
                            align:center shrink:0 {
                            col width:1fr height:min-content gap:{px(4.0)}px {
                                text font-size:{px(12.0)}px { model.notice.get() }
                                if model.can_retry() {
                                    text font-size:{px(11.0)}px { model.retry_summary() }
                                }
                            }
                            if model.can_rebase() {
                                button #action @click:{ model.review_latest(); }
                                    disabled:{ model.busy.get() } "Review latest state"
                            }
                            if model.can_rebase() {
                                button #action @click:{ model.rebase_conflict(); }
                                    label:"Review conflict for new request"
                                    "Review conflict / new request"
                            }
                            if model.can_retry() {
                                button #action @click:{ model.retry_pending(); }
                                    label:"Retry original request" "Retry original request"
                            }
                            button #action @click:{ model.notice.set(String::new()); } "Dismiss"
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
                                    text font-color:muted { model.status.get() }
                                    text font-color:muted
                                        "Start relay-server and connect with its workspace token."
                                }
                            } else {
                                col {
                                    if model.page.get() == Page::Board {
                                        if width.get() < px(1050.0) && model.issue.get().is_some() {
                                            IssueDetail model:(model)
                                        } else {
                                            row height:1fr {
                                                Board model:(model)
                                                    narrow:{ width.get() < px(850.0) }
                                                if model.issue.get().is_some() {
                                                    IssueDetail model:(model)
                                                }
                                            }
                                        }
                                    } else if model.page.get() == Page::Sessions {
                                        Sessions model:(model)
                                    } else {
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

#[component]
fn Board(model: Model, narrow: Derived<bool>) -> Element {
    let columns = Derived::new(move || model.board_columns());
    view! {
        col width:1fr pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) gap:{px(12.0)}px {
            scroll {
                col height:min-content gap:{px(12.0)}px {
                    text font-size:{px(12.0)}px font-color:muted
                        {
                if let Some(board)=model.selected_board() {board_caption(&board) } else {
                    model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| match &p.github {
                        Some(g) => with_board_error(format!("{} · board {} #{} · {}",g.url,g.owner,g.number,sync_label(g.last_synced_at)),g.sync_error.as_deref()),
                        None => "Fixture board".into()
                    }).unwrap_or_default()
                }
            }
                    BoardActions model:(model)
                    if model.selected_board().is_some_and(|b| b.source != BoardSource::Local) || (model.selected_board().is_none() && model.snapshot.get().projects.iter().any(|p| p.id==model.project.get() && p.github.is_some()) && !model.snapshot.get().boards.iter().any(|b|b.project_id==model.project.get())) {
                        button #action @click:{ model.sync_project(); } label:"Sync project"
                            disabled:{ model.busy.get() || !model.connected.get() } "Sync project"
                    }
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
fn BoardColumnView(model: Model, column: BoardColumn) -> Element {
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
                pad:(horizontal:{px(4.0)}px vertical:{px(10.0)}px) {
                text font-size:{px(13.0)}px font-weight:650 font-family:sans-serif
                    label:{ format!("Column {}", column_id.get()) }
                    { model.board_columns().iter().find(|c| c.id == column_id.get()).map(|c| c.title.clone()).unwrap_or_default() }
                text font-size:{px(12.0)}px font-color:muted { issues.get().len().to_string() }
            }
            for (_, issue) in { issues.get().into_iter().map(|i| (i.id.clone(), i)) } {
                IssueCard model:(model) issue:(issue.clone())
            }
        }
    }
}

#[component]
fn IssueCard(model: Model, issue: Issue) -> Element {
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
    view! {
        button @click:{ model.issue.set(Some(id.clone())); model.worker_approval.set(false); }
            width:fill height:min-content fill:surface radius:{px(10.0)}px pad:{px(16.0)}px
            label:{ current.get().reference.map(|r|format!("Open issue #{}",r.number)).unwrap_or_else(||format!("Open local task {}",current.get().title)) }
            hover { fill:raised }
            focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) } {
            col height:min-content gap:{px(14.0)}px align:start {
                text font-size:{px(11.0)}px font-color:muted { current.get().label() }
                text font-size:{px(15.0)}px font-weight:600 font-family:sans-serif font-color:ink
                    label:{ current.get().title } { current.get().title }
                for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                    text font-size:{px(11.0)}px font-color:accent (label.clone())
                }
                if session_count.get() > 0 {
                    text font-size:{px(11.0)}px font-color:muted
                        { format!("{} linked sessions", session_count.get()) }
                } else {
                    text font-size:{px(11.0)}px font-color:muted "Ready to scope"
                }
            }
        }
    }
}

#[component]
fn IssueDetail(model: Model) -> Element {
    let issue = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .find(|i| Some(&i.id) == model.issue.get().as_ref())
    });
    view! {
        col width:{px(320.0)} shrink:1 fill:surface pad:{px(22.0)}px gap:{px(14.0)}px {
            row height:min-content align:center justify:between {
                text font-size:{px(12.0)}px font-color:muted "ISSUE DETAILS"
                button #action @click:{ model.issue.set(None); } label:"Close issue details" "Close"
            }
            scroll {
                for (_, detail) in { issue.get().into_iter().map(|i| (i.id.clone(), i)) } {
                    let detail_id = State::new(detail.id.clone());
                    let fallback = detail.clone();
                    let current = Derived::new(move || model.snapshot.get().issue(&detail_id.get()).cloned().unwrap_or_else(|_| fallback.clone()));
                    col height:min-content gap:{px(18.0)}px selectable {
                        text font-size:{px(12.0)}px font-color:accent (current.get().label())
                        text font-size:{px(21.0)}px font-weight:650 font-family:sans-serif
                            label:{ current.get().title } { current.get().title }
                        text font-size:{px(14.0)}px
                            label:{ crate::projects::task_body(&current.get().body) }
                            { crate::projects::task_body(&current.get().body) }
                        text font-size:{px(11.0)}px font-color:muted
                            { if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "Fixture issue · execution unavailable".to_string() } else { current.get().reference.map(|r| r.url).unwrap_or_default() } }
                        if !model.snapshot.get().visible_task(&current.get().id) {
                            text font-size:{px(12.0)}px font-color:muted
                                label:"Issue removed from board"
                                "No longer on this board · history retained. Restore and sync before starting or continuing a worker. Active turns may finish or be stopped."
                        }
                        WorkerForm model:(model) continuation:false
                        text font-size:{px(12.0)}px font-weight:650 font-family:sans-serif
                            "LINKED SESSIONS"
                        for (_, session) in { model.sessions_for_task(&detail_id.get()).into_iter().map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                            let id = State::new(session.id.clone());
                            button #action @click:{ model.open_session(id.get_untracked()); }
                                { model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                        }
                        TaskEditor model:(model)
                        if current.get().result.is_some() {
                            text font-size:{px(12.0)}px font-weight:650 font-family:sans-serif
                                "RESULT"
                            text font-size:{px(14.0)}px { current.get().result.unwrap_or_default() }
                        }
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
fn Profiles(model: Model) -> Element {
    view! {
        col width:1fr pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) gap:{px(14.0)}px {
            scroll width:max-content {
                row height:min-content width:max-content gap:{px(8.0)}px {
                    button #action @click:{ model.open_profile(EditTarget::Defaults); }
                        "Project defaults"
                    for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                        let id = State::new(director.id.clone());
                        button #action
                            @click:{ model.open_profile(EditTarget::Director(id.get_untracked())); }
                            {
                        model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| d.name.clone()).unwrap_or_default()
                    }
                    }
                    button #action @click:{ model.open_profile(EditTarget::New); }
                        label:"Create director" "+ Director"
                }
            } as tabs
            { tabs.root().style_dyn(move || Style::stack().width(Dimension::Fill).height(px(42.0)).basis(px(42.0)).shrink(0.0)); }
            scroll {
                col height:min-content gap:{px(20.0)}px {
                    text font-size:{px(20.0)}px font-weight:650 font-family:sans-serif
                        { if model.editor.get() == EditTarget::Defaults { "Project defaults" } else { "Director profile" } }
                    if model.editor.get() != EditTarget::Defaults {
                        input #input-field label:"Director name" model.editor_name
                    }
                    text font-size:{px(12.0)}px font-color:muted
                        "Effective profiles gate issue-linked worker execution."
                    ProfileField model:(model) title:"Agent harness" field:"harness"
                    row height:min-content gap:{px(8.0)}px {
                        button #action
                            @click:{ model.modify_profile("harness", |p| p.harness = Harness::Codex); }
                            fill:if model.editor_profile.get().harness == Harness::Codex { accent-soft } else { raised }
                            "Codex"
                        button #action
                            @click:{ model.modify_profile("harness", |p| p.harness = Harness::ClaudeCode); }
                            fill:if model.editor_profile.get().harness == Harness::ClaudeCode { accent-soft } else { raised }
                            "Claude Code"
                    }
                    ProfileField model:(model) title:"Execution approval" field:"execution"
                    row height:min-content gap:{px(8.0)}px {
                        for mode in ApprovalMode::ALL {
                            button #action
                                @click:{model.modify_profile("execution", |p| p.execution.approval = mode);}
                                fill:if model.editor_profile.get().execution.approval == mode {accent-soft} else {raised}
                                label:{format!("Execution approval: {}",mode.label())}
                                {mode.label()}
                        }
                    }
                    text font-size:{px(12.0)}px font-color:muted
                        "Execution mode configures the harness. Action permissions are separate workflow settings."
                    ProfileField model:(model) title:"Scope" field:"scope"
                    row height:min-content gap:{px(8.0)}px {
                        button #action
                            @click:{ model.modify_profile("scope", |p| p.scope = DirectorScope::Project); }
                            "Whole project"
                        for (_, issue) in { model.snapshot.get().issues.into_iter().filter(|i| i.project_id == model.project.get() && model.snapshot.get().canonical_issue_id(&i.id) == i.id).map(|i| (i.id.clone(), i)).collect::<Vec<_>>() } {
                            let id = issue.id.clone();
                            let issue_id = State::new(id.clone());
                            let number = issue.reference.as_ref().map(|r|format!("#{}",r.number)).unwrap_or_else(||issue.title.clone());
                            button #action
                                @click:{
                                model.modify_profile("scope", |p| {
                                    let mut ids = match &p.scope { DirectorScope::Issues { issue_ids } => issue_ids.clone(), _ => vec![] };
                                    if model.scope_contains(&ids, &id) { let snapshot = model.snapshot.get_untracked(); ids.retain(|x| snapshot.canonical_issue_id(x) != snapshot.canonical_issue_id(&id)); } else { ids.push(id.clone()); }
                                    p.scope = if ids.is_empty() { DirectorScope::Project } else { DirectorScope::Issues { issue_ids: ids } };
                                });
                            }
                                { format!("{} {}", if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if model.scope_contains(issue_ids, &issue_id.get())) { "✓" } else { "+" }, number) }
                        }
                    }
                    text font-size:{px(12.0)}px font-color:muted
                        {
                        match model.editor_profile.get().scope {
                            DirectorScope::Project => "All project issues".into(),
                            DirectorScope::Issues { issue_ids } => issue_ids.iter()
                                .filter_map(|id| model.snapshot.get().issue(id).ok().cloned())
                                .map(|issue|issue.reference.as_ref().map(|r|format!("#{}",r.number)).unwrap_or_else(||issue.title.clone()))
                                .collect::<Vec<_>>().join(", "),
                        }
                    }
                    ProfileField model:(model) title:"Responsibilities" field:"responsibilities"
                    StepChoices model:(model) completion:false
                    ProfileField model:(model) title:"Required for completion" field:"completion"
                    StepChoices model:(model) completion:true
                    ProfileField model:(model) title:"Concurrent workers" field:"max_workers"
                    row height:min-content gap:{px(10.0)}px align:center {
                        button #action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = p.max_workers.saturating_sub(1)); }
                            label:"Decrease worker limit" "−"
                        text font-size:{px(18.0)}px
                            { model.editor_profile.get().max_workers.to_string() }
                        button #action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = (p.max_workers + 1).min(64)); }
                            label:"Increase worker limit" "+"
                        text font-size:{px(12.0)}px font-color:muted "0 pauses delegation"
                    }
                    ProfileField model:(model) title:"Action permissions" field:"permissions"
                    col height:min-content gap:{px(8.0)}px {
                        for task in Task::ALL {
                            row height:min-content align:center justify:between {
                                text font-size:{px(13.0)}px (task.label())
                                button #action
                                    @click:{ model.modify_profile("permissions", |p| { let current = p.permissions[&task]; p.permissions.insert(task, current.next()); }); }
                                    label:{ format!("{} permission", task.label()) }
                                    { model.editor_profile.get().permissions[&task].label() }
                            }
                        }
                    }
                    row height:min-content gap:{px(8.0)}px {
                        button #action @click:{ model.export_toml(); } "Export / edit TOML"
                        button #action
                            @click:{ model.toml.set(model.editor_profile.get_untracked().to_toml()); model.advanced.set(true); }
                            "Show effective profile"
                    }
                    if model.advanced.get() {
                        col height:min-content gap:{px(10.0)}px {
                            text font-size:{px(12.0)}px font-color:muted
                                "Project defaults use a complete profile. Directors use overrides; omitted fields inherit. Copy this text to export."
                            input #area multiline height:{px(240.0)}px label:"Profile TOML"
                                model.toml
                            button #action @click:{ model.import_toml(); } "Import TOML into draft"
                        }
                    }
                }
            }
            row height:min-content gap:{px(12.0)}px align:center shrink:0 {
                button #action @click:{ model.save_profile(); }
                    disabled:{ model.busy.get() || !model.connected.get() } label:"Save profile"
                    "Save profile"
                text font-size:{px(12.0)}px font-color:muted
                    "Explicit overrides survive project default changes"
            }
        }
    }
}

#[component]
fn ProfileField(model: Model, title: &'static str, field: &'static str) -> Element {
    view! {
        row height:min-content justify:between align:center {
            col height:min-content gap:{px(4.0)}px {
                text font-size:{px(13.0)}px font-weight:650 font-family:sans-serif (title)
                text font-size:{px(11.0)}px font-color:muted { model.origin(field) }
            }
            if model.origin(field) == "Director override" {
                button #action @click:{ model.inherit(field); } label:{ format!("Inherit {field}") }
                    "Inherit"
            }
        }
    }
}

#[component]
fn StepChoices(model: Model, completion: bool) -> Element {
    view! {
        grid cols:(1fr 1fr 1fr 1fr) height:min-content gap:{px(6.0)}px {
            for task in Task::ALL {
                button #action
                    @click:{ model.modify_profile(if completion { "completion" } else { "responsibilities" }, |p| {
                    let steps = if completion { &mut p.completion } else { &mut p.responsibilities };
                    if steps.contains(&task) { steps.retain(|t| t != &task); } else { steps.push(task); }
                }); }
                    width:fill {
                    text font-size:{px(12.0)}px
                        { format!("{} {}", if (if completion { model.editor_profile.get().completion } else { model.editor_profile.get().responsibilities }).contains(&task) { "✓" } else { "+" }, task.label()) }
                }
            }
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
        col fill:#00000055 align:center pad:(horizontal:{px(30.0)}px vertical:{px(90.0)}px) {
            col height:min-content width:{px(560.0)} max-width:100% fill:surface radius:{px(14.0)}px
                pad:{px(18.0)}px gap:{px(12.0)}px {
                row height:min-content justify:between align:center {
                    text font-size:{px(15.0)}px font-weight:650 font-family:sans-serif
                        "Command palette"
                    button #action @click:{ model.palette.set(false); } "Esc"
                }
                input #input-field placeholder:"Find an action…" label:"Command search" query
                    as command_search
                { command_search.focus(); }
                for (label, index) in [("Open board", 0), ("Open sessions", 1), ("Edit project defaults", 2), ("Create director", 3), ("Search transcript", 4), ("Sync project", 5), ("Stop worker", 6), ("Open settings", 7), ("Project Connections", 8), ("New Project", 9), ("Publish board", 10)] {
                    if label.to_lowercase().contains(&query.get().to_lowercase()) {
                        button #action
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
        _ => {
            model.page.set(Page::Sessions);
            model.searching.set(true);
        }
    }
    model.palette.set(false);
    model.palette_query.set(String::new());
}

#[component]
fn WorkerApproval(model: Model) -> Element {
    let root = view! {
        col height:min-content {}
    };
    root.switch(
        move || {
            use mosaic::core::theme::{color, scalar};
            (
                scalar(ui_scale),
                color(surface),
                color(accent),
                color(edge),
                color(base),
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
                    radius: 5.0 * scale,
                    label: TextStyle::inherited(),
                    gap: 8.0 * scale,
                },
            );
        },
    );
    root
}

#[component]
fn WorkerForm(model: Model, continuation: bool) -> Element {
    view! {
        col height:min-content gap:{px(10.0)}px {
            text font-size:{px(13.0)}px font-weight:650 font-family:sans-serif
                { if continuation { "Continue this session" } else { "Start task worker" } }
            if !continuation {
                WorkspaceChoices model:(model)
                for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                    let id = State::new(director.id.clone());
                    button #action
                        @click:{ model.worker_director.set(id.get_untracked()); model.worker_approval.set(false); }
                        fill:if model.worker_director.get() == id.get() { accent-soft } else { raised }
                        { model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| format!("Director: {}", d.name)).unwrap_or_default() }
                }
            }
            text font-size:{px(12.0)}px font-color:muted
                {
                match model.worker_profile(continuation) {
                    Ok((p, active)) => format!("{} · Implementation: {} · {} · {} / {} workers active", match p.harness { Harness::Codex => "Codex", Harness::ClaudeCode => "Claude Code" }, p.permissions.get(&Task::Implement).copied().unwrap_or(Permission::Deny).label(), scope_label(&p.scope, &model.snapshot.get()), active, p.max_workers),
                    Err(error) => error
                }
            }
            input #input-field label:"Worker prompt" placeholder:"Prompt for this turn…"
                model.worker_prompt
            if model.worker_profile(continuation).is_ok_and(|(p, _)| p.permissions.get(&Task::Implement) == Some(&Permission::Ask)) {
                WorkerApproval model:(model)
            }
            text font-size:{px(11.0)}px font-color:muted
                { model.worker_gate(continuation).err().unwrap_or_else(|| "Ready · server rechecks policy and revision".into()) }
            button #action @click:{ model.run_worker(continuation); }
                label:if continuation { "Send worker prompt" } else { "Start worker" }
                disabled:{ model.worker_gate(continuation).is_err() }
                { if continuation { "Send / continue" } else { "Start worker" } }
        }
    }
}

#[component]
fn HarnessCard(model: Model, harness: Harness) -> Element {
    let status = Derived::new(move || {
        model
            .harnesses
            .get()
            .into_iter()
            .find(|s| s.harness == harness)
    });
    let advanced = State::new(false);
    let executable = State::new(String::new());
    let name = match harness {
        Harness::Codex => "Codex",
        Harness::ClaudeCode => "Claude Code",
    };
    let width = State::new(0.0f32);
    view! {
        col height:min-content max-width:{px(720.0)}px gap:{px(8.0)}px
            @layout:{move |rect:Rect|width.set(rect.size.width)} {
            if width.get() < px(520.0) {
                col height:min-content gap:{px(6.0)}px {
                    HarnessSummary model:(model) harness:(harness)
                    HarnessActions model:(model) harness:(harness) advanced:(advanced)
                        executable:(executable)
                }
            } else {
                row height:min-content align:center gap:{px(12.0)}px {
                    HarnessSummary model:(model) harness:(harness)
                    HarnessActions model:(model) harness:(harness) advanced:(advanced)
                        executable:(executable)
                }
            }
            if advanced.get() {
                col height:min-content gap:{px(8.0)}px {
                    text font-size:{px(12.0)}px font-color:muted
                        {status.get().map(|s|s.detail).unwrap_or_default()}
                    text font-size:{px(12.0)}px font-color:muted
                        {status.get().map(|s|format!("{} · {}",s.version.unwrap_or_else(||"Version unavailable".into()),sync_label(Some(s.checked_at)).replacen("Last synced", "Checked", 1))).unwrap_or_default()}
                    text font-size:{px(12.0)}px font-color:muted
                        "Automatic · Ask · Unrestricted Access. Mode availability also depends on the harness account and managed settings."
                    input #input-field label:{format!("{name} executable on server")} executable
                    button #action
                        @click:{model.submit(Command::ConfigureHarness{harness,executable:executable.get_untracked().trim().into()},model.snapshot.get_untracked().revision,crate::model::Saved::Action);}
                        disabled:{!model.connected.get() || model.busy.get()}
                        label:{format!("Save {name} executable and check status")} "Save and check"
                }
            }
        }
    }
}

#[component]
fn HarnessSummary(model: Model, harness: Harness) -> Element {
    let status = Derived::new(move || {
        model
            .harnesses
            .get()
            .into_iter()
            .find(|s| s.harness == harness)
    });
    let state = Derived::new(move || {
        if !model.connected.get() || !model.harness_error.get().is_empty() {
            String::new()
        } else {
            status.get().map(|s| s.state).unwrap_or_default()
        }
    });
    view! {
        row height:min-content width:1fr align:center gap:{px(8.0)}px {
            icon size:{px(20.0)}px shrink:0
                {if harness==Harness::Codex{harness_codex}else{harness_claude}}
            text font-family:sans-serif font-size:{px(14.0)}px font-weight:650
                {if harness==Harness::Codex{"Codex"}else{"Claude Code"}}
            icon size:{px(14.0)}px shrink:0
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>success,"signed_out"|"incompatible"=>warning,"unknown"|"error"|"failed"=>danger,_=>muted})}
                {match state.get().as_str(){"ready"=>harness_ready,"signed_out"|"incompatible"=>harness_warning,"unknown"|"error"|"failed"=>harness_failed,_=>harness_neutral}}
            text font-size:{px(12.0)}px
                font-color:{mosaic::core::theme::color(match state.get().as_str(){"ready"=>success,"signed_out"|"incompatible"=>warning,"unknown"|"error"|"failed"=>danger,_=>muted})}
                {if !model.connected.get(){"Disconnected".into()}else if !model.harness_error.get().is_empty(){format!("Refresh failed · last check: {}",status.get().map(|s|match s.state.as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible",_=>"Check failed"}).unwrap_or("Unavailable"))}else{match state.get().as_str(){"ready"=>"Ready","signed_out"=>"Sign in","missing"=>"Missing","incompatible"=>"Incompatible","unknown"|"error"|"failed"=>"Check failed",""=>"Checking",_=>"Check unavailable"}.into()}}
        }
    }
}
#[component]
fn HarnessActions(
    model: Model,
    harness: Harness,
    advanced: State<bool>,
    executable: State<String>,
) -> Element {
    let name = if harness == Harness::Codex {
        "Codex"
    } else {
        "Claude Code"
    };
    view! {
        row height:min-content width:min-content gap:{px(6.0)}px {
            button #action
                @click:{if let Some(sender)=model.harness_refresh.get_untracked(){let _=sender.send(());}}
                disabled:{!model.connected.get()} label:{format!("Refresh {name} status")} "Refresh"
            button #action
                @click:{if let Some(status)=model.harnesses.get_untracked().iter().find(|s|s.harness==harness){executable.set(status.executable.clone());}advanced.set(!advanced.get_untracked());}
                label:{format!("{name} executable and status details")} "Details"
        }
    }
}

fn with_board_error(mut caption: String, error: Option<&str>) -> String {
    if let Some(error) = error.filter(|e| !e.trim().is_empty()) {
        caption.push('\n');
        caption.push_str(error);
    }
    caption
}
pub(crate) fn board_caption(board: &Board) -> String {
    let caption = match &board.source {
        BoardSource::Local if board.name == "Local board" => board.name.clone(),
        BoardSource::Local => format!("{} · Local board", board.name),
        BoardSource::Github { owner, number, .. } => format!(
            "{} · GitHub {owner} · {number} · {}",
            board.name,
            sync_label(board.last_synced_at)
        ),
        BoardSource::Gitlab {
            host,
            path,
            number,
            group,
            ..
        } => format!(
            "{} · GitLab {host}/{path} · {} · {number} · {}",
            board.name,
            if *group { "group" } else { "project" },
            sync_label(board.last_synced_at)
        ),
    };
    with_board_error(caption, board.error.as_deref())
}
