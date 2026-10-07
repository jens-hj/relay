use crate::{
    model::{EditTarget, Model, Page},
    theme::*,
};
use mosaic::prelude::*;
use relay_core::*;

pub fn shell(model: Model) -> Element {
    let width = State::new(1280.0f32);
    let root = view! {
        stack fill:base font-family:sans-serif font-color:ink font-size:14px {
            row @layout:{ move |rect: Rect| width.set(rect.size.width) } {
                Sidebar model:(model) narrow:{ width.get() < 900.0 }
                col width:1fr {
                    row height:84px pad:(horizontal:28px vertical:18px) align:center justify:between
                        shrink:0 {
                        col height:min-content gap:4px {
                            text font-size:22px font-weight:650
                                {
                                match model.page.get() { Page::Board => "Project board", Page::Sessions => "Sessions", Page::Directors => "Directors" }
                            }
                            text font-size:12px font-color:muted
                                { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| format!("{} · {}", p.repository, if p.fixture { "Fixture workspace" } else { "Remote GitHub board" })).unwrap_or_default() }
                        }
                        button #action @click:{ model.palette.set(true); }
                            label:"Open command palette" "Commands  ⌘ / Ctrl K"
                    }
                    if !model.notice.get().is_empty() {
                        row height:min-content fill:accent-soft pad:12px gap:12px align:center
                            shrink:0 {
                            text width:1fr font-size:12px { model.notice.get() }
                            button #action @click:{ model.review_latest(); }
                                disabled:{ model.busy.get() } "Review latest state"
                            if model.can_retry() {
                                button #action @click:{ model.retry_pending(); } label:"Retry unchanged request" "Retry unchanged request"
                            }
                            button #action @click:{ model.notice.set(String::new()); } "Dismiss"
                        }
                    }
                    if model.snapshot.get().projects.is_empty() {
                        col height:min-content pad:32px gap:12px {
                            text font-size:18px "Waiting for your workspace"
                            text font-color:muted { model.status.get() }
                            text font-color:muted
                                "Start relay-server and connect with its workspace token."
                        }
                    } else {
                        if model.page.get() == Page::Board {
                            if width.get() < 1050.0 && model.issue.get().is_some() {
                                IssueDetail model:(model)
                            } else {
                                row height:1fr {
                                    Board model:(model) narrow:{ width.get() < 850.0 }
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
fn Sidebar(model: Model, narrow: Derived<bool>) -> Element {
    view! {
        col width:if narrow.get() { 154px } else { 204px } fill:sidebar pad:18px gap:12px shrink:0 {
            text font-size:26px font-weight:750 font-color:accent "relay"
            text font-size:11px font-color:muted "YOUR TEAM, IN MOTION"
            el height:18px {}
            text font-size:11px font-weight:650 font-color:muted "PROJECTS"
            for (_, project) in { model.snapshot.get().projects.into_iter().map(|p| (p.id.clone(), p)) } {
                let id = State::new(project.id.clone());
                button #action @click:{ model.select_project(id.get_untracked()); }
                    label:{ model.snapshot.get().projects.iter().find(|p| p.id == id.get()).map(|p| format!("Open {}", p.name)).unwrap_or_default() } { model.snapshot.get().projects.iter().find(|p| p.id == id.get()).map(|p| p.name.clone()).unwrap_or_default() }
            }
            el height:8px {}
            for (label, page) in [("Board", Page::Board), ("Sessions", Page::Sessions), ("Directors", Page::Directors)] {
                button #action
                    @click:{
                        if page == Page::Directors { model.open_profile(EditTarget::Defaults); }
                        else { model.page.set(page); }
                    }
                    fill:if model.page.get() == page { accent-soft } else { sidebar }
                    label:(label.to_string()) (label)
            }
            el height:1fr {}
            col height:min-content gap:6px shrink:0 {
                text font-size:12px
                    font-color:if model.connected.get() { { mosaic::core::theme::color(accent) } } else { { mosaic::core::theme::color(danger) } }
                    { model.status.get() }
                text font-size:11px font-color:muted "Shared workspace"

            }
        }
    }
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
            .map(|p| p.columns.clone())
            .unwrap_or_default()
    });
    view! {
        col width:1fr pad:(horizontal:24px vertical:8px) gap:12px {
            text font-size:12px font-color:muted {
                model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).map(|p| match &p.github {
                    Some(g) => format!("{} · board {} #{} · last sync {}\n{}", g.url, g.owner, g.number, g.last_synced_at.map(|t| format!("{t} (Unix seconds)")).unwrap_or_else(|| "never".into()), g.sync_error.clone().unwrap_or_default()),
                    None => "Fixture board · no remote synchronization".into()
                }).unwrap_or_default()
            }
            button #action @click:{ model.sync_project(); } label:"Sync project"
                disabled:{ model.busy.get() || !model.connected.get() || !model.snapshot.get().projects.iter().any(|p| p.id == model.project.get() && p.github.is_some()) } "Sync project"
            scroll {
                if narrow.get() {
                    col height:min-content gap:18px {
                        for (_, column) in { columns.get().into_iter().map(|c| (c.id.clone(), c)) } {
                            col width:1fr height:min-content {
                                BoardColumnView model:(model) column:(column.clone())
                            }
                        }
                    }
                } else {
                    row height:min-content gap:16px align:start {
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
        col height:min-content width:1fr gap:12px {
            row height:min-content justify:between align:center pad:(horizontal:4px vertical:10px) {
                text font-size:13px font-weight:650 label:{ format!("Column {}", column_id.get()) } { model.snapshot.get().projects.iter().find(|p| p.id == model.project.get()).and_then(|p| p.columns.iter().find(|c| c.id == column_id.get())).map(|c| c.title.clone()).unwrap_or_default() }
                text font-size:12px font-color:muted { issues.get().len().to_string() }
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
        button @click:{ model.issue.set(Some(id.clone())); model.worker_approval.set(false); } width:fill height:min-content
            fill:surface radius:10px pad:16px
            label:{ format!("Open issue #{}", current.get().reference.number) } hover { fill:raised }
            focused { stroke:(width:2px color:accent offset:2px) } {
            col height:min-content gap:14px align:start {
                text font-size:11px font-color:muted
                    { format!("#{} · {}", current.get().reference.number, if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "FIXTURE" } else { "GITHUB" }) }
                text font-size:15px font-weight:600 font-color:ink label:{ current.get().title } { current.get().title }
                for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                    text font-size:11px font-color:accent (label.clone())
                }
                if session_count.get() > 0 {
                    text font-size:11px font-color:muted
                        { format!("{} linked sessions", session_count.get()) }
                } else {
                    text font-size:11px font-color:muted "Ready to scope"
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
        col width:320px shrink:1 fill:surface pad:22px gap:14px {
            row height:min-content align:center justify:between {
                text font-size:12px font-color:muted "ISSUE DETAILS"
                button #action @click:{ model.issue.set(None); } label:"Close issue details" "Close"
            }
            scroll {
                for (_, detail) in { issue.get().into_iter().map(|i| (i.id.clone(), i)) } {
                    let detail_id = State::new(detail.id.clone());
                    let fallback = detail.clone();
                    let current = Derived::new(move || model.snapshot.get().issues.into_iter().find(|i| i.id == detail_id.get()).unwrap_or_else(|| fallback.clone()));
                    col height:min-content gap:18px selectable {
                        text font-size:12px font-color:accent
                            (format!("{} #{}", current.get().reference.repository, current.get().reference.number))
                        text font-size:21px font-weight:650 label:{ current.get().title } { current.get().title }
                        text font-size:14px label:{ current.get().body } { current.get().body }
                        text font-size:11px font-color:muted
                            { if model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture) { "Fixture issue · execution unavailable".to_string() } else { current.get().reference.url } }
                        WorkerForm model:(model) continuation:false
                        text font-size:12px font-weight:650 "LINKED SESSIONS"
                        for (_, session) in { model.snapshot.get().sessions.into_iter().filter(|s| s.issue_id.as_ref() == Some(&detail_id.get())).map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                            let id = State::new(session.id.clone());
                            button #action @click:{ model.open_session(id.get_untracked()); }
                                { model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                        }
                        if current.get().result.is_some() {
                            text font-size:12px font-weight:650 "RESULT"
                            text font-size:14px { current.get().result.unwrap_or_default() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Sessions(model: Model) -> Element {
    let sessions = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .into_iter()
            .filter(|s| s.project_id == model.project.get())
            .collect::<Vec<_>>()
    });
    view! {
        col width:1fr pad:(horizontal:24px vertical:8px) gap:12px {
            row height:min-content gap:8px shrink:0 {
                for (_, session) in { sessions.get().into_iter().map(|s| (s.id.clone(), s)) } {
                    let id = State::new(session.id.clone());
                    button #action @click:{ model.open_session(id.get_untracked()); }
                        fill:if model.session.get() == id.get() { accent-soft } else { raised }
                        { model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default() }
                }
            }
            row height:min-content gap:8px align:center shrink:0 {
                button #action
                    @click:{
                    let snapshot = model.snapshot.get_untracked();
                    if let Some(session) = snapshot.sessions.iter().find(|s| s.id == model.session.get_untracked()) {
                        model.issue.set(session.issue_id.clone()); model.page.set(Page::Board);
                    }
                }
                    "Linked issue"
                button #action
                    @click:{
                    let snapshot = model.snapshot.get_untracked();
                    if let Some(session) = snapshot.sessions.iter().find(|s| s.id == model.session.get_untracked()) {
                        model.open_profile(EditTarget::Director(session.director_id.clone()));
                    }
                }
                    "Director profile"
                text font-size:12px font-color:muted
                    {
                    let snapshot = model.snapshot.get();
                    snapshot.sessions.iter().find(|s| s.id == model.session.get()).map(|s| {
                        let issue = snapshot.issues.iter().find(|i| Some(&i.id) == s.issue_id.as_ref());
                        format!("{} · {:?} session", issue.map(|i| format!("Issue #{}", i.reference.number)).unwrap_or_else(|| "Exploration".into()), s.role)
                    }).unwrap_or_default()
                }
            }
            row height:min-content align:center justify:between shrink:0 {
                text font-size:12px font-color:muted
                    { if model.snapshot.get().sessions.iter().any(|s| s.id == model.session.get() && s.fixture) { "Fixture transcript · Ctrl/Cmd+Enter to comment" } else { "Immutable worker messages and tool results · Ctrl/Cmd+Enter to comment" } }
                button #action @click:{ model.searching.set(!model.searching.get_untracked()); }
                    label:"Search transcript" "Find  ⌘ / Ctrl F"
            }
            if model.searching.get() {
                input #input-field placeholder:"Find messages…" model.search as search_field
                { search_field.focus(); }
            }
            scroll {
                col height:min-content gap:14px {
                    for (_, message) in { model.snapshot.get().messages.into_iter().filter(|m| m.session_id == model.session.get() && m.body.to_lowercase().contains(&model.search.get().to_lowercase())).map(|m| (m.id.clone(), m)).collect::<Vec<_>>() } {
                        MessageView model:(model) message:(message.clone())
                    }
                }
            }
            WorkerPanel model:(model)
            if !model.comment_target.get().is_empty() {
                Composer model:(model)
            }
            row height:min-content gap:8px shrink:0 {
                button #action @click:{} disabled "Compact and continue"
                button #action @click:{} disabled "Reset context"
                button #action @click:{} disabled "Archive / start new"
            }
            text font-size:11px font-color:muted shrink:0
                "Context/reset controls unavailable until harness support. Cache expiry unknown."
        }
    }
}

#[component]
fn MessageView(model: Model, message: Message) -> Element {
    let target = message.clone();
    let keyboard = message.clone();
    let id = message.id.clone();
    let focused_id = message.id.clone();
    let quote_model = model;
    let content = view! {
        col height:min-content fill:surface radius:10px pad:18px gap:10px focus
            label:(format!("Message {}", message.id))
            @focus:{ move |focused| { if focused { model.focused_message.set(focused_id.clone()); } } }
            focused { stroke:(width:2px color:accent offset:-1px) } {
            row height:min-content justify:between align:center {
                row height:min-content gap:10px {
                    text font-size:13px font-weight:650 (message.author)
                    text font-size:11px font-color:muted (message.kind)
                }
                button #action @click:{ model.start_comment(&target); }
                    label:(format!("Comment on {}", message.id)) "Comment"
            }
            col height:min-content selectable {
                text font-size:15px (message.body)
            }
            for (_, comment) in { model.snapshot.get().comments.into_iter().filter(|c| c.message_id == id).map(|c| (c.id.clone(), c)).collect::<Vec<_>>() } {
                let quoted = State::new(comment.quote.clone());
                col height:min-content fill:accent-soft radius:6px pad:12px gap:6px selectable {
                    text font-size:12px font-weight:650 (comment.author.clone())
                    if quoted.get().is_some() {
                        text font-size:12px font-color:muted
                            { format!("“{}”", quoted.get().unwrap_or_default()) }
                    }
                    text font-size:14px (comment.body.clone())
                }
            }
        }
    };
    content.on_pointer(|event, ctx| {
        if matches!(event.kind, PointerEventKind::Down(PointerButton::Primary)) {
            ctx.request_focus();
        }
    });
    content.on_key(move |event, ctx| {
        if !matches!(event.kind, KeyEventKind::Down { .. }) {
            return;
        }
        let command = if cfg!(target_os = "macos") {
            event.modifiers.meta
        } else {
            event.modifiers.ctrl
        };
        if command && event.key == Key::Enter {
            quote_model.start_comment(&keyboard);
            ctx.stop_propagation();
        } else if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let ui = model.ui.get_untracked();
            let snapshot = ui.inspection_snapshot();
            let message_nodes: Vec<_> = snapshot
                .nodes
                .iter()
                .filter(|n| {
                    n.label
                        .as_deref()
                        .is_some_and(|l| l.starts_with("Message "))
                })
                .collect();
            for _ in 0..snapshot.nodes.len() {
                if event.key == Key::ArrowDown {
                    ui.focus_next();
                } else {
                    ui.focus_prev();
                }
                if ui
                    .focused()
                    .is_some_and(|focused| message_nodes.iter().any(|n| n.id == focused.id()))
                {
                    if let Some(focused) = ui.focused() {
                        focused.reveal();
                    }
                    break;
                }
            }
            ctx.stop_propagation();
        }
    });
    content
}

#[component]
fn Composer(model: Model) -> Element {
    view! {
        col height:min-content fill:surface radius:10px pad:14px gap:8px shrink:0 {
            row height:min-content gap:12px justify:between align:center {
                text font-size:12px font-weight:650
                    {
                    model.snapshot.get().messages.iter().find(|m| m.id == model.comment_target.get()).map(|m| format!("Feedback on {}’s {}", m.author, m.kind)).unwrap_or_else(|| "Contextual feedback".into())
                }
                button #action
                    @click:{ model.comment_body.set(String::new()); model.comment_quote.set(String::new()); model.comment_target.set(String::new()); }
                    disabled:{ model.busy.get() } "Discard draft"
            }
            if !model.comment_quote.get().is_empty() {
                text font-size:12px font-color:muted
                    { format!("“{}”", model.comment_quote.get()) }
            }
            row height:min-content gap:10px align:center {
                input #input-field width:140px label:"Comment author" placeholder:"Your name"
                    model.author
                input #area multiline height:74px width:1fr label:"Comment body"
                    placeholder:"Leave contextual feedback…" model.comment_body as comment_input
                { comment_input.focus(); }
                button #action @click:{ model.save_comment(); }
                    disabled:{ model.busy.get() || !model.connected.get() || model.comment_body.get().trim().is_empty() }
                    label:"Send comment" "Send"
            }
        }
    }
}

#[component]
fn Profiles(model: Model) -> Element {
    view! {
        col width:1fr pad:(horizontal:24px vertical:8px) gap:14px {
            scroll width:max-content {
                row height:min-content width:max-content gap:8px {
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
            { tabs.root().style(Style::stack().width(Dimension::Fill).height(42.0).basis(42.0).shrink(0.0)); }
            scroll {
                col height:min-content gap:20px {
                    text font-size:20px font-weight:650
                        { if model.editor.get() == EditTarget::Defaults { "Project defaults" } else { "Director profile" } }
                    if model.editor.get() != EditTarget::Defaults {
                        input #input-field label:"Director name" model.editor_name
                    }
                    text font-size:12px font-color:muted
                        "Effective profiles gate issue-linked worker execution."
                    ProfileField model:(model) title:"Agent harness" field:"harness"
                    row height:min-content gap:8px {
                        button #action
                            @click:{ model.modify_profile("harness", |p| p.harness = Harness::Codex); }
                            fill:if model.editor_profile.get().harness == Harness::Codex { accent-soft } else { raised }
                            "Codex"
                        button #action
                            @click:{ model.modify_profile("harness", |p| p.harness = Harness::ClaudeCode); }
                            fill:if model.editor_profile.get().harness == Harness::ClaudeCode { accent-soft } else { raised }
                            "Claude Code"
                    }
                    ProfileField model:(model) title:"Scope" field:"scope"
                    row height:min-content gap:8px {
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
                    text font-size:12px font-color:muted
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
                    row height:min-content gap:10px align:center {
                        button #action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = p.max_workers.saturating_sub(1)); }
                            label:"Decrease worker limit" "−"
                        text font-size:18px { model.editor_profile.get().max_workers.to_string() }
                        button #action
                            @click:{ model.modify_profile("max_workers", |p| p.max_workers = (p.max_workers + 1).min(64)); }
                            label:"Increase worker limit" "+"
                        text font-size:12px font-color:muted "0 pauses delegation"
                    }
                    ProfileField model:(model) title:"Action permissions" field:"permissions"
                    col height:min-content gap:8px {
                        for task in Task::ALL {
                            row height:min-content align:center justify:between {
                                text font-size:13px (task.label())
                                button #action
                                    @click:{ model.modify_profile("permissions", |p| { let current = p.permissions[&task]; p.permissions.insert(task, current.next()); }); }
                                    label:{ format!("{} permission", task.label()) }
                                    { model.editor_profile.get().permissions[&task].label() }
                            }
                        }
                    }
                    row height:min-content gap:8px {
                        button #action @click:{ model.export_toml(); } "Export / edit TOML"
                        button #action
                            @click:{ model.toml.set(model.editor_profile.get_untracked().to_toml()); model.advanced.set(true); }
                            "Show effective profile"
                    }
                    if model.advanced.get() {
                        col height:min-content gap:10px {
                            text font-size:12px font-color:muted
                                "Project defaults use a complete profile. Directors use overrides; omitted fields inherit. Copy this text to export."
                            input #area multiline height:240px label:"Profile TOML" model.toml
                            button #action @click:{ model.import_toml(); } "Import TOML into draft"
                        }
                    }
                }
            }
            row height:min-content gap:12px align:center shrink:0 {
                button #action @click:{ model.save_profile(); }
                    disabled:{ model.busy.get() || !model.connected.get() } label:"Save profile"
                    "Save profile"
                text font-size:12px font-color:muted
                    "Explicit overrides survive project default changes"
            }
        }
    }
}

#[component]
fn ProfileField(model: Model, title: &'static str, field: &'static str) -> Element {
    view! {
        row height:min-content justify:between align:center {
            col height:min-content gap:4px {
                text font-size:13px font-weight:650 (title)
                text font-size:11px font-color:muted { model.origin(field) }
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
        grid cols:(1fr 1fr 1fr 1fr) height:min-content gap:6px {
            for task in Task::ALL {
                button #action
                    @click:{ model.modify_profile(if completion { "completion" } else { "responsibilities" }, |p| {
                    let steps = if completion { &mut p.completion } else { &mut p.responsibilities };
                    if steps.contains(&task) { steps.retain(|t| t != &task); } else { steps.push(task); }
                }); }
                    width:fill {
                    text font-size:12px
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
        col fill:#00000055 align:center pad:(horizontal:30px vertical:90px) {
            col height:min-content width:560px max-width:100% fill:surface radius:14px pad:18px
                gap:12px {
                row height:min-content justify:between align:center {
                    text font-size:15px font-weight:650 "Command palette"
                    button #action @click:{ model.palette.set(false); } "Esc"
                }
                input #input-field placeholder:"Find an action…" label:"Command search" query
                    as command_search
                { command_search.focus(); }
                for (label, index) in [("Open board", 0), ("Open sessions", 1), ("Edit project defaults", 2), ("Create director", 3), ("Search transcript", 4), ("Sync project", 5), ("Stop worker", 6)] {
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
        _ => {
            model.page.set(Page::Sessions);
            model.searching.set(true);
        }
    }
    model.palette.set(false);
    model.palette_query.set(String::new());
}

#[component]
fn WorkerForm(model: Model, continuation: bool) -> Element {
    view! {
        col height:min-content gap:10px {
            text font-size:13px font-weight:650 { if continuation { "Continue this session" } else { "Start issue worker" } }
            if !continuation {
                for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                    let id = State::new(director.id.clone());
                    button #action @click:{ model.worker_director.set(id.get_untracked()); model.worker_approval.set(false); }
                        fill:if model.worker_director.get() == id.get() { accent-soft } else { raised }
                        { model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| format!("Director: {}", d.name)).unwrap_or_default() }
                }
            }
            text font-size:12px font-color:muted {
                match model.worker_profile(continuation) {
                    Ok((p, active)) => format!("{:?} · Implement {} · scope {:?} · {} / {} workers active", p.harness, p.permissions.get(&Task::Implement).copied().unwrap_or(Permission::Deny).label(), p.scope, active, p.max_workers),
                    Err(error) => error
                }
            }
            input #input-field label:"Worker prompt" placeholder:"Prompt for this turn…" model.worker_prompt
            if model.worker_profile(continuation).is_ok_and(|(p, _)| p.permissions.get(&Task::Implement) == Some(&Permission::Ask)) {
                checkbox label:"Approve implementation for this turn" model.worker_approval
            }
            text font-size:11px font-color:muted { model.worker_gate(continuation).err().unwrap_or_else(|| "Ready · server rechecks policy and revision".into()) }
            button #action @click:{ model.run_worker(continuation); }
                label:if continuation { "Send worker prompt" } else { "Start worker" }
                disabled:{ model.worker_gate(continuation).is_err() }
                { if continuation { "Send / continue" } else { "Start worker" } }
        }
    }
}

#[component]
fn WorkerPanel(model: Model) -> Element {
    let worker = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .into_iter()
            .find(|s| s.id == model.session.get())
            .and_then(|s| s.worker)
    });
    view! {
          col height:min-content {
            if worker.get().is_some() {
                scroll {
                    col height:min-content gap:10px {
                        text font-size:13px font-weight:650 { format!("Worker: {:?}", worker.get().unwrap().status) }
                        text font-size:12px font-color:muted { worker.get().unwrap().error.unwrap_or_default() }
                        if worker.get().is_some_and(|w| matches!(w.status, WorkerStatus::Running | WorkerStatus::Queued)) {
                            button #action @click:{ model.stop_worker(); } label:"Stop worker" disabled:{ model.busy.get() || !model.connected.get() } "Stop worker"
                        } else {
                            WorkerForm model:(model) continuation:true
                        }
                        text font-size:11px font-color:muted "Completed means the harness turn ended; issue acceptance still needs review."
                        text font-size:12px font-color:muted {
                            worker.get().unwrap().usage.map(|u| format!("Latest measured turn · input {} · cached input {} · output {} tokens", u.input_tokens, u.cached_input_tokens, u.output_tokens)).unwrap_or_else(|| "Latest turn usage unavailable".into())
                        }
                        button #action @click:{ model.review_changes.set(!model.review_changes.get_untracked()); } label:"Toggle change review" "Changes and provenance"
                        if model.review_changes.get() {
                            col height:min-content gap:8px selectable {
                                text font-size:12px {
                                    let w = worker.get().unwrap();
                                    format!("Branch: {}\nBase: {}\nWorktree: {}\nThread: {}", w.branch.unwrap_or_else(|| "unavailable".into()), w.base_commit.unwrap_or_else(|| "unavailable".into()), w.worktree.unwrap_or_else(|| "unavailable".into()), w.thread_id.unwrap_or_else(|| "unavailable".into()))
                                }
                                text font-size:12px {
                                    worker.get().unwrap().changes.map(|c| format!("Files: {}\n{}\n{}", c.files.join(", "), if c.truncated { "Review truncated by server" } else { "Bounded review" }, c.diff)).unwrap_or_else(|| "Change review unavailable".into())
                                }
                            }
                        }
                    }
                } as panel
                { panel.root().style(Style::stack().width(Dimension::Fill).height(280.0).shrink(0.0)); }
            }
        }
    }
}
