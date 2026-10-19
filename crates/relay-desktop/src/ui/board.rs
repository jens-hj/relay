use super::*;
use crate::panels::{BoundedPanel, BoundedPanelProps};
use crate::window_chrome::{WindowControlsSpace, WindowControlsSpaceProps};
use mosaic::widgets::OverlayAlign;
use relay_ui::overlay::{Popover, PopoverContent, PopoverProps};
use relay_ui::prelude::{
    CellButton, CellButtonProps, Ellipsis, EllipsisProps, Label, LabelProps, OnPress,
    PrimaryButton, PrimaryButtonProps, Rule, SecondaryButton, SecondaryButtonProps, TextRole, Tone,
    UiText, Weight,
};
use relay_ui::surfaces::{
    BoardColumn as ColumnFrame, BoardColumnProps as ColumnFrameProps, IssueCard as IssueCell,
    IssueCardProps as IssueCellProps, IssueInspector as IssueFrame,
    IssueInspectorProps as IssueFrameProps, IssueView, Module, ModuleProps, Notice as NoticeStrip,
    NoticeProps as NoticeStripProps, NoticeTone, ReadoutCell, ReadoutCellProps, TextInput,
    TextInputProps,
};
use std::rc::Rc;

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

/// The last synchronisation error of the visible board, unless an
/// unfinished operation row already shows it.
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
    error.filter(|e| {
        !e.trim().is_empty() && !crate::projects::operation_shows_error(&model.snapshot.get(), e)
    })
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

