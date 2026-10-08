use super::*;
use std::{cell::RefCell, rc::Rc};

/// Whether a project is the read-only demo fixture.
pub(crate) fn fixture_project(model: Model, project_id: &str) -> bool {
    model
        .snapshot
        .get()
        .projects
        .iter()
        .any(|p| p.id == project_id && p.fixture)
}

/// The board's source without repeating its name.
pub(crate) fn source_label(model: Model) -> String {
    if fixture_project(model, &model.project.get()) {
        return "Fixture".to_string();
    }
    if let Some(board) = model.selected_board() {
        return match &board.source {
            BoardSource::Local => "Local board".to_string(),
            BoardSource::Github { owner, number, .. } => format!("GitHub {owner} · board {number}"),
            BoardSource::Gitlab {
                host,
                path,
                number,
                group,
                ..
            } => format!(
                "GitLab {host}/{path} · {} board {number}",
                if *group { "group" } else { "project" }
            ),
        };
    }
    model
        .snapshot
        .get()
        .projects
        .iter()
        .find(|p| p.id == model.project.get())
        .map(|p| match &p.github {
            Some(g) => format!("GitHub {} · board {}", g.owner, g.number),
            None => "Fixture board".into(),
        })
        .unwrap_or_default()
}

/// When a remote board was last synchronised, if the board is remote.
pub(crate) fn sync_age(model: Model) -> Option<String> {
    let synced = match model.selected_board() {
        Some(board) if board.source == BoardSource::Local => return None,
        Some(board) => board.last_synced_at,
        None => {
            model
                .snapshot
                .get()
                .projects
                .iter()
                .find(|p| p.id == model.project.get())
                .and_then(|p| p.github.as_ref())?
                .last_synced_at
        }
    };
    Some(
        sync_label(synced)
            .trim_start_matches("Last synced ")
            .to_string(),
    )
}

/// The last synchronisation error of the visible board, if any.
pub(crate) fn board_error(model: Model) -> Option<String> {
    let error = match model.selected_board() {
        Some(board) => board.error,
        None => model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == model.project.get())
            .and_then(|p| p.github.as_ref()?.sync_error.clone()),
    };
    error.filter(|e| !e.trim().is_empty())
}

fn can_sync(model: Model) -> bool {
    let snapshot = model.snapshot.get();
    model
        .selected_board()
        .is_some_and(|b| b.source != BoardSource::Local)
        || (model.selected_board().is_none()
            && snapshot
                .projects
                .iter()
                .any(|p| p.id == model.project.get() && p.github.is_some())
            && !snapshot
                .boards
                .iter()
                .any(|b| b.project_id == model.project.get()))
}

fn active_boards(model: Model) -> Vec<Board> {
    let snapshot = model.snapshot.get();
    snapshot
        .boards
        .iter()
        .filter(|b| b.project_id == model.project.get() && snapshot.board_active(&b.id))
        .cloned()
        .collect()
}

fn select_board(model: Model, id: &str) {
    model.preferences.update(|p| {
        p.selected_boards
            .insert(model.project.get_untracked(), id.to_string());
    });
    model.issue.set(None);
}

