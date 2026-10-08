use crate::{
    controls::{ButtonStyle, button},
    conversation::{Conversation, ConversationProps},
    model::{EditTarget, Model, Page},
    sidebar::{Sidebar, SidebarProps},
    theme::*,
};
use mosaic::prelude::*;
use relay_core::*;

#[component]
fn Settings(model: Model) -> Element {
    use crate::settings::ThemeMode;
    let checkout = State::new(String::new());
    let repository = State::new(String::new());
    let owner = State::new(String::new());
    let number = State::new(String::new());
    let setup = State::new(
        !model
            .snapshot
            .get_untracked()
            .projects
            .iter()
            .any(|p| !p.fixture),
    );
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
                    row height:min-content align:center gap:{px(10.0)}px {
                        text font-family:sans-serif font-size:{px(18.0)}px font-weight:650
                            "Projects"
                        button #action @click:{setup.set(!setup.get_untracked());}
                            label:"Add project" "+ Project"
                    }
                    for (_, binding) in {model.snapshot.get().bindings.into_iter().map(|b|(b.project_id(),b)).collect::<Vec<_>>()} {
                        let id = State::new(binding.project_id());
                        button #action @click:{model.select_project(id.get_untracked());}
                            {model.snapshot.get().bindings.iter().find(|b|b.project_id()==id.get()).map(|b|format!("{} · board {}",b.repository,b.number)).unwrap_or_default()}
                    }
                    if setup.get() {
                        col max-width:{px(600.0)}px height:min-content gap:{px(8.0)}px {
                            text font-size:{px(12.0)}px font-color:muted
                                "Connect a GitHub board. The checkout path is on the server; project setup and sessions are saved there."
                            input #input-field label:"Checkout path on server"
                                placeholder:"/home/you/repos/project" checkout
                            input #input-field label:"GitHub repository"
                                placeholder:"owner/repository" repository
                            input #input-field label:"GitHub project owner"
                                placeholder:"user or organization" owner
                            input #input-field label:"GitHub board number" placeholder:"5" number
                            button #action
                                @click:{
                                    if let Ok(number) = number.get_untracked().trim().parse::<u64>() {
                                        let binding = ProjectBinding { checkout: checkout.get_untracked().trim().into(), repository: repository.get_untracked().trim().into(), owner: owner.get_untracked().trim().into(), number };
                                        model.submit(Command::ConfigureProject {binding:binding.clone()},model.snapshot.get_untracked().revision,crate::model::Saved::Project(binding.project_id()));
                                    } else {model.notice.set("Enter a positive board number".into());}
                                }
                                disabled:{!model.connected.get() || model.busy.get()}
                                label:"Save project and sync board" "Connect project"
                        }
                    }
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650 "Appearance"
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px {
                        for (label, mode) in [("Dark", ThemeMode::Dark), ("Light", ThemeMode::Light), ("System", ThemeMode::System)] {
                            button #action @click:{ model.preferences.update(|p| p.mode = mode); }
                                width:fill
                                fill:if model.preferences.get().mode == mode { accent-soft } else { raised }
                                label:{format!("Theme: {label}")} (label)
                        }
                    }
                    text font-family:sans-serif font-size:{px(14.0)}px "Light palette"
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px {
                        for (label, warm) in [("Paper", false), ("Warm", true)] {
                            button #action
                                @click:{ model.preferences.update(|p| p.light_warm = warm); }
                                width:fill
                                fill:if model.preferences.get().light_warm == warm { accent-soft } else { raised }
                                label:{format!("Light palette: {label}")} (label)
                        }
                    }
                    text font-family:sans-serif font-size:{px(14.0)}px "Dark palette"
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px {
                        for (label, neutral) in [("Slate", false), ("Neutral", true)] {
                            button #action
                                @click:{ model.preferences.update(|p| p.dark_neutral = neutral); }
                                width:fill
                                fill:if model.preferences.get().dark_neutral == neutral { accent-soft } else { raised }
                                label:{format!("Dark palette: {label}")} (label)
                        }
                    }
                }
                col height:min-content gap:{px(10.0)}px {
                    text font-family:sans-serif font-size:{px(18.0)}px font-weight:650
                        "Interface scale"
                    text font-size:{px(12.0)}px font-color:muted
                        "Display scaling follows your operating system. Adjust the interface size here."
                    grid
                        cols:{GridTracks::auto_fit(GridTrack::minmax(px(100.0).into(), GridTrack::fr(1.0)))}
                        max-width:{px(480.0)}px height:min-content gap:{px(8.0)}px align:center {
                        button #action
                            @click:{ model.preferences.update(|p| p.scale = (p.scale - 0.1).max(0.8)); }
                            width:fill label:"Decrease interface scale"
                            disabled:{model.preferences.get().scale <= 0.8} "−"
                        text {format!("{:.0}%", model.preferences.get().scale * 100.0)}
                        button #action
                            @click:{ model.preferences.update(|p| p.scale = (p.scale + 0.1).min(2.0)); }
                            width:fill label:"Increase interface scale"
                            disabled:{model.preferences.get().scale >= 2.0} "+"
                        button #action @click:{model.preferences.update(|p| p.scale = 1.0);}
                            width:fill label:"Reset interface scale" "Reset"
                    }
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
                        .issues
                        .iter()
                        .find(|i| &i.id == id)
                        .map(|i| format!("#{}", i.reference.number))
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
                Sidebar model:(model)
                col width:1fr {
                    if model.page.get() != Page::Sessions {
                        row height:min-content min-height:{px(84.0)}px
                            pad:(horizontal:{px(28.0)}px vertical:{px(18.0)}px) align:center
                            justify:between shrink:0 {
                            col height:min-content gap:{px(4.0)}px {
                                text font-size:{px(22.0)}px font-weight:650 font-family:sans-serif
                                    {
                                match model.page.get() { Page::Board => "Project board", Page::Sessions => "Sessions", Page::Directors => "Directors", Page::Settings => "Settings" }
                            }
                                text font-size:{px(12.0)}px font-color:muted font-family:sans-serif
                                    { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| format!("{} · {}", p.repository, if p.fixture { "Fixture workspace" } else { "Remote GitHub board" })).unwrap_or_default() }
                            }
                            button #action @click:{ model.palette.set(true); }
                                label:"Open command palette" "Commands"
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
                            button #action @click:{ model.review_latest(); }
                                disabled:{ model.busy.get() } "Review latest state"
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
    let columns = Derived::new(move || {
        model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == model.project.get())
            .map(|p| {
                p.columns
                    .iter()
                    .filter(|c| c.id != "github-removed-from-board")
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    view! {
        col width:1fr pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px) gap:{px(12.0)}px {
            text font-size:{px(12.0)}px font-color:muted
                {
                model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| match &p.github {
                    Some(g) => format!("{} · board {} #{} · {}\n{}", g.url, g.owner, g.number, sync_label(g.last_synced_at), g.sync_error.clone().unwrap_or_default()),
                    None => "Fixture board · no remote synchronization".into()
                }).unwrap_or_default()
            }
            button #action @click:{ model.sync_project(); } label:"Sync project"
                disabled:{ model.busy.get() || !model.connected.get() || !model.snapshot.get().projects.iter().any(|p| p.id == model.project.get() && p.github.is_some()) }
                "Sync project"
            scroll {
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
            .filter(|i| i.project_id == model.project.get() && i.column_id == id)
            .collect::<Vec<_>>()
    });
    view! {
        col height:min-content width:1fr gap:{px(12.0)}px {
            row height:min-content justify:between align:center
                pad:(horizontal:{px(4.0)}px vertical:{px(10.0)}px) {
                text font-size:{px(13.0)}px font-weight:650 font-family:sans-serif
                    label:{ format!("Column {}", column_id.get()) }
                    { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).and_then(|p| p.columns.iter().find(|c| c.id == column_id.get())).map(|c| c.title.clone()).unwrap_or_default() }
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
    let session_count = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .iter()
            .filter(|s| s.issue_id.as_ref() == Some(&count_id))
            .count()
    });
    view! {
        button @click:{ model.issue.set(Some(id.clone())); model.worker_approval.set(false); }
            width:fill height:min-content fill:surface radius:{px(10.0)}px pad:{px(16.0)}px
            label:{ format!("Open issue #{}", current.get().reference.number) }
            hover { fill:raised }
            focused { stroke:(width:{px(2.0)} color:accent offset:{px(2.0)}) } {
            col height:min-content gap:{px(14.0)}px align:start {
                text font-size:{px(11.0)}px font-color:muted
                    { format!("#{} · {}", current.get().reference.number, if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "FIXTURE" } else { "GITHUB" }) }
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
                    let current = Derived::new(move || model.snapshot.get().issues.into_iter().find(|i| i.id == detail_id.get()).unwrap_or_else(|| fallback.clone()));
                    col height:min-content gap:{px(18.0)}px selectable {
                        text font-size:{px(12.0)}px font-color:accent
                            (format!("{} #{}", current.get().reference.repository, current.get().reference.number))
                        text font-size:{px(21.0)}px font-weight:650 font-family:sans-serif
                            label:{ current.get().title } { current.get().title }
                        text font-size:{px(14.0)}px label:{ current.get().body }
                            { current.get().body }
                        text font-size:{px(11.0)}px font-color:muted
                            { if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "Fixture issue · execution unavailable".to_string() } else { current.get().reference.url } }
                        if model.snapshot.get().projects.iter().find(|p| p.id == current.get().project_id).is_some_and(|p| !p.columns.iter().any(|c| c.id == current.get().column_id)) {
                            text font-size:{px(12.0)}px font-color:muted
                                label:"Issue removed from board"
                                "No longer on this board · history retained. Restore and sync before starting or continuing a worker. Active turns may finish or be stopped."
                        }
                        WorkerForm model:(model) continuation:false
                        text font-size:{px(12.0)}px font-weight:650 font-family:sans-serif
                            "LINKED SESSIONS"
                        for (_, session) in { model.snapshot.get().sessions.into_iter().filter(|s| s.issue_id.as_ref() == Some(&detail_id.get())).map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                            let id = State::new(session.id.clone());
                            button #action @click:{ model.open_session(id.get_untracked()); }
                                { model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                        }
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
                        for (_, issue) in { model.snapshot.get().issues.into_iter().filter(|i| i.project_id == model.project.get()).map(|i| (i.id.clone(), i)).collect::<Vec<_>>() } {
                            let id = issue.id.clone();
                            let issue_id = State::new(id.clone());
                            let number = issue.reference.number;
                            button #action
                                @click:{
                                model.modify_profile("scope", |p| {
                                    let mut ids = match &p.scope { DirectorScope::Issues { issue_ids } => issue_ids.clone(), _ => vec![] };
                                    if ids.contains(&id) { ids.retain(|x| x != &id); } else { ids.push(id.clone()); }
                                    p.scope = if ids.is_empty() { DirectorScope::Project } else { DirectorScope::Issues { issue_ids: ids } };
                                });
                            }
                                { format!("{} #{}", if matches!(&model.editor_profile.get().scope, DirectorScope::Issues { issue_ids } if issue_ids.contains(&issue_id.get())) { "✓" } else { "+" }, number) }
                        }
                    }
                    text font-size:{px(12.0)}px font-color:muted
                        {
                        match model.editor_profile.get().scope {
                            DirectorScope::Project => "All project issues".into(),
                            DirectorScope::Issues { issue_ids } => model.snapshot.get().issues.iter()
                                .filter(|issue| issue_ids.contains(&issue.id))
                                .map(|issue| format!("#{}", issue.reference.number))
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
                for (label, index) in [("Open board", 0), ("Open sessions", 1), ("Edit project defaults", 2), ("Create director", 3), ("Search transcript", 4), ("Sync project", 5), ("Stop worker", 6), ("Open settings", 7)] {
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
                { if continuation { "Continue this session" } else { "Start issue worker" } }
            if !continuation {
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
    view! {
        col height:min-content max-width:{px(720.0)}px gap:{px(8.0)}px {
            row height:min-content gap:{px(12.0)}px align:center {
                text font-family:sans-serif font-size:{px(14.0)}px font-weight:650 (name)
                text font-size:{px(12.0)}px
                    {if !model.connected.get() {"Disconnected".to_owned()} else {status.get().map(|s|match s.state.as_str(){"ready"=>"Ready","signed_out"=>"Signed out","missing"=>"Not installed","incompatible"=>"Incompatible",_=>"Check unavailable"}.to_owned()).unwrap_or_else(||"Checking…".to_owned())}}
                button #action
                    @click:{if let Some(sender)=model.harness_refresh.get_untracked(){let _=sender.send(());}}
                    disabled:{!model.connected.get()} label:{format!("Refresh {name} status")}
                    "Check again"
                button #action
                    @click:{
                    if let Some(status) = status.get_untracked() {executable.set(status.executable);}
                    advanced.set(!advanced.get_untracked());
                }
                    label:{format!("{name} executable and status details")} "Details"
            }
            if status.get().is_some() {
                text font-size:{px(12.0)}px font-color:muted
                    {status.get().map(|s|s.detail).unwrap_or_default()}
            }
            if advanced.get() {
                col height:min-content gap:{px(8.0)}px {
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