/// The project board: a header whose cells carry the source, sync, board
/// choice and actions, then full-height columns.
#[component]
pub(crate) fn Board(model: Model, narrow: Derived<bool>) -> Element {
    let columns = Derived::new(move || model.board_columns());
    let width = State::new(1200.0f32);
    let height = State::new(900.0f32);
    let disclosure_limit = Derived::new(move || (height.get() - px(90.0)).max(0.0) * 0.5);
    let operations_limit = Derived::new(move || (disclosure_limit.get() * 0.5).min(px(200.0)));
    // Full header cells need room; below this the source, sync and board
    // choice move into the Board actions menu.
    let full = Derived::new(move || width.get() >= px(960.0));
    let stacked = Derived::new(move || narrow.get() || width.get() < px(620.0));
    let tight = Derived::new(move || width.get() < px(620.0));
    let creating = State::new(false);
    let managing = State::new(false);
    let history = State::new(false);
    let draft = TaskDraft {
        title: State::new(String::new()),
        body: State::new(String::new()),
        repository: State::new(None),
    };
    let menu = State::new(false);
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
    let sync_disabled = Derived::new(move || model.busy.get() || !model.connected.get());
    let trigger = view! {
        CellButton label:"Board actions" on-press:(press(move || menu.set(!menu.get_untracked())))
            rule:(Rule::Left) horizontal-inset:(0.0) {
            icon size:{px(16.0)}px more-icon
        }
    };
    let menu_trigger = trigger.clone();
    let actions: PopoverContent =
        Rc::new(move || board_menu(model, menu, full, local, managing, history, source, synced));
    let header = view! {
        PageHeader model:(model) eyebrow:(Derived::new(|| "Project Board".to_string()))
            title:(project_name) compact:(Derived::new(move || !full.get())) {
            if full.get() {
                ReadoutCell key:"Source" value:(source)
            }
            if full.get() && can_sync(model) {
                ReadoutCell key:"Last sync" value:(synced) {
                    SecondaryButton text:"Sync" label:(Some("Sync project".into()))
                        disabled:(sync_disabled) on-press:(press(move || model.sync_project()))
                }
            }
            if full.get() && active_boards(model).len() > 1 {
                BoardChoices model:(model)
            }
            if model.selected_board().is_some() {
                row width:{if tight.get() {Dimension::Px(px(36.0))} else {Dimension::MaxContent}}
                    shrink:0 {
                    CellButton label:"New Task"
                        on-press:(press(move || creating.set(!creating.get_untracked())))
                        rule:(Rule::Left) selected:true
                        horizontal-inset:(if tight.get() {0.0} else {16.0}) {
                        if tight.get() {
                            icon size:{px(16.0)}px stroke:ground.ink plus-icon
                        } else {
                            Label text:"New Task" role:(TextRole::Label) weight:(Weight::SEMIBOLD)
                        }
                    }
                }
            }
            stack width:{px(36.0)}px shrink:0 {
                { parent.adopt(&trigger); }
                Popover open:(menu) label:"Board actions" content:(actions)
                    align:(OverlayAlign::End) trigger:(Some(menu_trigger)) gap:(0.0)
            }
        }
    };
    view! {
        col width:1fr min-width:0px
            @layout:{move |rect: Rect| {width.set(rect.size.width);height.set(rect.size.height);}} {
            { parent.adopt(&header); }
            if model.issue.get().is_none() {
                Notice model:(model)
            }
            BoundedPanel limit:(disclosure_limit) {
                if board_error(model).is_some() {
                    col height:min-content label:"Board error" {
                        NoticeStrip
                            text:(Derived::new(move || board_error(model).unwrap_or_default()))
                            tone:(NoticeTone::Failure)
                    }
                }
                if creating.get() && model.selected_board().is_some() {
                    NewTaskForm model:(model) open:(creating) draft:(draft)
                }
                if managing.get() {
                    Columns model:(model) open:(managing)
                }
                if history.get() {
                    OperationHistory model:(model) open:(history)
                }
            }
            // Unfinished operations sit directly over the board, closed by
            // the strong rule so the list never reads as a board column.
            Operations model:(model) separated:true limit:(Some(operations_limit))
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

/// One menu entry: a full-width cell over a hairline.
fn menu_item(label: &'static str, press: OnPress) -> Element {
    view! {
        row height:{px(36.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            CellButton label:(label) on-press:(press) start:true {
                Label text:(label) role:(TextRole::Label)
            }
        }
    }
}

/// The Board actions menu. Narrow headers move the source, sync and board
/// choice in here.
#[allow(clippy::too_many_arguments)]
fn board_menu(
    model: Model,
    menu: State<bool>,
    full: Derived<bool>,
    local: Derived<bool>,
    managing: State<bool>,
    history: State<bool>,
    source: Derived<String>,
    synced: Derived<String>,
) -> Element {
    let list = view! {
        col height:min-content width:{px(260.0)}px gap:0px {}
    };
    if !full.get_untracked() {
        let sync_disabled = Derived::new(move || model.busy.get() || !model.connected.get());
        list.adopt(&view! {
            col height:min-content gap:{px(6.0)}px pad:{px(12.0)}px
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Label text:"Source" role:(TextRole::Caption) tone:(Tone::Muted) weight:500 caps:true
                Label text:(source) role:(TextRole::Body)
                if can_sync(model) {
                    row height:min-content gap:{px(8.0)}px align:center {
                        row width:1fr min-width:0px height:min-content {
                            Label text:(Derived::new(move || format!("Last sync {}", synced.get())))
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                        SecondaryButton text:"Sync" label:(Some("Sync project".into()))
                            disabled:(sync_disabled)
                            on-press:(press(move || { model.sync_project(); menu.set(false); }))
                    }
                }
                if active_boards(model).len() > 1 {
                    row height:{px(60.0)}px {
                        BoardChoices model:(model)
                    }
                }
            }
        });
    }
    if local.get_untracked() {
        list.adopt(&menu_item(
            "Manage columns",
            Rc::new(move || {
                managing.set(!managing.get_untracked());
                menu.set(false);
            }),
        ));
        list.adopt(&menu_item(
            "Publish Board",
            Rc::new(move || {
                model.page.set(Page::Publish);
                menu.set(false);
            }),
        ));
    }
    list.adopt(&menu_item(
        "Operation History",
        Rc::new(move || {
            history.set(!history.get_untracked());
            menu.set(false);
        }),
    ));
    list
}

/// The board choice: one cell per active board.
#[component]
pub(crate) fn BoardChoices(model: Model) -> Element {
    view! {
        col width:max-content shrink:0 gap:0px label:"Board selector"
            stroke:(width:{px(1.0)} color:ground.rule edges:left) {
            row height:{px(24.0)}px shrink:0 align:center justify:center
                stroke:(width:{px(1.0)} color:ground.rule edges:bottom) {
                Label text:"Board" role:(TextRole::Caption) tone:(Tone::Muted) weight:500
            }
            row gap:0px {
                for ((index, _), board) in {active_boards(model).into_iter().enumerate().map(|(i,b)| ((i,b.id.clone()), b)).collect::<Vec<_>>()} {
                    let id = board.id.clone();
                    let chosen = id.clone();
                    let name = board.name.clone();
                    let first = *index == 0;
                    let provider = match &board.source { BoardSource::Local => 0, BoardSource::Github { .. } => 1, BoardSource::Gitlab { .. } => 2 };
                    row width:max-content shrink:0 {
                        CellButton label:(format!("Select board {name}"))
                            rule:(if first { Rule::None } else { Rule::Left })
                            on-press:(press(move || select_board(model, &id)))
                            selected:(Derived::new(move || model.selected_board().is_some_and(|b| b.id == chosen)))
                            horizontal-inset:(14.0) {
                            if provider == 0 {
                                icon size:{px(14.0)}px board-local-icon
                            } else if provider == 1 {
                                icon size:{px(14.0)}px board-github-icon
                            } else {
                                icon size:{px(14.0)}px board-gitlab-icon
                            }
                            Label text:(name.clone()) role:(TextRole::Label)
                        }
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

/// The new task form, opened from the board header: title and body cells,
/// the issue repository choice and its actions.
#[component]
pub(crate) fn NewTaskForm(model: Model, open: State<bool>, draft: TaskDraft) -> Element {
    let TaskDraft {
        title,
        body,
        repository,
    } = draft;
    let repositories = Derived::new(move || {
        model
            .snapshot
            .get()
            .connections
            .into_iter()
            .filter(|c| {
                c.project_id == model.project.get()
                    && c.enabled
                    && c.state == ConnectionState::Ready
                    && matches!(c.kind, ConnectionKind::Repository { .. })
            })
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect::<Vec<_>>()
    });
    let blocked = Derived::new(move || {
        model.busy.get()
            || !model.connected.get()
            || title.get().trim().is_empty()
            || (model
                .selected_board()
                .is_some_and(|b| b.source != BoardSource::Local)
                && repository.get().is_none())
    });
    let create: OnPress = Rc::new(move || {
        if let Some(board) = model.selected_board() {
            model.action(Command::CreateTask {
                board_id: board.id,
                title: title.get_untracked(),
                body: body.get_untracked(),
                repository_connection_id: repository.get_untracked(),
            });
        }
    });
    view! {
        Module title:"New Task" icon:(Some(relay_ui::icons::command.create))
            note:(Some(UiText::from("Issue repository optional for local tasks"))) {
            row height:min-content stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                TextInput value:(title) label:"New task title" placeholder:"Task title" framed:false
            }
            row height:min-content stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                TextInput value:(body) label:"New task body" placeholder:"Description"
                    multiline:true height:(Some(100.0)) framed:false
            }
            row height:{px(36.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                label:"Issue repository" {
                if model.selected_board().is_some_and(|b| b.source == BoardSource::Local) {
                    row width:max-content shrink:0 {
                        CellButton label:"Local task" on-press:(press(move || repository.set(None)))
                            selected:(Derived::new(move || repository.get().is_none()))
                            rule:(Rule::Right) {
                            Label text:"Local task" role:(TextRole::Label)
                        }
                    }
                }
                for (_, connection) in { repositories.get().into_iter().map(|c| (c.0.clone(), c)).collect::<Vec<_>>() } {
                    let (id, name) = connection.clone();
                    let chosen = id.clone();
                    row width:max-content shrink:0 {
                        CellButton label:(format!("New task repository {name}"))
                            on-press:(press(move || repository.set(Some(id.clone()))))
                            selected:(Derived::new(move || repository.get().as_ref() == Some(&chosen)))
                            rule:(Rule::Right) {
                            Label text:(name.clone()) role:(TextRole::Label)
                        }
                    }
                }
            }
            row height:{px(40.0)}px gap:0px {
                row width:max-content shrink:0 {
                    PrimaryButton text:"Create task" on-press:(create) disabled:(blocked) fill:true
                }
                row width:max-content shrink:0 {
                    CellButton label:"Cancel" on-press:(press(move || open.set(false)))
                        rule:(Rule::Right) {
                        Label text:"Cancel" role:(TextRole::Label)
                    }
                }
            }
        }
    }
}

/// One board column on relay-ui's column frame, with its issue cards.
#[component]
pub(crate) fn BoardColumnView(model: Model, column: BoardColumn, scrolls: bool) -> Element {
    let id = column.id.clone();
    let column_id = column.id.clone();
    let issues = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .into_iter()
            .filter(|i| model.task_in_column(i, &id))
            .collect::<Vec<_>>()
    });
    let title_id = column_id.clone();
    let title = Derived::new(move || {
        model
            .board_columns()
            .iter()
            .find(|c| c.id == title_id)
            .map(|c| c.title.clone())
            .unwrap_or_default()
    });
    let count = Derived::new(move || issues.get().len());
    view! {
        ColumnFrame name:(title) count:(count) fill:true scrolls:(scrolls)
            head-label:(Some(format!("Column {column_id}"))) {
            for (_, issue) in { issues.get().into_iter().map(|i| (i.id.clone(), i)) } {
                IssueCard model:(model) issue:(issue.clone())
            }
        }
    }
}

/// What an issue's card shows now.
fn issue_view(model: Model, issue: &Issue) -> IssueView {
    let fixture = fixture_project(model, &issue.project_id);
    let sessions = model.sessions_for_task(&issue.id).len();
    // Fixture tasks cannot run, so they read as fixtures whatever their sessions.
    let state = if fixture {
        None
    } else {
        crate::adapt::issue_status(&model.snapshot.get(), &model.sessions_for_task(&issue.id))
    };
    IssueView {
        identifier: issue_number(issue),
        title: issue.title.clone(),
        status: state,
        labels: issue.labels.clone(),
        selected: model.issue.get().as_deref() == Some(issue.id.as_str()),
        preview: body_preview(&issue.body),
        meta: match sessions {
            0 => String::new(),
            1 => "1 session".into(),
            n => format!("{n} sessions"),
        },
        label: issue
            .reference
            .as_ref()
            .map(|r| format!("Open issue #{}", r.number))
            .unwrap_or_else(|| format!("Open local task {}", issue.title)),
        description: format!(
            "{} · {sessions} linked sessions",
            if fixture {
                "Fixture"
            } else {
                status_word(state)
            }
        ),
    }
}

/// An issue on its column: relay-ui's card, rebuilt when what it shows
/// changes.
#[component]
pub(crate) fn IssueCard(model: Model, issue: Issue) -> Element {
    let id = issue.id.clone();
    let host = view! {
        col height:min-content {}
    };
    host.switch(
        move || {
            let current = model
                .snapshot
                .get()
                .issues
                .into_iter()
                .find(|i| i.id == issue.id)
                .unwrap_or_else(|| issue.clone());
            issue_view(model, &current)
        },
        move |parent, shown| {
            let id = id.clone();
            let open: OnPress = Rc::new(move || {
                model.issue.set(Some(id.clone()));
                model.worker_approval.set(false);
            });
            parent.adopt(&view! {
                IssueCell issue:(shown.clone()) on-open:(Some(open))
            });
        },
    );
    host
}

/// A key and value row of the issue detail grid.
#[component]
fn DetailRow(key: String, value: Derived<String>) -> Element {
    view! {
        row height:min-content stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            row height:min-content width:{px(96.0)}px shrink:0
                pad:(horizontal:{px(18.0)}px vertical:{px(8.0)}px)
                stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                Label text:(key.clone()) role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                    caps:true
            }
            row height:min-content width:1fr min-width:0px
                pad:(horizontal:{px(14.0)}px vertical:{px(8.0)}px) {
                Label text:(value) role:(TextRole::Body)
            }
        }
    }
}

/// The issue inspector on relay-ui's frame: details, body, linked
/// sessions and the task editor scroll; the worker setup is fixed beneath
/// when the inspector is tall enough.
#[component]
pub(crate) fn IssueDetail(model: Model, #[prop(optional)] full: bool) -> Element {
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
    let close: OnPress = Rc::new(move || model.issue.set(None));
    let unavailable_close = close.clone();
    view! {
        col width:{if full {Dimension::Fill} else {Dimension::MaxContent}} min-width:0px shrink:1
            @layout:{move |rect: Rect| height.set(rect.size.height)} label:"Issue details" {
            if issue.get().is_none() {
                IssueFrame identifier:"" title:"Issue Unavailable" fill:(full)
                    on-close:(Some(unavailable_close.clone())) close-label:"Close issue details"
                    head-label:(Some("Issue unavailable header".to_string()))
                    head-start:(Some(view! { NavigationButton model:(model) }))
                    head-end:(Some(view! { WindowControlsSpace })) {
                    Notice model:(model)
                }
            }
            for (_, detail) in { issue.get().into_iter().map(|i| (i.id.clone(), i)) } {
                IssuePanel model:(model) issue:(detail.clone()) full:(full) anchored:(anchored)
                    close:(close.clone())
            }
        }
    }
}

/// The inspector of one issue.
#[component]
fn IssuePanel(
    model: Model,
    issue: Issue,
    full: bool,
    anchored: Derived<bool>,
    close: OnPress,
) -> Element {
    let detail_id = issue.id.clone();
    let fallback = issue.clone();
    let current = Derived::new(move || {
        model
            .snapshot
            .get()
            .issue(&detail_id)
            .cloned()
            .unwrap_or_else(|_| fallback.clone())
    });
    let fixture = Derived::new(move || {
        model
            .snapshot
            .get()
            .projects
            .iter()
            .any(|p| p.id == current.get().project_id && p.fixture)
    });
    let task_id = issue.id.clone();
    let linked = Derived::new(move || model.sessions_for_task(&task_id).len());
    let chips = view! {
        row height:min-content gap:{px(4.0)}px clip {
            for (_, label) in { current.get().labels.into_iter().map(|label| (label.clone(), label)) } {
                Chip text:(label.clone())
            }
        }
    };
    let footer = view! {
        col height:min-content shrink:0 {
            if anchored.get() {
                WorkerForm model:(model) continuation:false
            }
        }
    };
    let sessions_id = issue.id.clone();
    view! {
        IssueFrame identifier:(issue_number(&issue))
            title:(Derived::new(move || current.get().title)) fill:(full) on-close:(Some(close))
            close-label:"Close issue details" head-label:(Some("Issue header".to_string()))
            head-start:(Some(view! { NavigationButton model:(model) })) head-extra:(Some(chips))
            head-end:(Some(view! { WindowControlsSpace })) footer:(Some(footer)) {
            col height:min-content gap:0px selectable {
                Notice model:(model)
                DetailRow key:("Source".to_string())
                    value:(Derived::new(move || if fixture.get() { "Fixture issue · execution unavailable".to_string() } else { match current.get().reference { Some(r) => r.repository, None => "Local task".into() } }))
                if !fixture.get() && current.get().reference.is_some() {
                    DetailRow key:("Link".to_string())
                        value:(Derived::new(move || current.get().reference.map(|r| r.url).unwrap_or_default()))
                }
                col height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px)
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    Label
                        text:(Derived::new(move || crate::projects::task_body(&current.get().body)))
                        role:(TextRole::Body) tone:(Tone::Muted)
                }
                if !model.snapshot.get().visible_task(&current.get().id) {
                    col height:min-content label:"Issue removed from board" {
                        NoticeStrip tone:(NoticeTone::Failure)
                            text:"No longer on this board · history retained. Restore and sync before starting or continuing a worker. Active turns may finish or be stopped."
                    }
                }
                Module title:"Linked Sessions" icon:(Some(relay_ui::icons::call.spawn))
                    note:(Some(UiText::from(Derived::new(move || format!("{:02}", linked.get()))))) {
                    for (_, session) in { model.sessions_for_task(&sessions_id).into_iter().map(|s| (s.id.clone(), s)).collect::<Vec<_>>() } {
                        LinkedSession model:(model) id:(session.id.clone())
                    }
                }
                TaskEditor model:(model)
                if current.get().result.is_some() {
                    Module title:"Result" icon:(Some(relay_ui::icons::call.result)) {
                        col height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                            Label
                                text:(Derived::new(move || current.get().result.unwrap_or_default()))
                                role:(TextRole::Body)
                        }
                    }
                }
                if !anchored.get() {
                    WorkerForm model:(model) continuation:false
                }
            }
        }
    }
}

/// A linked session: its status square, title and state word in one cell.
#[component]
fn LinkedSession(model: Model, id: String) -> Element {
    let state_id = id.clone();
    let state = Derived::new(move || {
        let snapshot = model.snapshot.get();
        snapshot
            .sessions
            .iter()
            .find(|s| s.id == state_id)
            .map(|s| crate::adapt::session_status(&snapshot, s))
    });
    let title_id = id.clone();
    let title = Derived::new(move || {
        model
            .snapshot
            .get()
            .sessions
            .iter()
            .find(|s| s.id == title_id)
            .map(|s| s.title.clone())
            .unwrap_or_default()
    });
    let word = Derived::new(move || status_word(state.get()).to_string());
    view! {
        row height:{px(32.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            CellButton label:(title) description:(Some(UiText::from(word)))
                on-press:(press(move || model.open_session(id.clone()))) start:true
                horizontal-inset:(18.0) {
                StatusMark status:(state)
                row width:1fr min-width:0px height:min-content {
                    Ellipsis text:(title) role:(TextRole::Label)
                }
                StatusLine status:(state)
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
                scalar(relay_ui::tokens::ui_scale),
                color(ground.panel),
                color(ground.ink),
                color(ground.hairline),
                color(ground.page),
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

/// The worker setup: the director and resources, the policy the server
/// will apply, the prompt and Start worker, in one module.
#[component]
pub(crate) fn WorkerForm(model: Model, continuation: bool) -> Element {
    let choosing = State::new(false);
    let width = State::new(600.0f32);
    let compact = Derived::new(move || width.get() < px(320.0));
    // Which director and resources the next worker uses, in one line.
    let setup = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let director = snapshot
            .agent_configs
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
    let change = (!continuation).then(|| {
        view! {
            row width:max-content shrink:0 {
                CellButton label:"Choose director and resources"
                    on-press:(press(move || choosing.set(!choosing.get_untracked())))
                    rule:(Rule::Left) {
                    Label
                        text:(Derived::new(move || if choosing.get() { "Done".to_string() } else { "Change".to_string() }))
                        role:(TextRole::Label) weight:500
                }
            }
        }
    });
    let blocked = Derived::new(move || model.worker_gate(continuation).is_err());
    view! {
        col height:min-content min-width:0px fill:ground.panel
            stroke:(width:{px(1.0)} color:ground.rule edges:top) label:"Worker setup"
            @layout:{move |rect:Rect| width.set(rect.size.width)} {
            Module
                title:(if continuation { "Continue This Session" } else { "Start Task Worker" }.to_string())
                icon:(Some(relay_ui::icons::call.spawn)) head-end:(change) {
                if !continuation {
                    row height:min-content min-width:0px
                        pad:(horizontal:{px(18.0)}px vertical:{px(8.0)}px)
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                        description:{setup.get()} {
                        Ellipsis text:(setup) role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
                BoundedPanel limit:(Derived::new(|| px(180.0))) {
                    if choosing.get() && !continuation {
                        col height:min-content gap:0px
                            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                            for (_, director) in { model.snapshot.get().agent_configs.into_iter().filter(|d| d.project_id == model.project.get()).map(|d| (d.id.clone(), d)).collect::<Vec<_>>() } {
                                DirectorChoice model:(model) id:(director.id.clone())
                                    name:(director.name.clone())
                            }
                            WorkspaceChoices model:(model) flush:true
                        }
                    }
                    if model.worker_profile(continuation).is_ok() {
                        grid
                            cols:{GridTracks::auto_fit(GridTrack::minmax(px(80.0).into(), GridTrack::fr(1.0)))}
                            height:min-content gap:0px
                            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                            label:"Worker policy" {
                            for (_, key) in { worker_policy(model, continuation).into_iter().map(|(k, _)| (k, k)).collect::<Vec<_>>() } {
                                let field: &'static str = key;
                                col min-width:0px label:(format!("Worker policy {field}"))
                                    pad:(horizontal:{px(12.0)}px vertical:{px(8.0)}px)
                                    stroke:(width:{px(1.0)} color:ground.hairline edges:right) {
                                    Readout key:(field.to_string())
                                        value:(Derived::new(move || worker_policy(model, continuation).into_iter().find(|(k, _)| *k == field).map(|(_, v)| v).unwrap_or_default()))
                                }
                            }
                        }
                    }
                }
                row height:min-content stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    TextInput value:(model.worker_prompt) label:"Worker prompt"
                        placeholder:"Prompt for this turn…" multiline:true framed:false
                }
                if model.worker_profile(continuation).is_ok_and(|(p, _)| p.permissions.get(&Task::Implement) == Some(&Permission::Ask)) {
                    row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px)
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        WorkerApproval model:(model)
                    }
                }
                grid height:min-content align:center gap:0px
                    cols:{if compact.get() {GridTracks::new([GridTrack::fr(1.0)])} else {GridTracks::new([GridTrack::fr(1.0),GridTrack::MaxContent])}} {
                    row width:1fr min-width:0px height:min-content
                        pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                        Label text:(gate_status) role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                    row width:{if compact.get() {Dimension::Fill} else {Dimension::MaxContent}}
                        height:{px(44.0)}px {
                        PrimaryButton
                            text:(if continuation { "Send / continue" } else { "Start worker" })
                            label:(Some(UiText::from(if continuation { "Send worker prompt" } else { "Start worker" })))
                            on-press:(press(move || model.run_worker(continuation)))
                            disabled:(blocked) fill:true
                    }
                }
            }
        }
    }
}

/// One director choice of the worker setup.
#[component]
fn DirectorChoice(model: Model, id: String, name: String) -> Element {
    let chosen = id.clone();
    view! {
        row height:{px(34.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            CellButton label:(format!("Agent: {name}"))
                on-press:(press(move || { model.worker_director.set(id.clone()); model.worker_approval.set(false); }))
                selected:(Derived::new(move || model.worker_director.get() == chosen)) start:true
                horizontal-inset:(18.0) {
                Label text:(format!("Agent: {name}")) role:(TextRole::Label)
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
    let now = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
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

pub(crate) fn scope_label(scope: &AgentScope, snapshot: &Snapshot) -> String {
    match scope {
        AgentScope::Project => "All project issues".into(),
        AgentScope::Issues { issue_ids } => format!(
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
/// agent_configs. Fixture projects cannot run, so they show only the task count.
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
                crate::adapt::issue_status(&model.snapshot.get(), &model.sessions_for_task(&i.id))
            })
            .collect::<Vec<_>>()
    });
    let with_state = move |state: AgentStatus| {
        Derived::new(move || {
            format!(
                "{:02}",
                tasks.get().iter().filter(|s| **s == Some(state)).count()
            )
        })
    };
    let fixture = Derived::new(move || fixture_project(model, &model.project.get()));
    // Each director limits its own workers, so the project's ceiling is the
    // sum of those limits. Director turns are not counted.
    let workers = Derived::new(move || {
        let snapshot = model.snapshot.get();
        let (active, limit) = snapshot
            .agent_configs
            .iter()
            .filter(|d| d.project_id == model.project.get())
            .map(|d| crate::sidebar::director_capacity(&snapshot, &d.id))
            .fold((0, 0), |(a, l), (_, active, limit)| (a + active, l + limit));
        format!("{active} / {limit} active")
    });
    // Narrow strips (small windows, large scales) read as one summary cell.
    let width = State::new(0.0f32);
    let compact = Derived::new(move || width.get() < px(560.0));
    let summary = Derived::new(move || {
        let count = |state: AgentStatus| tasks.get().iter().filter(|s| **s == Some(state)).count();
        if fixture.get() {
            format!("{} tasks", tasks.get().len())
        } else {
            format!(
                "{} tasks · {} running · {} waiting · workers {}",
                tasks.get().len(),
                count(AgentStatus::Working),
                count(AgentStatus::Waiting),
                workers.get()
            )
        }
    });
    view! {
        row height:{px(32.0)}px shrink:0 stroke:(width:{px(1.0)} color:ground.rule edges:top)
            @layout:{move |rect: Rect| width.set(rect.size.width)} label:"Board summary" {
            if compact.get() {
                ReadoutCell key:"Board" value:(summary) inline:true
            }
            if !compact.get() {
                ReadoutCell key:"Tasks" inline:true
                    value:(Derived::new(move || format!("{:02}", tasks.get().len())))
            }
            if !fixture.get() && !compact.get() {
                ReadoutCell key:"Running" value:(with_state(AgentStatus::Working)) inline:true
                ReadoutCell key:"Waiting" value:(with_state(AgentStatus::Waiting)) inline:true
                ReadoutCell key:"Project Workers" value:(workers) inline:true
            }
        }
    }
}