/// The project board: a 56px header whose cells carry the source, sync,
/// board choice and actions, then full-height columns.
#[component]
pub(crate) fn Board(model: Model, narrow: Derived<bool>) -> Element {
    let columns = Derived::new(move || model.board_columns());
    let width = State::new(1200.0f32);
    // Full header cells need room; below this the source, sync and board
    // choice move into the Board actions menu.
    let full = Derived::new(move || width.get() >= px(960.0));
    let stacked = Derived::new(move || narrow.get() || width.get() < px(620.0));
    let creating = State::new(false);
    let managing = State::new(false);
    let history = State::new(false);
    let draft = TaskDraft {
        title: State::new(String::new()),
        body: State::new(String::new()),
        repository: State::new(None),
    };
    let menu = State::new(false);
    let trigger_slot: Rc<RefCell<Option<Element>>> = Rc::default();
    let project_name = Derived::new(move || {
        model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == model.project.get())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    });
    let source = Derived::new(move || source_label(model));
    let synced = Derived::new(move || sync_age(model).unwrap_or_default());
    let local = Derived::new(move || {
        model
            .selected_board()
            .is_some_and(|b| b.source == BoardSource::Local)
    });
    view! {
        col width:1fr min-width:0px @layout:{move |rect: Rect| width.set(rect.size.width)} {
            PageHeader model:(model) eyebrow:("Project board".to_string()) title:(project_name)
                compact:(Derived::new(move || !full.get())) {
                if full.get() {
                    HeaderCell key:("Source".to_string()) value:(source)
                }
                if full.get() && can_sync(model) {
                    HeaderCell key:("Last sync".to_string()) value:(synced) {
                        button #relay.action @click:{ model.sync_project(); } label:"Sync project"
                            pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                            disabled:{ model.busy.get() || !model.connected.get() } "Sync"
                    }
                }
                if full.get() && active_boards(model).len() > 1 {
                    col #relay.cell width:max-content label:"Boards" {
                        row #relay.eyebrow height:min-content {
                            text text-transform:uppercase letter-spacing:{px(0.6)}px "Board"
                        }
                        BoardChoices model:(model)
                    }
                }
                row #relay.cell width:max-content align:center gap:{px(6.0)}px {
                    if model.selected_board().is_some() {
                        button #relay.primary @click:{creating.set(!creating.get_untracked());}
                            label:"New task" "New task"
                    }
                    row width:max-content height:min-content {
                        button #relay.action @click:{menu.set(!menu.get_untracked());}
                            label:"Board actions" pad:(horizontal:{px(10.0)}px vertical:{px(7.0)}px)
                            "⋯" as menu_trigger
                        { *trigger_slot.borrow_mut() = Some(menu_trigger.clone()); }
                        tooltip #relay.tooltip summary:"Board actions" trigger:manual open:menu
                            side:bottom align:end {
                            col height:min-content width:{px(260.0)}px gap:{px(6.0)}px {
                                if !full.get() {
                                    row #relay.eyebrow height:min-content { text text-transform:uppercase letter-spacing:{px(0.6)}px "Source" }
                                    row #relay.value height:min-content { text {source.get()} }
                                    if can_sync(model) {
                                        row height:min-content gap:{px(8.0)}px align:center {
                                            row #relay.caption height:min-content width:1fr { text {format!("Last sync {}", synced.get())} }
                                            button #relay.action @click:{ model.sync_project(); menu.set(false); }
                                                label:"Sync project" disabled:{ model.busy.get() || !model.connected.get() } "Sync"
                                        }
                                    }
                                    if active_boards(model).len() > 1 {
                                        row #relay.eyebrow height:min-content { text text-transform:uppercase letter-spacing:{px(0.6)}px "Board" }
                                        BoardChoices model:(model)
                                    }
                                }
                                if local.get() {
                                    button #relay.action width:fill justify:start
                                        @click:{managing.set(!managing.get_untracked()); menu.set(false);}
                                        label:"Manage columns" "Manage columns"
                                    button #relay.action width:fill justify:start
                                        @click:{model.page.set(Page::Publish); menu.set(false);}
                                        label:"Publish board" "Publish board"
                                }
                                button #relay.action width:fill justify:start
                                    @click:{history.set(!history.get_untracked()); menu.set(false);}
                                    label:"Operation history" "Operation history"
                            }
                        }
                    } as menu_anchor
                    { bind_menu(model, menu, &menu_anchor, trigger_slot.clone()); }
                }
            }
            if board_error(model).is_some() {
                row height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(8.0)}px)
                    fill:attention.fill stroke:(width:{px(4.0)} color:attention.text edges:left)
                    label:"Board error" {
                    row font-size:{px(12.0)}px font-color:attention.on {
                        text {board_error(model).unwrap_or_default()}
                    }
                }
            }
            if creating.get() && model.selected_board().is_some() {
                NewTaskForm model:(model) open:(creating) draft:(draft)
            }
            if managing.get() {
                col height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(12.0)}px)
                    stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                    Columns model:(model) open:(managing)
                }
            }
            if model.snapshot.get().operations.iter().any(|o| o.project_id == model.project.get() && o.state != OperationState::Completed) {
                col height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(12.0)}px)
                    stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                    Operations model:(model)
                }
            }
            if history.get() {
                col height:min-content pad:(horizontal:{px(24.0)}px vertical:{px(12.0)}px)
                    stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                    OperationHistory model:(model) open:(history)
                }
            }
            if stacked.get() {
                scroll {
                    col height:min-content {
                        for (_, column) in { columns.get().into_iter().map(|c| (c.id.clone(), c)) } {
                            BoardColumnView model:(model) column:(column.clone()) scrolls:(false)
                        }
                    }
                }
            } else {
                row height:1fr {
                    for (_, column) in { columns.get().into_iter().map(|c| (c.id.clone(), c)) } {
                        BoardColumnView model:(model) column:(column.clone()) scrolls:(true)
                    }
                }
            }
            BoardFooter model:(model)
        }
    }
}

/// The board choice: one square choice per active board.
#[component]
pub(crate) fn BoardChoices(model: Model) -> Element {
    view! {
        row height:min-content gap:{px(4.0)}px {
            for (_, board) in {active_boards(model).into_iter().map(|b| (b.id.clone(), b)).collect::<Vec<_>>()} {
                let id = State::new(board.id.clone());
                let board_name = State::new(board.name.clone());
                button #relay.action @click:{select_board(model, &id.get_untracked());}
                    label:{format!("Select board {}",board_name.get())}
                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                    fill:if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.inverse} else {surface.panel}
                    hover {
                        fill:if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.inverse} else {surface.raised}
                    }
                    pressed {
                        fill:if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.inverse} else {surface.raised}
                    } {
                    row width:max-content
                        font-color:{color(if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.on_inverse} else {ink.fg})}
                        font-weight:{if model.selected_board().is_some_and(|b|b.id==id.get()) {700} else {400}} {
                        text {board_name.get()}
                    }
                }
            }
        }
    }
}

/// The new task form's draft. The board owns it, so hiding the form keeps
/// what was entered.
#[derive(Clone, Copy)]
pub(crate) struct TaskDraft {
    title: State<String>,
    body: State<String>,
    repository: State<Option<String>>,
}

/// The new task form, opened from the board header.
#[component]
pub(crate) fn NewTaskForm(model: Model, open: State<bool>, draft: TaskDraft) -> Element {
    let TaskDraft {
        title,
        body,
        repository,
    } = draft;
    view! {
        col height:min-content gap:{px(8.0)}px pad:(horizontal:{px(24.0)}px vertical:{px(12.0)}px)
            stroke:(width:{px(1.0)} color:rule.line edges:bottom) label:"New task" {
            input #relay.field label:"New task title" placeholder:"Task title" title
            input #relay.area multiline label:"New task body" placeholder:"Description"
                height:{px(100.0)}px body
            row #relay.eyebrow height:min-content {
                text text-transform:uppercase letter-spacing:{px(0.6)}px
                    "Issue repository · optional for local tasks"
            }
            row height:min-content gap:{px(6.0)}px {
                if model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                    button #relay.action @click:{repository.set(None);}
                        fill:if repository.get().is_none() {ink.inverse} else {surface.panel}
                        hover {
                            fill:if repository.get().is_none() {ink.inverse} else {surface.raised}
                        }
                        pressed {
                            fill:if repository.get().is_none() {ink.inverse} else {surface.raised}
                        } {
                        row width:max-content
                            font-color:{color(if repository.get().is_none() {ink.on_inverse} else {ink.fg})} {
                            text "Local task"
                        }
                    }
                }
                for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && matches!(c.kind,ConnectionKind::Repository{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                    let connection_id=State::new(connection.id.clone());
                    let connection_name=State::new(connection.name.clone());
                    button #relay.action
                        @click:{repository.set(Some(connection_id.get_untracked()));}
                        label:{format!("New task repository {}",connection_name.get())}
                        fill:if repository.get().as_ref()==Some(&connection_id.get()) {ink.inverse} else {surface.panel}
                        hover {
                            fill:if repository.get().as_ref()==Some(&connection_id.get()) {ink.inverse} else {surface.raised}
                        }
                        pressed {
                            fill:if repository.get().as_ref()==Some(&connection_id.get()) {ink.inverse} else {surface.raised}
                        } {
                        row width:max-content
                            font-color:{color(if repository.get().as_ref()==Some(&connection_id.get()) {ink.on_inverse} else {ink.fg})} {
                            text {connection_name.get()}
                        }
                    }
                }
            }
            row height:min-content gap:{px(8.0)}px {
                button #relay.primary
                    @click:{if let Some(board)=model.selected_board(){model.action(Command::CreateTask{board_id:board.id,title:title.get_untracked(),body:body.get_untracked(),repository_connection_id:repository.get_untracked()});}}
                    disabled:{model.busy.get() || !model.connected.get() || title.get().trim().is_empty() || (model.selected_board().is_some_and(|b|b.source!=BoardSource::Local) && repository.get().is_none())}
                    "Create task"
                button #relay.action @click:{open.set(false);} "Cancel"
            }
        }
    }
}

/// One board column: a 40px head on a strong rule, then its cards. Columns
/// are divided by quiet full-height rules.
#[component]
pub(crate) fn BoardColumnView(model: Model, column: BoardColumn, scrolls: bool) -> Element {
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
    let title = Derived::new(move || {
        model
            .board_columns()
            .iter()
            .find(|c| c.id == column_id.get())
            .map(|c| c.title.clone())
            .unwrap_or_default()
    });
    view! {
        col width:1fr min-width:0px
            height:if scrolls { {Dimension::Fill} } else { {Dimension::MinContent} }
            stroke:(width:{px(1.0)} color:rule.hair edges:right) {
            row #relay.strip height:{px(40.0)}px shrink:0 align:center gap:{px(10.0)}px
                pad:(horizontal:{px(12.0)}px vertical:0px)
                label:{ format!("Column {}", column_id.get()) } {
                row #relay.title height:min-content width:1fr font-size:{px(13.0)}px clip {
                    text text-wrap:none {title.get()}
                }
                row #relay.caption height:min-content width:max-content {
                    text {format!("{:02}", issues.get().len())}
                }
            }
            if scrolls {
                scroll {
                    col height:min-content gap:{px(12.0)}px pad:{px(12.0)}px {
                        for (_, issue) in { issues.get().into_iter().map(|i| (i.id.clone(), i)) } {
                            IssueCard model:(model) issue:(issue.clone())
                        }
                    }
                }
            } else {
                col height:min-content gap:{px(12.0)}px pad:{px(12.0)}px {
                    for (_, issue) in { issues.get().into_iter().map(|i| (i.id.clone(), i)) } {
                        IssueCard model:(model) issue:(issue.clone())
                    }
                }
            }
        }
    }
}

/// An issue as a label: identifier strip, title and preview, status foot.
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
    // Fixture tasks cannot run, so they read as fixtures whatever their sessions.
    let state = Derived::new(move || {
        if fixture_project(model, &current.get().project_id) {
            return RunState::Fixture;
        }
        crate::labels::issue_status(&model.snapshot.get(), &model.sessions_for_task(&status_id))
    });
    let selected_id = id.clone();
    let selected = Derived::new(move || model.issue.get().as_deref() == Some(selected_id.as_str()));
    let preview = Derived::new(move || body_preview(&current.get().body));
    view! {
        button @click:{ model.issue.set(Some(id.clone())); model.worker_approval.set(false); }
            width:fill height:min-content fill:surface.panel pad:0px radius:0px
            label:{ current.get().reference.map(|r|format!("Open issue #{}",r.number)).unwrap_or_else(||format!("Open local task {}",current.get().title)) }
            description:{ format!("{} · {} linked sessions", state.get().label(), session_count.get()) }
            stroke:(width:{px(if selected.get() {2.0} else {1.0})} color:{color(if selected.get() {ink.fg} else {rule.line})} offset:{px(-1.0)})
            shadow:if selected.get() {
                (offset:(x:{px(4.0)} y:{px(4.0)}) blur:0.0 color:{color(ink.fg)})
            } else {
                (offset:(x:0.0 y:0.0) blur:0.0 color:{Color::TRANSPARENT})
            }
            hover { fill:surface.raised }
            focused { stroke:(width:{px(2.0)} color:accent.focus offset:{px(2.0)}) } {
            col height:min-content gap:0px {
                row height:{px(22.0)}px stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                    row #relay.id-label width:max-content height:fill align:center
                        pad:(horizontal:{px(8.0)}px vertical:0px) fill:ink.inverse
                        font-size:{px(11.0)}px {
                        text text-wrap:none { issue_number(&current.get()) }
                    }
                    LabelStrip labels:(Derived::new(move || current.get().labels))
                }
                col height:min-content gap:{px(6.0)}px
                    pad:(left:{px(10.0)}px right:{px(10.0)}px top:{px(10.0)}px bottom:{px(12.0)}px) {
                    row #relay.title font-size:{px(14.5)}px height:min-content {
                        text label:{ current.get().title } { current.get().title }
                    }
                    if !preview.get().is_empty() {
                        col #relay.preview font-color:ink.muted label:"Task preview" {
                            text {preview.get()}
                        }
                    }
                }
                row height:{px(26.0)}px stroke:(width:{px(1.0)} color:rule.hair edges:top) {
                    row width:1fr min-width:0px align:center gap:{px(6.0)}px
                        pad:(horizontal:{px(8.0)}px vertical:0px) clip {
                        if state.get() != RunState::Ready {
                            StatusGlyph state:(state)
                        }
                        row width:max-content height:min-content font-size:{px(11.0)}px
                            font-color:{color(state.get().text_color())} {
                            text text-wrap:none { state.get().label() }
                        }
                    }
                    if session_count.get() > 0 {
                        row #relay.caption height:min-content width:max-content align:center
                            font-size:{px(11.0)}px pad:(horizontal:{px(8.0)}px vertical:0px)
                            stroke:(width:{px(1.0)} color:rule.hair edges:left) {
                            text text-wrap:none
                                { format!("{} {}", session_count.get(), if session_count.get() == 1 { "session" } else { "sessions" }) }
                        }
                    }
                }
            }
        }
    }
}

/// A key and value row of the issue detail grid.
#[component]
fn DetailRow(key: String, value: Derived<String>) -> Element {
    view! {
        row height:min-content stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
            row #relay.eyebrow height:min-content width:{px(96.0)}px shrink:0
                pad:(horizontal:{px(14.0)}px vertical:{px(7.0)}px)
                stroke:(width:{px(1.0)} color:rule.hair edges:right) {
                text text-transform:uppercase letter-spacing:{px(0.6)}px (key.clone())
            }
            row #relay.value height:min-content width:1fr min-width:0px
                pad:(horizontal:{px(14.0)}px vertical:{px(7.0)}px) font-size:{px(12.0)}px {
                text {value.get()}
            }
        }
    }
}

/// The issue inspector: identifier block, key/value grid, body, linked
/// sessions and the worker form.
#[component]
pub(crate) fn IssueDetail(model: Model) -> Element {
    // A tall inspector keeps the worker setup anchored at the bottom; a short
    // one (small window, large scale) scrolls it with the details.
    let height = State::new(0.0f32);
    let anchored = Derived::new(move || height.get() >= px(760.0));
    let issue = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .find(|i| Some(&i.id) == model.issue.get().as_ref())
    });
    view! {
        col width:{px(392.0)} shrink:1 fill:surface.panel
            stroke:(width:{px(1.0)} color:rule.line edges:left)
            @layout:{move |rect: Rect| height.set(rect.size.height)} label:"Issue details" {
            if issue.get().is_none() {
                row #relay.strip height:{px(56.0)}px shrink:0 align:center justify:between
                    pad:(horizontal:{px(14.0)}px vertical:0px) {
                    row #relay.caption height:min-content width:max-content {
                        text "Issue unavailable"
                    }
                    button #relay.action @click:{ model.issue.set(None); }
                        label:"Close issue details" "Close"
                }
            }
            for (_, detail) in { issue.get().into_iter().map(|i| (i.id.clone(), i)) } {
                let detail_id = State::new(detail.id.clone());
                let fallback = detail.clone();
                let current = Derived::new(move || model.snapshot.get().issue(&detail_id.get()).cloned().unwrap_or_else(|_| fallback.clone()));
                let fixture = Derived::new(move || model.snapshot.get().projects.iter().any(|p| p.id == current.get().project_id && p.fixture));
                col gap:0px {
                    row height:min-content shrink:0
                        stroke:(width:{px(1.0)} color:rule.line edges:bottom) label:"Issue header" {
                        col #relay.id-label width:{px(74.0)}px min-height:{px(92.0)}px shrink:0
                            align:center justify:center fill:ink.inverse font-size:{px(24.0)}px {
                            text text-wrap:none { issue_number(&current.get()) }
                        }
                        col width:1fr min-width:0px height:min-content gap:{px(6.0)}px
                            pad:(left:{px(14.0)}px right:{px(8.0)}px top:{px(8.0)}px bottom:{px(10.0)}px) {
                            row height:min-content align:center gap:{px(8.0)}px {
                                row #relay.eyebrow height:min-content width:1fr {
                                    text text-transform:uppercase letter-spacing:{px(0.6)}px
                                        "Issue details"
                                }
                                button #relay.action @click:{ model.issue.set(None); }
                                    label:"Close issue details"
                                    pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px) "Close"
                            }
                            row #relay.title font-size:{px(17.0)}px height:min-content selectable {
                                text label:{ current.get().title } { current.get().title }
                            }
                            row height:min-content gap:{px(4.0)}px clip {
                                for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                                    Tag text:(label.clone())
                                }
                            }
                        }
                    }
                    scroll {
                        col height:min-content gap:0px selectable {
                            DetailRow key:("Source".to_string())
                                value:(Derived::new(move || if fixture.get() { "Fixture issue · execution unavailable".to_string() } else { match current.get().reference { Some(r) => r.repository, None => "Local task".into() } }))
                            if !fixture.get() && current.get().reference.is_some() {
                                DetailRow key:("Link".to_string())
                                    value:(Derived::new(move || current.get().reference.map(|r| r.url).unwrap_or_default()))
                            }
                            col height:min-content
                                pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px)
                                stroke:(width:{px(1.0)} color:rule.line edges:bottom) {
                                row font-size:{px(13.0)}px font-color:ink.muted height:min-content {
                                    text label:{ crate::projects::task_body(&current.get().body) }
                                        { crate::projects::task_body(&current.get().body) }
                                }
                            }
                            if !model.snapshot.get().visible_task(&current.get().id) {
                                col height:min-content pad:{px(10.0)}px fill:attention.fill
                                    stroke:(width:{px(4.0)} color:attention.text edges:left) {
                                    row font-size:{px(12.0)}px font-color:attention.on {
                                        text label:"Issue removed from board"
                                            "No longer on this board · history retained. Restore and sync before starting or continuing a worker. Active turns may finish or be stopped."
                                    }
                                }
                            }
                            row height:{px(30.0)}px shrink:0 align:center justify:between
                                pad:(horizontal:{px(14.0)}px vertical:0px)
                                stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                                row #relay.eyebrow height:min-content width:max-content {
                                    text text-transform:uppercase letter-spacing:{px(0.6)}px
                                        "Linked sessions"
                                }
                                row #relay.caption height:min-content width:max-content {
                                    text
                                        { format!("{:02}", model.sessions_for_task(&detail_id.get()).len()) }
                                }
                            }
                            for (_, session) in { model.sessions_for_task(&detail_id.get()).into_iter().map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                                let id = State::new(session.id.clone());
                                let linked_state = Derived::new(move || model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| crate::labels::session_state(&model.snapshot.get(), s)).unwrap_or(RunState::Unavailable));
                                let linked_title = Derived::new(move || model.snapshot.get().sessions.iter().find(|s| s.id == id.get()).map(|s| s.title.clone()).unwrap_or_default());
                                button #relay.tree-control
                                    @click:{ model.open_session(id.get_untracked()); } width:fill
                                    height:{px(28.0)}px justify:start gap:{px(8.0)}px
                                    pad:(horizontal:{px(14.0)}px vertical:0px)
                                    stroke:(width:{px(1.0)} color:rule.hair edges:bottom)
                                    label:{ linked_title.get() }
                                    description:{ linked_state.get().label() }
                                    hover { fill:surface.raised } {
                                    StatusGlyph state:(linked_state)
                                    stack #relay.fade-label font-size:{px(12.0)}px
                                        font-color:ink.fg {
                                        row #relay.fade-line {
                                            text width:max-content shrink:0 text-wrap:none
                                                { linked_title.get() }
                                        }
                                    }
                                    row width:max-content height:min-content shrink:0
                                        font-size:{px(11.0)}px
                                        font-color:{color(linked_state.get().text_color())} {
                                        text text-wrap:none { linked_state.get().label() }
                                    }
                                }
                            }
                            col height:min-content
                                pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px)
                                gap:{px(12.0)}px {
                                TaskEditor model:(model)
                                if current.get().result.is_some() {
                                    col height:min-content gap:{px(6.0)}px pad:(top:{px(8.0)}px)
                                        stroke:(width:{px(1.0)} color:rule.line edges:top) {
                                        row #relay.eyebrow height:min-content {
                                            text text-transform:uppercase letter-spacing:{px(0.6)}px
                                                "Result"
                                        }
                                        row font-size:{px(14.0)}px height:min-content {
                                            text { current.get().result.unwrap_or_default() }
                                        }
                                    }
                                }
                            }
                            if !anchored.get() {
                                col height:min-content
                                    pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px) {
                                    WorkerForm model:(model) continuation:false
                                }
                            }
                        }
                    }
                    if anchored.get() {
                        col height:min-content shrink:0
                            pad:(horizontal:{px(14.0)}px vertical:{px(12.0)}px)
                            stroke:(width:{px(1.0)} color:rule.line edges:top) {
                            WorkerForm model:(model) continuation:false
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
    let choosing = State::new(false);
    // Which director and resources the next worker uses, in one line.
    let setup = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let director = snapshot
            .directors
            .iter()
            .find(|d| d.id == model.worker_director.get())
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "No director".into());
        let resources = match model.workspace_selection.get() {
            None => "automatic resources".to_string(),
            Some(ids) => format!(
                "{} resource{}",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
        };
        format!("{director} · {resources}")
    });
    // One status line: why the worker cannot start, or that it can.
    let gate_status = Derived::new(move || {
        model
            .worker_gate(continuation)
            .err()
            .or_else(|| model.worker_profile(continuation).err())
            .unwrap_or_else(|| "Ready · server rechecks policy and revision".into())
    });
    view! {
        col #relay.module label:"Worker setup" {
            row #relay.module-head gap:{px(8.0)}px {
                row #relay.eyebrow height:min-content width:max-content {
                    text text-transform:uppercase letter-spacing:{px(0.6)}px
                        (if continuation { "Continue this session" } else { "Start task worker" })
                }
                if !continuation {
                    row #relay.caption height:min-content width:1fr min-width:0px clip {
                        text text-wrap:none {setup.get()}
                    }
                    button #relay.action @click:{ choosing.set(!choosing.get_untracked()); }
                        pad:(horizontal:{px(8.0)}px vertical:{px(2.0)}px)
                        label:"Choose director and resources"
                        { if choosing.get() { "Done" } else { "Change" } }
                }
            }
            if choosing.get() && !continuation {
                col height:min-content gap:{px(6.0)}px pad:{px(12.0)}px
                    stroke:(width:{px(1.0)} color:rule.hair edges:bottom) {
                    for (_, director) in { model.snapshot.get().directors.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                        let id = State::new(director.id.clone());
                        button #relay.action
                            @click:{ model.worker_director.set(id.get_untracked()); model.worker_approval.set(false); }
                            width:fill justify:start role:radio
                            fill:if model.worker_director.get() == id.get() {surface.selected} else {surface.panel}
                            stroke:(width:{px(if model.worker_director.get() == id.get() {3.0} else {1.0})} color:{color(if model.worker_director.get() == id.get() {ink.fg} else {rule.line})} edges:left)
                            { model.snapshot.get().directors.iter().find(|d| d.id == id.get()).map(|d| format!("Director: {}", d.name)).unwrap_or_default() }
                    }
                    WorkspaceChoices model:(model)
                }
            }
            if model.worker_profile(continuation).is_ok() {
                grid
                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(80.0).into(), GridTrack::fr(1.0)))}
                    height:min-content gap:0px stroke:(width:{px(1.0)} color:rule.hair edges:bottom)
                    label:"Worker policy" {
                    for (_, key) in { worker_policy(model, continuation).into_iter().map(|(k, _)| (k, k)).collect::<Vec<_>>() } {
                        let field: &'static str = key;
                        col height:min-content min-width:0px
                            pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px)
                            stroke:(width:{px(1.0)} color:rule.hair edges:right) {
                            Readout key:(field.to_string())
                                value:(Derived::new(move || worker_policy(model, continuation).into_iter().find(|(k, _)| *k == field).map(|(_, v)| v).unwrap_or_default()))
                        }
                    }
                }
            }
            col height:min-content gap:{px(10.0)}px pad:{px(12.0)}px {
                input #relay.area multiline height:{px(92.0)}px label:"Worker prompt"
                    placeholder:"Prompt for this turn…" model.worker_prompt
                if model.worker_profile(continuation).is_ok_and(|(p, _)| p.permissions.get(&Task::Implement) == Some(&Permission::Ask)) {
                    WorkerApproval model:(model)
                }
                row height:min-content align:center gap:{px(12.0)}px {
                    row #relay.caption width:1fr min-width:0px height:min-content {
                        text {gate_status.get()}
                    }
                    button #relay.primary @click:{ model.run_worker(continuation); }
                        label:if continuation { "Send worker prompt" } else { "Start worker" }
                        disabled:{ model.worker_gate(continuation).is_err() }
                        { if continuation { "Send / continue" } else { "Start worker" } }
                }
            }
        }
    }
}

/// A card preview of a task body: recovery markers removed and whitespace
/// collapsed. The card clips it to three laid-out lines.
pub(crate) fn body_preview(body: &str) -> String {
    crate::projects::task_body(body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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

/// The board's reading strip, level with the sidebar footer: task and run
/// counts for the visible board, and worker activity across the project's
/// directors. Fixture projects cannot run, so they show only the task count.
#[component]
fn BoardFooter(model: Model) -> Element {
    let tasks = Derived::new(move || {
        let columns = model.board_columns();
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .filter(|i| columns.iter().any(|c| model.task_in_column(i, &c.id)))
            .map(|i| {
                crate::labels::issue_status(&model.snapshot.get(), &model.sessions_for_task(&i.id))
            })
            .collect::<Vec<_>>()
    });
    let with_state = move |state: RunState| {
        Derived::new(move || format!("{:02}", tasks.get().iter().filter(|s| **s == state).count()))
    };
    let fixture = Derived::new(move || fixture_project(model, &model.project.get()));
    // Each director limits its own workers, so the project's ceiling is the
    // sum of those limits. Director turns are not counted.
    let workers = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let (active, limit) = snapshot
            .directors
            .iter()
            .filter(|d| d.project_id == model.project.get())
            .map(|d| crate::labels::director_capacity(&snapshot, &d.id))
            .fold((0, 0), |(a, l), (_, active, limit)| (a + active, l + limit));
        format!("{active} / {limit} active")
    });
    // Narrow strips (small windows, large scales) read as one summary cell.
    let width = State::new(0.0f32);
    let compact = Derived::new(move || width.get() < px(560.0));
    let summary = Derived::new(move || {
        let count = |state: RunState| tasks.get().iter().filter(|s| **s == state).count();
        if fixture.get() {
            format!("{} tasks", tasks.get().len())
        } else {
            format!(
                "{} tasks · {} running · {} waiting · workers {}",
                tasks.get().len(),
                count(RunState::Running),
                count(RunState::Waiting),
                workers.get()
            )
        }
    });
    view! {
        row height:{px(52.0)}px shrink:0 stroke:(width:{px(1.0)} color:rule.line edges:top)
            @layout:{move |rect: Rect| width.set(rect.size.width)} label:"Board summary" {
            if compact.get() {
                HeaderCell key:("Board".to_string()) value:(summary)
            }
            if !compact.get() {
                HeaderCell key:("Tasks".to_string())
                    value:(Derived::new(move || format!("{:02}", tasks.get().len())))
            }
            if !fixture.get() && !compact.get() {
                HeaderCell key:("Running".to_string()) value:(with_state(RunState::Running))
                HeaderCell key:("Waiting".to_string()) value:(with_state(RunState::Waiting))
                HeaderCell key:("Project workers".to_string()) value:(workers)
            }
        }
    }
}

/// Owns the board actions menu's dismissal: Escape closes it and returns
/// focus to its trigger, and a press anywhere outside the trigger and menu,
/// including on another control, closes it. Clearing `open` also clears the tooltip's
/// own Escape suppression, so the next click on the trigger reopens it.
fn bind_menu(
    model: Model,
    open: State<bool>,
    anchor: &Element,
    trigger: Rc<RefCell<Option<Element>>>,
) {
    anchor.on_key(move |event, ctx| {
        if matches!(event.kind, KeyEventKind::Down { .. })
            && event.key == Key::Escape
            && open.get_untracked()
        {
            open.set(false);
            if let Some(trigger) = trigger.borrow().as_ref() {
                trigger.focus();
            }
            ctx.stop_propagation();
        }
    });
    anchor.on_pointer(|event, ctx| {
        if matches!(
            event.kind,
            PointerEventKind::Down(_) | PointerEventKind::Click(_)
        ) {
            ctx.stop_propagation();
        }
    });
    // Views are built under a placeholder root that mounting replaces, so
    // the outside-press handler joins the mounted root when the menu first
    // opens. It lives as long as the anchor.
    let anchor = anchor.clone();
    let watching = std::cell::Cell::new(false);
    Effect::new(move || {
        if !open.get() || watching.replace(true) {
            return;
        }
        model
            .ui
            .get_untracked()
            .root()
            .on_pointer_for(&anchor, move |event, _| {
                if matches!(event.kind, PointerEventKind::Down(PointerButton::Primary))
                    && open.get_untracked()
                {
                    open.set(false);
                }
            });
    });
}
