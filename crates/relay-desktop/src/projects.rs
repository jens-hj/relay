use crate::model::{Model, Page, Saved};
use crate::panels::{BoundedPanel, BoundedPanelProps};
use crate::ui::{Readout, ReadoutProps, press};
use mosaic::prelude::*;
use relay_core::*;
use relay_ui::Live;
use relay_ui::prelude::{
    AgentStatus, CellButton, CellButtonProps, Ellipsis, EllipsisProps, Label, LabelProps, OnPress,
    PrimaryButton, PrimaryButtonProps, Rule, Segments, SegmentsProps, StatusSquare,
    StatusSquareProps, TextRole, Tone, UiText, Wash, WashProps, live_reach,
};
use relay_ui::surfaces::{
    Field, FieldProps, Module, ModuleProps, RemoveCell, RemoveCellProps, TextInput, TextInputProps,
};
use relay_ui::tokens::{Signal, ground, px};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProjectDraft {
    pub name: String,
    pub root: String,
    pub connections: Vec<ConnectionInput>,
    pub connection_form: ConnectionDraft,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionDraft {
    pub kind: usize,
    pub address: String,
    pub host: String,
    pub number: String,
    pub group: bool,
}
impl Default for ConnectionDraft {
    fn default() -> Self {
        Self {
            kind: 0,
            address: String::new(),
            host: "gitlab.com".into(),
            number: String::new(),
            group: false,
        }
    }
}

#[derive(Clone)]
pub struct PublishDraft {
    pub github: bool,
    pub group: bool,
    pub host: String,
    pub path: String,
    pub number: String,
    pub title: String,
    pub mappings: std::collections::BTreeMap<String, String>,
    pub repositories: std::collections::BTreeMap<String, String>,
}
impl Default for PublishDraft {
    fn default() -> Self {
        Self {
            github: true,
            group: false,
            host: "gitlab.com".into(),
            path: String::new(),
            number: "0".into(),
            title: String::new(),
            mappings: Default::default(),
            repositories: Default::default(),
        }
    }
}

/// A full-width action cell over a hairline; `selected` marks the current
/// choice of a list.
#[component]
pub(crate) fn ActionCell(
    label: UiText,
    on_press: OnPress,
    /// The visible text; the label by default.
    #[prop(optional)]
    text: Option<UiText>,
    #[prop(default = Live::fixed(false))] disabled: Live<bool>,
    #[prop(default = Live::fixed(false))] selected: Live<bool>,
) -> Element {
    let text = text.unwrap_or_else(|| label.clone());
    view! {
        row height:{px(36.0)}px shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            CellButton label:(label) on-press:(on_press) disabled:(disabled) selected:(selected)
                start:true horizontal-inset:(18.0) {
                Label text:(text) role:(TextRole::Label) weight:500
            }
        }
    }
}

/// A muted note in a padded cell, over a hairline unless it ends its
/// section.
#[component]
pub(crate) fn Caption(
    text: UiText,
    #[prop(default = true)] rule: bool,
    #[prop(default = Tone::Muted)] tone: Tone,
) -> Element {
    view! {
        row height:min-content pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px)
            stroke:(width:{px(if rule {1.0} else {0.0})} color:ground.hairline edges:bottom) {
            Label text:(text) role:(TextRole::Caption) tone:(tone)
        }
    }
}

/// A text input filling its ruled cell.
#[component]
pub(crate) fn InputCell(
    value: State<String>,
    label: String,
    #[prop(default = UiText::fixed(String::new()))] placeholder: UiText,
    #[prop(default = false)] multiline: bool,
    #[prop(optional)] height: Option<f32>,
) -> Element {
    view! {
        row height:min-content shrink:0
            stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            TextInput value:(value) label:(label) placeholder:(placeholder) multiline:(multiline)
                height:(height) framed:false
        }
    }
}

#[component]
pub fn ProjectPage(model: Model) -> Element {
    view! {
        scroll {
            col height:min-content gap:0px {
                if model.page.get() == Page::NewProject {
                    ProjectSetup model:(model)
                } else if model.page.get() == Page::Connections {
                    for (_, _project) in {model.snapshot.get().projects.into_iter().filter(|p|p.id==model.project.get()).map(|p|(p.id.clone(),p)).collect::<Vec<_>>()} {
                        Connections model:(model)
                    }
                } else if model.page.get() == Page::Publish {
                    for (_, _board) in {model.selected_board().into_iter().map(|b|(b.id.clone(),b)).collect::<Vec<_>>()} {
                        Publish model:(model)
                    }
                }
            }
        }
    }
}

#[component]
fn ProjectSetup(model: Model) -> Element {
    let name = State::new(model.project_draft.get_untracked().name);
    let root = State::new(model.project_draft.get_untracked().root);
    let connections_open = State::new(
        !model
            .project_draft
            .get_untracked()
            .connection_form
            .address
            .is_empty(),
    );
    Effect::new(move || {
        model.project_draft.update(|d| {
            d.name = name.get();
            d.root = root.get();
        })
    });
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    let create = press(move || {
        let draft = model.project_draft.get_untracked();
        if let Some(problem) = setup_problem(&draft) {
            model.notice.set(problem.into());
        } else {
            model.submit(
                Command::CreateProject {
                    name: draft.name.trim().into(),
                    root: draft.root.trim().into(),
                    connections: draft.connections.clone(),
                },
                model.snapshot.get_untracked().revision,
                Saved::CreatedProject(draft),
            );
        }
    });
    let form_label = Derived::new(move || {
        if connections_open.get() {
            "Hide connection form".to_string()
        } else {
            "Add connection".to_string()
        }
    });
    view! {
        col height:min-content gap:0px {
            Module title:"Project" icon:(Some(relay_ui::icons::command.project)) {
                Field label:"Name" name:(Some("Project name".to_string())) value:(name)
                    placeholder:"Project name"
                Field label:"Root on server"
                    name:(Some("Absolute project root on server".to_string())) value:(root)
                    placeholder:"/home/you/projects/project"
                Caption
                    text:"The root is on the connected server. A local board is created automatically."
                    rule:false
            }
            Module title:"Connections" icon:(Some(relay_ui::icons::mark.link)) {
                for (index, _connection) in {model.project_draft.get().connections.into_iter().enumerate()} {
                    let index = *index;
                    let connection = Derived::new(move || model.project_draft.get().connections.get(index).cloned());
                    row height:{px(40.0)}px align:center gap:0px
                        stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                        row width:1fr min-width:0px height:min-content
                            pad:(horizontal:{px(18.0)}px vertical:0px) {
                            Label
                                text:(Derived::new(move || connection.get().as_ref().map(connection_label).unwrap_or_default()))
                                role:(TextRole::Body)
                        }
                        row width:max-content shrink:0 {
                            CellButton label:"Remove initial connection" rule:(Rule::Left)
                                on-press:(press(move || model.project_draft.update(|d| { d.connections.remove(index); }))) {
                                Label text:"Remove" role:(TextRole::Label) weight:500
                            }
                        }
                    }
                }
                ActionCell label:(form_label)
                    on-press:(press(move || connections_open.set(!connections_open.get_untracked())))
                if connections_open.get() {
                    ConnectionForm model:(model) initial:true
                }
            }
            row height:{px(44.0)}px {
                PrimaryButton text:"Create project" on-press:(create) disabled:(blocked) fill:true
            }
        }
    }
}

/// Whether `path` is absolute on the server. The server runs on Unix, so
/// this must not use `std::path::Path::is_absolute`, which follows the
/// client's platform: in the browser (wasm32-unknown-unknown) and on Windows
/// it rejects every Unix path.
pub(crate) fn server_absolute(path: &str) -> bool {
    path.starts_with('/')
}

/// What blocks creating the drafted project, naming only the failing fields.
pub(crate) fn setup_problem(draft: &ProjectDraft) -> Option<&'static str> {
    match (
        draft.name.trim().is_empty(),
        server_absolute(draft.root.trim()),
    ) {
        (false, true) => None,
        (true, true) => Some("Enter a project name."),
        (false, false) => Some("Enter an absolute root on the server."),
        (true, false) => Some("Enter a name and an absolute root on the server."),
    }
}

fn connection_label(input: &ConnectionInput) -> String {
    match input {
        ConnectionInput::Repository { remote } => format!("Repository · {remote}"),
        ConnectionInput::Directory { path } => format!("Directory · {path}"),
        ConnectionInput::Board { source } => source_label(source),
    }
}
fn source_label(source: &BoardSource) -> String {
    match source {
        BoardSource::Local => "Local board".into(),
        BoardSource::Github { owner, number, .. } => format!("GitHub · {owner} · {number}"),
        BoardSource::Gitlab {
            host,
            path,
            number,
            group,
            ..
        } => format!(
            "GitLab · {host}/{path} · {} · {number}",
            if *group { "group" } else { "project" }
        ),
    }
}

#[component]
fn ConnectionForm(model: Model, initial: bool) -> Element {
    let draft = if initial {
        model.project_draft.get_untracked().connection_form
    } else {
        ConnectionDraft::default()
    };
    let kind = State::new(draft.kind);
    let address = State::new(draft.address);
    let host = State::new(draft.host);
    let number = State::new(draft.number);
    let group = State::new(draft.group);
    if initial {
        Effect::new(move || {
            let form = ConnectionDraft {
                kind: kind.get(),
                address: address.get(),
                host: host.get(),
                number: number.get(),
                group: group.get(),
            };
            model.project_draft.update(|d| d.connection_form = form);
        });
    }
    let select: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot| kind.set(slot));
    let picker_width = State::new(0.0f32);
    let add = press(move || {
        let address = address.get_untracked().trim().to_owned();
        let connection = match kind.get_untracked() {
            0 if !address.is_empty() => Some(ConnectionInput::Repository { remote: address }),
            1 if server_absolute(&address) => Some(ConnectionInput::Directory { path: address }),
            2 | 3 if !address.is_empty() => number
                .get_untracked()
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .map(|number| ConnectionInput::Board {
                    source: if kind.get_untracked() == 2 {
                        BoardSource::Github {
                            owner: address,
                            number,
                            url: String::new(),
                        }
                    } else {
                        BoardSource::Gitlab {
                            host: host.get_untracked(),
                            path: address,
                            group: group.get_untracked(),
                            number,
                            url: String::new(),
                        }
                    },
                }),
            _ => None,
        };
        if let Some(connection) = connection {
            if initial {
                model
                    .project_draft
                    .update(|d| d.connections.push(connection));
            } else {
                model.action(Command::AddConnection {
                    project_id: model.project.get_untracked(),
                    connection,
                });
            }
        } else {
            model
                .notice
                .set("Enter a valid connection address and existing board number.".into());
        }
    });
    let blocked = Derived::new(move || !initial && (model.busy.get() || !model.connected.get()));
    view! {
        col height:min-content gap:0px {
            row height:{px(40.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                @layout:{move |rect:Rect| picker_width.set(rect.size.width)} {
                scroll width:fill {
                    Segments name:("Connection type".to_string())
                        options:(vec!["Repository".into(),"Directory".into(),"GitHub board".into(),"GitLab board".into()])
                        selected:(relay_ui::Live::from(Derived::new(move || kind.get())))
                        on-select:(select)
                } as picker_scroll
                { picker_scroll.content().style_dyn(move || Style::column().width(picker_width.get().max(px(448.0))).height(px(40.0)).shrink(0.0)); }
            }
            InputCell value:(address) label:"Connection address"
                placeholder:(Derived::new(move || match kind.get() {0=>"Repository URL or SSH remote",1=>"Absolute directory on server",2=>"GitHub user or organization",_=>"GitLab project or group path"}.to_string()))
            if kind.get() == 3 {
                InputCell value:(host) label:"GitLab host"
                ActionCell
                    label:(Derived::new(move || if group.get() {"Group board".to_string()} else {"Project Board".to_string()}))
                    on-press:(press(move || group.set(!group.get_untracked())))
            }
            if kind.get() >= 2 {
                InputCell value:(number) label:"Board number" placeholder:"Existing board number"
            }
            Caption
                text:(Derived::new(move || if kind.get()==0 {"Repositories are cloned by the server.".to_string()} else if kind.get()==1 {"Directory access follows the session execution mode.".to_string()} else {"Connect an existing remote board.".to_string()}))
            ActionCell label:"Add connection" on-press:(add) disabled:(blocked)
        }
    }
}

/// One project connection: its name and state, where it points, its error
/// and its actions.
#[component]
fn ConnectionRow(model: Model, connection: ProjectConnection) -> Element {
    let id = connection.id.clone();
    let fallback = connection.clone();
    let current_id = id.clone();
    let connection = Derived::new(move || {
        model
            .snapshot
            .get()
            .connections
            .into_iter()
            .find(|c| c.id == current_id)
            .unwrap_or_else(|| fallback.clone())
    });
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    let editing = State::new(false);
    let name = State::new(connection.get_untracked().name);
    let edit = press(move || {
        name.set(connection.get_untracked().name);
        editing.set(true);
    });
    let update_id = id.clone();
    let save = press(move || {
        model.action(Command::UpdateConnection {
            connection_id: update_id.clone(),
            name: name.get_untracked(),
        });
        editing.set(false);
    });
    let retry_id = id.clone();
    let retry = press(move || {
        model.action(Command::RetryConnection {
            connection_id: retry_id.clone(),
        })
    });
    let remove = press(move || {
        model.action(Command::RemoveConnection {
            connection_id: id.clone(),
        })
    });
    let where_to = Derived::new(move || match &connection.get().kind {
        ConnectionKind::Repository {
            remote, checkout, ..
        } => format!(
            "{remote}\n{}",
            checkout.as_deref().unwrap_or("Clone pending")
        ),
        ConnectionKind::Directory { path } => path.clone(),
        ConnectionKind::Board { board_id } => model
            .snapshot
            .get()
            .boards
            .iter()
            .find(|b| &b.id == board_id)
            .map(|b| source_label(&b.source))
            .unwrap_or_else(|| "Board unavailable".into()),
    });
    view! {
        col height:min-content gap:0px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
            row height:min-content gap:0px
                label:{format!("Connection row {}", connection.get().id)} {
                col width:1fr min-width:0px height:min-content gap:{px(6.0)}px
                    pad:(horizontal:{px(18.0)}px vertical:{px(12.0)}px) {
                    row height:min-content align:center gap:{px(8.0)}px {
                        row width:1fr min-width:0px height:min-content {
                            Label text:(Derived::new(move || connection.get().name))
                                role:(TextRole::Label) weight:600
                        }
                        text font-family:monospace font-size:{px(10.5)}px
                            font-color:{mosaic::core::theme::color(if !connection.get().enabled { ground.muted } else { match connection.get().state { ConnectionState::Ready => relay_ui::tokens::status.full_steam, ConnectionState::Failed => relay_ui::tokens::status.failed, ConnectionState::Interrupted => relay_ui::tokens::status.needs_you, ConnectionState::Pending => ground.muted } })}
                            {format!("{:?}{}", connection.get().state, if connection.get().enabled {""} else {" · disabled"})}
                    }
                    Label text:(where_to) role:(TextRole::Caption) tone:(Tone::Muted)
                    if connection.get().error.is_some() {
                        Label
                            text:(Derived::new(move || connection.get().error.unwrap_or_default()))
                            role:(TextRole::Caption)
                            tone:(Tone::Signal(relay_ui::tokens::Signal::Failed))
                    }
                }
                row width:max-content shrink:0 {
                    CellButton label:{format!("Edit connection {}", connection.get().name)}
                        on-press:(edit.clone()) disabled:(blocked) rule:(Rule::Left) {
                        { parent.adopt(&relay_ui::icons::icon_mark(relay_ui::icons::mark.edit, 14.0)); }
                        Label text:"Edit" role:(TextRole::Label) weight:500
                    }
                }
                if connection.get().enabled {
                    RemoveCell label:"Remove connection" on-press:(remove.clone())
                        disabled:(blocked)
                }
            }
            if editing.get() {
                col height:min-content stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                    Field label:"Name" value:(name) name:(Some("Connection name".to_string()))
                    Caption text:"The connection location stays the same."
                    row height:{px(36.0)}px {
                        CellButton label:"Save connection name" on-press:(save.clone())
                            disabled:(Derived::new(move || blocked.get() || name.get().trim().is_empty())) {
                            Label text:"Save name" role:(TextRole::Label) weight:500
                        }
                        CellButton label:"Cancel connection edit"
                            on-press:(press(move || editing.set(false))) rule:(Rule::Left) {
                            Label text:"Cancel" role:(TextRole::Label) weight:500
                        }
                    }
                }
            }
            if !connection.get().enabled {
                row height:{px(36.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                    CellButton
                        label:(Derived::new(move || format!("Restore connection {}", connection.get().name)))
                        on-press:(retry.clone()) disabled:(blocked) start:true
                        horizontal-inset:(18.0) {
                        Label text:"Restore connection" role:(TextRole::Label) weight:500
                    }
                }
            } else if matches!(connection.get().state, ConnectionState::Failed | ConnectionState::Interrupted) {
                row height:{px(36.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                    CellButton label:"Retry connection" on-press:(retry.clone()) disabled:(blocked)
                        start:true horizontal-inset:(18.0) {
                        Label text:"Retry connection" role:(TextRole::Label) weight:500
                    }
                }
            }
        }
    }
}

#[component]
fn Connections(model: Model) -> Element {
    view! {
        col height:min-content gap:0px {
            Module title:"Connections" icon:(Some(relay_ui::icons::mark.link)) {
                for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && match &c.kind { ConnectionKind::Board{board_id} => model.snapshot.get().canonical_board_id(board_id) == board_id, _ => true }).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                    ConnectionRow model:(model) connection:(connection.clone())
                }
            }
            Module title:"New Connection" icon:(Some(relay_ui::icons::mark.link)) {
                ConnectionForm model:(model) initial:false
            }
            Module title:"Nix Commands" icon:(Some(relay_ui::icons::call.run)) {
                NixCommandsSetting model:(model)
            }
            Module title:"Worker Execution Approval" icon:(Some(relay_ui::icons::call.spawn)) {
                SpawnExecutionApprovalSetting model:(model)
            }
            Operations model:(model)
        }
    }
}

/// Whether the project's agents may run `nix develop --command` on the
/// server, outside their sandbox.
#[component]
fn NixCommandsSetting(model: Model) -> Element {
    let selected = Derived::new(move || {
        let project = model.project.get();
        let mode = model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| p.nix_commands)
            .unwrap_or_default();
        NixCommands::ALL.iter().position(|m| *m == mode).unwrap_or(0)
    });
    let choose: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot: usize| {
        model.action(Command::SetNixCommands {
            project_id: model.project.get_untracked(),
            mode: NixCommands::ALL[slot],
        })
    });
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    view! {
        col height:min-content gap:0px {
            row height:{px(40.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Segments name:("Nix commands".to_string())
                    options:(NixCommands::ALL.iter().map(|m| m.label().to_string()).collect::<Vec<_>>())
                    tones:(vec![ground.ink, relay_ui::tokens::Signal::NeedsYou.token(), relay_ui::tokens::Signal::Failed.token()])
                    selected:(relay_ui::Live::from(selected)) on-select:(choose)
                    disabled:(relay_ui::Live::from(blocked))
            }
            Caption rule:false
                text:"Agents can run nix develop --command in their assigned workspaces on the server, outside their sandbox and with the server user's access. Ask needs your approval for each command. Agents also need Verify and Implement permission."
        }
    }
}

/// When agents need your approval for the execution mode of a worker they
/// start with spawn_agent.
#[component]
fn SpawnExecutionApprovalSetting(model: Model) -> Element {
    let selected = Derived::new(move || {
        let project = model.project.get();
        let policy = model
            .snapshot
            .get()
            .projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| p.spawn_execution_approval)
            .unwrap_or_default();
        SpawnExecutionApproval::ALL
            .iter()
            .position(|p| *p == policy)
            .unwrap_or(0)
    });
    let choose: std::rc::Rc<dyn Fn(usize)> = std::rc::Rc::new(move |slot: usize| {
        model.action(Command::SetSpawnExecutionApproval {
            project_id: model.project.get_untracked(),
            policy: SpawnExecutionApproval::ALL[slot],
        })
    });
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    view! {
        col height:min-content gap:0px {
            row height:{px(40.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                Segments name:("Worker execution approval".to_string())
                    options:(SpawnExecutionApproval::ALL.iter().map(|p| p.label().to_string()).collect::<Vec<_>>())
                    tones:(vec![ground.ink, ground.ink, relay_ui::tokens::Signal::Failed.token()])
                    selected:(relay_ui::Live::from(selected)) on-select:(choose)
                    disabled:(relay_ui::Live::from(blocked))
            }
            Caption rule:false
                text:"Workers that agents start run in their parent's execution mode unless the agent asks for another. Every choice needs your approval for a mode above the parent's. Standard also needs it for every Unrestricted Access worker. Strict needs it for every worker above Ask. Relaxed lets an Unrestricted Access agent start Unrestricted Access workers without asking. Task permissions apply in every mode."
        }
    }
}

#[component]
pub fn TaskEditor(model: Model) -> Element {
    let snapshot = model.snapshot.get_untracked();
    let issue = model
        .issue
        .get_untracked()
        .and_then(|id| snapshot.issue(&id).ok().cloned());
    let title = State::new(issue.as_ref().map(|i| i.title.clone()).unwrap_or_default());
    let body = State::new(issue.map(|i| task_body(&i.body)).unwrap_or_default());
    let editing = State::new(false);
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    let save_blocked = Derived::new(move || blocked.get() || title.get().trim().is_empty());
    let save = press(move || {
        if let Some(issue_id) = model.issue.get_untracked() {
            model.action(Command::UpdateTask {
                issue_id,
                title: title.get_untracked(),
                body: body.get_untracked(),
            });
        }
    });
    view! {
        col height:min-content gap:0px {
            ActionCell label:"Edit task"
                on-press:(press(move || editing.set(!editing.get_untracked()))) selected:(editing)
            if editing.get() {
                InputCell value:(title) label:"Task title"
                InputCell value:(body) label:"Task body" multiline:true height:(Some(120.0))
                ActionCell label:"Save task" on-press:(save.clone()) disabled:(save_blocked)
                for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                    let id = column.id.clone();
                    let column_title = column.title.clone();
                    ActionCell label:(format!("Move task to {column_title}"))
                        text:(Some(UiText::from(format!("Move to {column_title}"))))
                        disabled:(blocked)
                        on-press:(press(move || if let (Some(board), Some(issue_id)) = (model.selected_board(), model.issue.get_untracked()) { model.action(Command::MoveTask { board_id: board.id, issue_id, column_id: id.clone() }); }))
                }
            }
        }
    }
}

#[component]
pub(crate) fn Columns(model: Model, open: State<bool>) -> Element {
    let name = State::new(String::new());
    let blocked = Derived::new(move || {
        name.get().trim().is_empty() || model.busy.get() || !model.connected.get()
    });
    let add = move || {
        if let Some(board) = model.selected_board() {
            let mut columns = board.columns;
            columns.push(BoardColumn {
                id: uuid::Uuid::new_v4().to_string(),
                title: name.get_untracked(),
            });
            model.action(Command::UpdateBoardColumns {
                board_id: board.id,
                columns,
            });
        }
    };
    view! {
        col height:min-content gap:0px {
            if open.get() && model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                Module title:"Columns" icon:(Some(relay_ui::icons::command.board)) {
                    InputCell value:(name) label:"New column name" placeholder:"Column name"
                    ActionCell label:"Add column" on-press:(press(add)) disabled:(blocked)
                    for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                        ColumnEditor model:(model) column:(column.clone())
                    }
                }
            }
        }
    }
}

#[component]
fn ColumnEditor(model: Model, column: BoardColumn) -> Element {
    let id = State::new(column.id.clone());
    let title = State::new(column.title);
    let occupied = Derived::new(move || {
        model
            .snapshot
            .get()
            .issues
            .iter()
            .any(|i| model.task_in_column(i, &id.get()))
    });
    let rename_blocked = Derived::new(move || {
        title.get().trim().is_empty() || model.busy.get() || !model.connected.get()
    });
    let delete_blocked = Derived::new(move || {
        occupied.get()
            || model.board_columns().len() < 2
            || model.busy.get()
            || !model.connected.get()
    });
    let rename = press(move || {
        if let Some(mut board) = model.selected_board() {
            if let Some(c) = board
                .columns
                .iter_mut()
                .find(|c| c.id == id.get_untracked())
            {
                c.title = title.get_untracked();
            }
            model.action(Command::UpdateBoardColumns {
                board_id: board.id,
                columns: board.columns,
            });
        }
    });
    let delete = press(move || {
        if let Some(mut board) = model.selected_board() {
            board.columns.retain(|c| c.id != id.get_untracked());
            model.action(Command::UpdateBoardColumns {
                board_id: board.id,
                columns: board.columns,
            });
        }
    });
    view! {
        col height:min-content gap:0px {
            InputCell value:(title) label:"Column title"
            grid height:min-content gap:0px
                stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                cols:{GridTracks::new([GridTrack::fr(1.0), GridTrack::fr(1.0)])} {
                row height:{px(36.0)}px {
                    CellButton label:"Rename column" on-press:(rename) disabled:(rename_blocked) {
                        Label text:"Rename column" role:(TextRole::Label) weight:500
                    }
                }
                row height:{px(36.0)}px {
                    CellButton label:"Delete empty column" on-press:(delete)
                        disabled:(delete_blocked) rule:(Rule::Left) {
                        Label text:"Delete empty column" role:(TextRole::Label) weight:500
                    }
                }
            }
            if occupied.get() {
                Caption text:"Move the tasks to another column before deleting this column."
            }
        }
    }
}

/// The project's unfinished operations in server order, under a header
/// that counts them by state. Nothing is shown when there are none.
/// `separated` closes the list with the strong rule that sits above module
/// headings, for use directly over the board. `limit` bounds the list's
/// height; it scrolls beyond that.
#[component]
pub fn Operations(
    model: Model,
    #[prop(default = false)] separated: bool,
    #[prop(optional)] limit: Option<Derived<f32>>,
) -> Element {
    let limit = limit.unwrap_or_else(|| Derived::new(|| px(200.0)));
    let active = Derived::new(move || {
        let project = model.project.get();
        model
            .snapshot
            .get()
            .operations
            .into_iter()
            .filter(|o| o.project_id == project && o.state != OperationState::Completed)
            .collect::<Vec<_>>()
    });
    let summary = Derived::new(move || operations_summary(&active.get()));
    view! {
        col height:min-content shrink:0 min-width:0px gap:0px {
            if !active.get().is_empty() {
                row height:{px(28.0)}px shrink:0 gap:{px(12.0)}px align:center
                    pad:(left:{px(18.0)}px right:{px(18.0)}px top:0px bottom:0px)
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom)
                    label:"Operations" {
                    Label text:"Operations" role:(TextRole::Caption) tone:(Tone::Muted) weight:500
                        caps:true
                    row width:1fr min-width:0px height:min-content justify:end {
                        Ellipsis text:(summary) role:(TextRole::Caption) tone:(Tone::Muted)
                    }
                }
                BoundedPanel limit:(limit) {
                    col height:min-content gap:0px {
                        for (_, operation) in {active.get().into_iter().map(|o|(o.id.clone(),o)).collect::<Vec<_>>()} {
                            OperationCard model:(model) operation:(operation.clone()) history:false
                        }
                    }
                }
                if separated {
                    el width:fill height:{px(1.0)}px shrink:0 fill:ground.rule-strong nohit {}
                }
            }
        }
    }
}

/// The shared status mark of an operation: queued for pending, the
/// breathing working square for running, done, failed, amber for a result
/// the user must confirm, and the idle dash for an interrupted run.
pub(crate) fn operation_status(state: &OperationState) -> AgentStatus {
    match state {
        OperationState::Pending => AgentStatus::Queued,
        OperationState::Running => AgentStatus::Working,
        OperationState::Completed => AgentStatus::Done,
        OperationState::Failed => AgentStatus::Failed,
        OperationState::Interrupted => AgentStatus::Idle,
        OperationState::NeedsReconciliation => AgentStatus::Waiting,
    }
}

pub(crate) fn operation_state_word(state: &OperationState) -> &'static str {
    match state {
        OperationState::Pending => "Pending",
        OperationState::Running => "Running",
        OperationState::Completed => "Completed",
        OperationState::Failed => "Failed",
        OperationState::Interrupted => "Interrupted",
        OperationState::NeedsReconciliation => "Needs confirmation",
    }
}

fn operation_state_tone(state: &OperationState) -> Tone {
    match state {
        OperationState::Running => Tone::Signal(Signal::Live),
        OperationState::Failed => Tone::Signal(Signal::Failed),
        OperationState::NeedsReconciliation => Tone::Signal(Signal::NeedsYou),
        OperationState::Pending | OperationState::Completed | OperationState::Interrupted => {
            Tone::Muted
        }
    }
}

fn operation_title(kind: &OperationKind) -> &'static str {
    match kind {
        OperationKind::Clone { .. } => "Clone",
        OperationKind::Sync { .. } => "Sync",
        OperationKind::Publish { .. } => "Publish",
        OperationKind::CreateTask { .. } => "Create task",
        OperationKind::MoveTask { .. } => "Move task",
        OperationKind::EditTask { .. } => "Edit task",
    }
}

/// The operations header's count, such as `2 running · 1 failed`. Active
/// states come first, then those that need attention.
pub(crate) fn operations_summary(operations: &[ProjectOperation]) -> String {
    let count = |state: OperationState| operations.iter().filter(|o| o.state == state).count();
    [
        (count(OperationState::Running), "running"),
        (count(OperationState::Pending), "pending"),
        (count(OperationState::NeedsReconciliation), "to confirm"),
        (count(OperationState::Interrupted), "interrupted"),
        (count(OperationState::Failed), "failed"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, word)| format!("{n} {word}"))
    .collect::<Vec<_>>()
    .join(" · ")
}

/// What each state means and what the user can do next.
fn operation_state_note(state: &OperationState) -> &'static str {
    match state {
        OperationState::Pending => "Waiting to start.",
        OperationState::Running => "In progress. This updates as the server reports.",
        OperationState::Completed => "Finished.",
        OperationState::Failed => "Stopped with an error. Retry is below.",
        OperationState::Interrupted => "Stopped before it finished. Retry is below.",
        OperationState::NeedsReconciliation => {
            "The provider result is unknown. Confirm it below before continuing."
        }
    }
}

/// The journal step an operation last recorded, in plain words.
fn operation_step(key: &str) -> String {
    let what = if key == "board" {
        "Creating the remote board"
    } else if key.starts_with("issue/") {
        "Creating the remote issue"
    } else if key.starts_with("membership/") {
        "Adding the task to the board"
    } else if key.starts_with("status/") {
        "Setting the task's column"
    } else if key.starts_with("edit/") {
        "Updating the remote task"
    } else {
        return key.to_string();
    };
    format!("{what} ({key})")
}

/// The board an operation acts on, for the kinds that name one.
fn operation_board(kind: &OperationKind) -> Option<&str> {
    match kind {
        OperationKind::Sync { board_id }
        | OperationKind::Publish { board_id, .. }
        | OperationKind::MoveTask { board_id, .. }
        | OperationKind::CreateTask { board_id, .. } => Some(board_id),
        OperationKind::Clone { .. } | OperationKind::EditTask { .. } => None,
    }
}

/// Whether an unfinished operation of `project` shows `text` as its error
/// on its row. With `board`, only an operation on that board counts.
pub(crate) fn operation_shows_error(
    snapshot: &Snapshot,
    project: &str,
    board: Option<&str>,
    text: &str,
) -> bool {
    let text = text.trim();
    let board = board.map(|id| snapshot.canonical_board_id(id));
    !text.is_empty()
        && snapshot.operations.iter().any(|o| {
            o.project_id == project
                && o.state != OperationState::Completed
                && o.error.as_deref().is_some_and(|e| e.trim() == text)
                && board.is_none_or(|id| {
                    operation_board(&o.kind).is_some_and(|b| snapshot.canonical_board_id(b) == id)
                })
        })
}

/// Whether a notice repeats an error that an operation row of the open
/// project is showing right now. Operation rows render on the board (when
/// no issue is open) and on the Connections and Publish pages; elsewhere
/// the notice always stays.
pub(crate) fn notice_shown_by_operation(model: Model, text: &str) -> bool {
    let rendered = match model.page.get() {
        Page::Board => model.issue.get().is_none(),
        Page::Connections | Page::Publish => true,
        _ => false,
    };
    rendered && operation_shows_error(&model.snapshot.get(), &model.project.get(), None, text)
}

/// Labelled facts about an operation, from what the snapshot records: its
/// state, target and last journal step. The error stays on the row and the
/// provider receipts follow separately, so neither is repeated here.
pub(crate) fn operation_details(
    snapshot: &Snapshot,
    operation: &ProjectOperation,
) -> Vec<(String, String)> {
    let board = |id: &str| {
        snapshot
            .board(id)
            .map(|b| b.name.clone())
            .unwrap_or_else(|_| id.to_string())
    };
    let task = |id: &str| {
        snapshot
            .issue(id)
            .map(|i| format!("{} · {}", i.label(), i.title))
            .unwrap_or_else(|_| id.to_string())
    };
    let mut rows = vec![(
        "Status".to_string(),
        format!(
            "{}. {}",
            operation_state_word(&operation.state),
            operation_state_note(&operation.state)
        ),
    )];
    match &operation.kind {
        OperationKind::Clone { connection_id } => {
            let connection = snapshot.connections.iter().find(|c| &c.id == connection_id);
            rows.push((
                "Connection".into(),
                connection.map_or_else(|| connection_id.clone(), |c| c.name.clone()),
            ));
            if let Some(ConnectionKind::Repository {
                remote, checkout, ..
            }) = connection.map(|c| &c.kind)
            {
                rows.push(("Remote".into(), remote.clone()));
                rows.push((
                    "Checkout".into(),
                    checkout.clone().unwrap_or_else(|| "Not cloned yet".into()),
                ));
            }
        }
        OperationKind::Sync { board_id } => {
            rows.push(("Board".into(), board(board_id)));
            if let Ok(found) = snapshot.board(board_id) {
                rows.push(("Source".into(), source_label(&found.source)));
                if let BoardSource::Github { url, .. } | BoardSource::Gitlab { url, .. } =
                    &found.source
                    && !url.is_empty()
                {
                    rows.push(("Board URL".into(), url.clone()));
                }
                if found.source != BoardSource::Local {
                    rows.push((
                        "Last synced".into(),
                        crate::ui::sync_label(found.last_synced_at)
                            .trim_start_matches("Last synced ")
                            .to_string(),
                    ));
                }
            }
        }
        OperationKind::Publish {
            board_id,
            target,
            columns,
            tasks,
        } => {
            rows.push(("Board".into(), board(board_id)));
            rows.push((
                "Destination".into(),
                format!("{} · {}", target.name, source_label(&target.source)),
            ));
            rows.push(("Columns".into(), format!("{} mapped", columns.len())));
            rows.push(("Tasks".into(), format!("{} to publish", tasks.len())));
        }
        OperationKind::EditTask {
            issue_id, title, ..
        } => {
            rows.push(("Task".into(), task(issue_id)));
            rows.push(("New title".into(), title.clone()));
        }
        OperationKind::MoveTask {
            board_id,
            issue_id,
            column_id,
        } => {
            rows.push(("Task".into(), task(issue_id)));
            rows.push(("Board".into(), board(board_id)));
            let column = snapshot
                .board(board_id)
                .ok()
                .and_then(|b| b.columns.iter().find(|c| &c.id == column_id))
                .map_or_else(|| column_id.clone(), |c| c.title.clone());
            rows.push(("Column".into(), column));
        }
        OperationKind::CreateTask { board_id, issue_id } => {
            rows.push(("Task".into(), task(issue_id)));
            rows.push(("Board".into(), board(board_id)));
        }
    }
    if operation.state != OperationState::Completed
        && let Some(step) = operation.results.get("pending")
    {
        rows.push(("Last step".into(), operation_step(step)));
    }
    rows
}
/// Completed operations for the project, opened from the board actions menu.
#[component]
pub fn OperationHistory(model: Model, open: State<bool>) -> Element {
    let close = view! {
        row width:max-content shrink:0 {
            CellButton label:"Close operation history" on-press:(press(move || open.set(false)))
                rule:(Rule::Left) {
                Label text:"Close" role:(TextRole::Label) weight:500
            }
        }
    };
    view! {
        col height:min-content gap:0px label:"Completed operations" {
            Module title:"Operation History" icon:(Some(relay_ui::icons::call.result))
                head-end:(Some(close)) {
                BoundedPanel limit:(Derived::new(|| px(180.0))) {
                    col height:min-content gap:0px {
                        for (_, operation) in {model.snapshot.get().operations.into_iter().filter(|o|o.project_id==model.project.get() && o.state==OperationState::Completed).map(|o|(o.id.clone(),o)).collect::<Vec<_>>()} {
                            OperationCard model:(model) operation:(operation.clone()) history:true
                        }
                        if !model.snapshot.get().operations.iter().any(|o|o.project_id==model.project.get() && o.state==OperationState::Completed) {
                            Caption text:"No completed operations" rule:false
                        }
                    }
                }
            }
        }
    }
}
/// A recorded operation's actual destination, retaining IDs when that
/// destination is no longer in the snapshot.
fn operation_subject(snapshot: &Snapshot, operation: &ProjectOperation) -> String {
    match &operation.kind {
        OperationKind::Clone { connection_id } => snapshot
            .connections
            .iter()
            .find(|c| &c.id == connection_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| connection_id.clone()),
        OperationKind::Sync { board_id } | OperationKind::Publish { board_id, .. } => snapshot
            .board(board_id)
            .map(|b| b.name.clone())
            .unwrap_or_else(|_| board_id.clone()),
        OperationKind::CreateTask { issue_id, .. }
        | OperationKind::EditTask { issue_id, .. }
        | OperationKind::MoveTask { issue_id, .. } => snapshot
            .issue(issue_id)
            .map(|i| format!("{} · {}", i.label(), i.title))
            .unwrap_or_else(|_| issue_id.clone()),
    }
}

/// Human-readable known provider receipts. Unrecognised steps stay available
/// as recorded values in this optional diagnostic disclosure.
fn operation_results(operation: &ProjectOperation) -> Vec<(String, String, String)> {
    operation
        .results
        .iter()
        .filter(|(key, _)| key.as_str() != "pending")
        .map(|(key, value)| {
            if key == "board"
                && let Ok(source) = serde_json::from_str::<BoardSource>(value)
            {
                let url = match source {
                    BoardSource::Github { url, .. } | BoardSource::Gitlab { url, .. } => url,
                    BoardSource::Local => "Local board".into(),
                };
                return (key.clone(), "Board".into(), url);
            }
            if key.starts_with("issue/")
                && let Ok(issue) = serde_json::from_str::<IssueRef>(value)
            {
                return (
                    key.clone(),
                    format!("{} #{}", issue.repository, issue.number),
                    issue.url,
                );
            }
            (key.clone(), key.clone(), value.clone())
        })
        .collect()
}

/// One operation. Its top hairline separates it from the row above; the
/// first row of a list sits under a header that already has a rule.
#[component]
fn OperationCard(model: Model, operation: ProjectOperation, history: bool) -> Element {
    let expanded = State::new(false);
    let id = State::new(operation.id.clone());
    let fallback = operation;
    let operation = Derived::new(move || {
        model
            .snapshot
            .get()
            .operations
            .into_iter()
            .find(|o| o.id == id.get())
            .unwrap_or_else(|| fallback.clone())
    });
    let first = Derived::new(move || {
        let project = model.project.get();
        model
            .snapshot
            .get()
            .operations
            .iter()
            .find(|o| {
                o.project_id == project && (o.state == OperationState::Completed) == history
            })
            .is_some_and(|o| o.id == id.get())
    });
    let key = Derived::new(move || {
        operation
            .get()
            .results
            .get("pending")
            .cloned()
            .unwrap_or_default()
    });
    let url = State::new(String::new());
    let requested = State::new(None::<crate::project_network::RecoveryRequest>);
    Effect::new(move || {
        if requested.get().is_some_and(|r| {
            r.input.url != url.get().trim()
                || r.pending != key.get()
                || operation.get().state != OperationState::NeedsReconciliation
        }) {
            requested.set(None);
        }
    });
    let lookup = Derived::new(move || {
        let update = model.recovery.get();
        let request = requested.get()?;
        if update.request.as_ref() != Some(&request)
            || request.input.operation_id != id.get()
            || request.input.url != url.get().trim()
            || request.pending != key.get()
            || operation.get().state != OperationState::NeedsReconciliation
        {
            return None;
        }
        update.result.map(|result| {
            result.and_then(|result| {
                if result.key != key.get()
                    || result.result.is_empty()
                    || result.description.trim().is_empty()
                {
                    Err("The recovery step changed. Check the result again.".into())
                } else {
                    Ok(result)
                }
            })
        })
    });
    let failed = Tone::Signal(Signal::Failed);
    let running = Derived::new(move || operation.get().state == OperationState::Running);
    let status = Derived::new(move || operation_status(&operation.get().state));
    let title = Derived::new(move || operation_title(&operation.get().kind).to_string());
    // The state word follows the operation's live state, tinted for the
    // states that need the user's eye.
    let state_label = view! {
        row width:max-content height:min-content shrink:0 {}
    };
    state_label.switch(
        move || operation.get().state,
        |parent, state| {
            parent.adopt(&view! {
                Label text:(operation_state_word(state)) role:(TextRole::Caption)
                    tone:(operation_state_tone(state))
            });
        },
    );
    let blocked = Derived::new(move || model.busy.get() || !model.connected.get());
    let check_blocked = Derived::new(move || blocked.get() || url.get().trim().is_empty());
    let check = move || {
        let request = crate::project_network::RecoveryRequest {
            input: ReconciliationInput {
                operation_id: id.get_untracked(),
                url: url.get_untracked().trim().into(),
            },
            pending: key.get_untracked(),
            id: uuid::Uuid::new_v4().to_string(),
        };
        requested.set(Some(request.clone()));
        model.recovery.set(crate::project_network::RecoveryUpdate {
            request: Some(request.clone()),
            result: None,
        });
        if model
            .recovery_requests
            .get_untracked()
            .is_none_or(|sender| sender.send(request.clone()).is_err())
        {
            model.recovery.set(crate::project_network::RecoveryUpdate {
                request: Some(request),
                result: Some(Err(
                    "Cannot check the provider result. Reconnect and try again.".into(),
                )),
            });
        }
    };
    let reconcile = move || {
        if let Some(Ok(result)) = lookup.get_untracked() {
            model.action(Command::ReconcileOperation {
                operation_id: id.get_untracked(),
                key: result.key,
                result: result.result,
            });
            requested.set(None);
        }
    };
    let retry = press(move || {
        model.action(Command::RetryOperation {
            operation_id: id.get_untracked(),
        })
    });
    let details = view! {
        row width:max-content shrink:0 {
            CellButton label:(Derived::new(move || format!("Operation details {}", id.get())))
                on-press:(press(move || expanded.set(!expanded.get_untracked()))) rule:(Rule::Left)
                selected:(expanded) {
                Label text:"Details" role:(TextRole::Label) weight:500
            }
        }
    };
    view! {
        col height:min-content gap:0px
            stroke:(width:{if first.get() { 0.0 } else { px(1.0) }} color:ground.hairline edges:top)
            label:{format!("Operation {}", id.get())} {
            stack height:min-content clip {
                if running.get() {
                    Wash reach:(live_reach(2)) over:(ground.page)
                }
                row height:min-content gap:0px {
                    col width:1fr height:min-content min-width:0px gap:{px(3.0)}px
                        pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px) {
                        row height:min-content gap:{px(8.0)}px align:center {
                            StatusSquare status:(status)
                            Label text:(title) role:(TextRole::Body) weight:500
                            { parent.adopt(&state_label); }
                        }
                        row height:min-content pad:(left:{px(18.0)}px right:0px top:0px bottom:0px) {
                            Label
                                text:(Derived::new(move || operation_subject(&model.snapshot.get(), &operation.get())))
                                role:(TextRole::Caption) tone:(Tone::Muted)
                        }
                    }
                    { parent.adopt(&details); }
                }
            }
            if expanded.get() {
                col height:min-content gap:{px(6.0)}px
                    pad:(horizontal:{px(18.0)}px vertical:{px(10.0)}px)
                    stroke:(width:{px(1.0)} color:ground.hairline edges:top) selectable {
                    for (_, fact) in {operation_details(&model.snapshot.get(), &operation.get()).into_iter().map(|r| (r.0.clone(), r))} {
                        let field = State::new(fact.0.clone());
                        Readout key:(fact.0.clone())
                            value:(Derived::new(move || operation_details(&model.snapshot.get(), &operation.get()).into_iter().find(|r| r.0 == field.get()).map(|r| r.1).unwrap_or_default()))
                    }
                    for (_, result) in {operation_results(&operation.get()).into_iter().map(|r| (r.0.clone(), r))} {
                        let key = State::new(result.0.clone());
                        Readout key:(result.1.clone())
                            value:(Derived::new(move || operation_results(&operation.get()).into_iter().find(|r| r.0 == key.get()).map(|r| r.2).unwrap_or_default()))
                    }
                    Label text:(Derived::new(move || format!("Operation: {}", id.get())))
                        role:(TextRole::Caption) tone:(Tone::Muted)
                }
            }
            if operation.get().error.is_some_and(|e| !e.is_empty()) {
                Caption text:(Derived::new(move || operation.get().error.unwrap_or_default()))
                    tone:(failed) rule:false
            }
            if matches!(operation.get().state,OperationState::Failed|OperationState::Interrupted) {
                row height:{px(36.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                    CellButton label:"Retry operation" on-press:(retry.clone()) disabled:(blocked)
                        start:true horizontal-inset:(18.0) {
                        Label text:"Retry operation" role:(TextRole::Label) weight:500
                    }
                }
            }
            if operation.get().state==OperationState::NeedsReconciliation {
                col height:min-content gap:0px
                    stroke:(width:{px(1.0)} color:ground.hairline edges:top) {
                    Caption
                        text:"Confirm the provider result before continuing. Inspect the remote board or issue; do not repeat an unknown write."
                        tone:(Tone::Ink) rule:false
                    Caption
                        text:(Derived::new(move || reconciliation_instruction(&key.get()).to_string()))
                        rule:false
                    Caption
                        text:(Derived::new(move || key.get().split_once('/').and_then(|(_,id)|model.snapshot.get().issues.into_iter().find(|i|i.id==id)).map(|i|format!("{}\n{}",i.title,i.reference.map(|r|r.url).or_else(||operation.get().results.get(&format!("issue/{}",i.id)).and_then(|json|serde_json::from_str::<IssueRef>(json).ok()).map(|r|r.url)).unwrap_or_default())).unwrap_or_default()))
                    if key.get()=="board" || key.get().starts_with("issue/") {
                        InputCell value:(url) label:"Created board or issue URL"
                            placeholder:"https://…"
                        ActionCell label:"Check result" on-press:(press(check))
                            disabled:(check_blocked)
                        if lookup.get().is_some() {
                            Caption
                                text:(Derived::new(move || match lookup.get() { Some(Ok(result)) => result.description, Some(Err(error)) => error, None => String::new() }))
                                tone:(Tone::Ink)
                        }
                        if lookup.get().is_some_and(|r|r.is_ok()) {
                            ActionCell label:"Use this result and continue"
                                on-press:(press(reconcile)) disabled:(blocked)
                        }
                    } else {
                        Caption
                            text:"This recovery step does not support URL lookup. Ask the server administrator to inspect the provider result before continuing."
                            rule:false
                    }
                }
            }
        }
    }
}

fn reconciliation_instruction(key: &str) -> &'static str {
    if key == "board" {
        "Find the destination board created by this publish operation and enter its URL."
    } else if key.starts_with("issue/") {
        "Find the issue created for this task in its selected repository and enter its URL."
    } else if key.starts_with("membership/") {
        "Find the task's existing board item and enter its item ID. Confirm the item belongs to the selected task and destination."
    } else if key.starts_with("status/") {
        "Inspect the remote task's status. Confirm only if the requested move has already completed."
    } else if key.starts_with("edit/") {
        "Inspect the remote task's title and body. Confirm only if the requested edit has already completed."
    } else {
        "The server has not provided a recoverable step. Review the operation with the server administrator."
    }
}

#[component]
fn Publish(model: Model) -> Element {
    let board_id = State::new(model.selected_board().map(|b| b.id).unwrap_or_default());
    let draft = model
        .publish_drafts
        .get_untracked()
        .get(&board_id.get_untracked())
        .cloned()
        .unwrap_or_else(|| PublishDraft {
            title: model.selected_board().map(|b| b.name).unwrap_or_default(),
            ..Default::default()
        });
    let github = State::new(draft.github);
    let group = State::new(draft.group);
    let host = State::new(draft.host);
    let path = State::new(draft.path);
    let number = State::new(draft.number);
    let title = State::new(draft.title);
    let mappings = State::new(draft.mappings);
    let repositories = State::new(draft.repositories);
    Effect::new(move || {
        let draft = PublishDraft {
            github: github.get(),
            group: group.get(),
            host: host.get(),
            path: path.get(),
            number: number.get(),
            title: title.get(),
            mappings: mappings.get(),
            repositories: repositories.get(),
        };
        model.publish_drafts.update(|drafts| {
            drafts.insert(board_id.get_untracked(), draft);
        });
    });
    let source = Derived::new(move || {
        number.get().trim().parse::<u64>().ok().map(|number| {
            if github.get() {
                BoardSource::Github {
                    owner: path.get().trim().into(),
                    number,
                    url: String::new(),
                }
            } else {
                BoardSource::Gitlab {
                    host: host.get().trim().into(),
                    group: group.get(),
                    path: path.get().trim().into(),
                    number,
                    url: String::new(),
                }
            }
        })
    });
    let discovered = Derived::new(move || {
        let update = model.discovery.get();
        if update.source == source.get() {
            update.result
        } else {
            None
        }
    });
    let choices = Derived::new(move || {
        discovered
            .get()
            .and_then(Result::ok)
            .map(|d| d.columns)
            .unwrap_or_default()
    });
    let previous = State::new(source.get_untracked());
    Effect::new(move || {
        let current = source.get();
        if previous.get_untracked() != current {
            mappings.set(Default::default());
            previous.set(current);
        }
    });
    let destination_index = Derived::new(move || usize::from(!github.get()));
    let choose_destination: std::rc::Rc<dyn Fn(usize)> =
        std::rc::Rc::new(move |slot| github.set(slot == 0));
    let discover_blocked = Derived::new(move || {
        !model.connected.get() || path.get().trim().is_empty() || source.get().is_none()
    });
    let discover = press(move || {
        if let Some(source) = source.get_untracked() {
            model.discover_destination(source);
        }
    });
    let publish_blocked = Derived::new(move || {
        model.busy.get()
            || !model.connected.get()
            || model
                .selected_board()
                .is_none_or(|b| b.source != BoardSource::Local)
            || model.snapshot.get().operations.iter().any(|o| {
                matches!(&o.kind, OperationKind::Publish { board_id: id, .. } if id == &board_id.get())
                    && matches!(
                        o.state,
                        OperationState::Pending
                            | OperationState::Running
                            | OperationState::NeedsReconciliation
                    )
            })
    });
    let publish = press(move || {
        if let (Some(board), Ok(number)) = (
            model.selected_board(),
            number.get_untracked().parse::<u64>(),
        ) {
            let columns: Vec<_> = board
                .columns
                .iter()
                .map(|c| ColumnMapping {
                    local_id: c.id.clone(),
                    remote_id: mappings
                        .get_untracked()
                        .get(&c.id)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let tasks: Vec<_> = model
                .snapshot
                .get_untracked()
                .issues
                .iter()
                .filter(|i| board.columns.iter().any(|c| model.task_in_column(i, &c.id)))
                .map(|i| TaskPublication {
                    issue_id: i.id.clone(),
                    repository_connection_id: repositories
                        .get_untracked()
                        .get(&i.id)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            if path.get_untracked().trim().is_empty()
                || title.get_untracked().trim().is_empty()
                || columns.iter().any(|c| {
                    !choices
                        .get_untracked()
                        .iter()
                        .any(|choice| choice.id == c.remote_id)
                })
                || tasks.iter().any(|t| t.repository_connection_id.is_empty())
            {
                model.notice.set(
                    "Choose a destination, column mappings and a repository for every task.".into(),
                );
            } else {
                let source = if github.get_untracked() {
                    BoardSource::Github {
                        owner: path.get_untracked(),
                        number,
                        url: String::new(),
                    }
                } else {
                    BoardSource::Gitlab {
                        host: host.get_untracked(),
                        group: group.get_untracked(),
                        path: path.get_untracked(),
                        number,
                        url: String::new(),
                    }
                };
                model.action(Command::PublishBoard {
                    board_id: board.id,
                    target: PublishTarget {
                        source,
                        name: title.get_untracked(),
                    },
                    columns,
                    tasks,
                });
            }
        } else {
            model
                .notice
                .set("Enter a destination board number, or 0 for a new board.".into());
        }
    });
    view! {
        col height:min-content gap:0px {
            Module title:"Destination" icon:(Some(relay_ui::icons::command.board)) {
                row height:{px(40.0)}px
                    stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                    Segments name:("Destination".to_string())
                        options:(vec!["GitHub project".to_string(), "GitLab board".to_string()])
                        selected:(relay_ui::Live::from(destination_index))
                        on-select:(choose_destination)
                }
                Caption text:"Use 0 to create a new destination, or enter an existing board number."
                if !github.get() {
                    InputCell value:(host) label:"Publish GitLab host"
                    ActionCell
                        label:(Derived::new(move || if group.get() {"Group".to_string()} else {"Project".to_string()}))
                        on-press:(press(move || group.set(!group.get_untracked())))
                }
                InputCell value:(path) label:"Destination owner or path"
                InputCell value:(number) label:"Destination number (0 creates new)"
                InputCell value:(title) label:"Destination title"
                ActionCell label:"Read destination statuses" on-press:(discover.clone())
                    disabled:(discover_blocked)
                if discovered.get().is_some() {
                    Caption
                        text:(Derived::new(move || match discovered.get() { Some(Ok(metadata)) => format!("Destination: {}", metadata.name), Some(Err(error)) => error, None => String::new() }))
                }
                if model.discovery.get().source==source.get() && model.discovery.get().result.is_none() {
                    Caption text:"Reading destination statuses…"
                }
            }
            Module title:"Columns" icon:(Some(relay_ui::icons::command.board))
                note:(Some(UiText::from("Map each local column to a destination status or list."))) {
                for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                    let id = State::new(column.id.clone());
                    let column_title = State::new(column.title.clone());
                    Caption text:(column_title.get()) tone:(Tone::Ink)
                    for (_, choice) in {choices.get().into_iter().map(|c|(c.id.clone(),c))} {
                        let choice_id=State::new(choice.id.clone());
                        let choice_title=Derived::new(move || choices.get().iter().find(|c|c.id==choice_id.get()).map(|c|c.title.clone()).unwrap_or_default());
                        ActionCell
                            label:(Derived::new(move || format!("Map {} to {}", column_title.get(), choice_title.get())))
                            text:(Some(UiText::from(choice_title)))
                            selected:(Derived::new(move || mappings.get().get(&id.get()) == Some(&choice_id.get())))
                            on-press:(press(move || mappings.update(|m| { m.insert(id.get_untracked(), choice_id.get_untracked()); })))
                    }
                }
            }
            Module title:"Issue Repositories" icon:(Some(relay_ui::icons::command.project)) {
                for (_, issue) in {model.snapshot.get().issues.into_iter().filter(|i|model.board_columns().iter().any(|c|model.task_in_column(i,&c.id))).map(|i|(i.id.clone(),i)).collect::<Vec<_>>()} {
                    let issue_id = State::new(issue.id.clone());
                    let issue_title = State::new(issue.title.clone());
                    Caption text:(issue_title.get()) tone:(Tone::Ink)
                    for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && matches!(c.kind,ConnectionKind::Repository{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                        let connection_id = State::new(connection.id.clone());
                        let connection_name = State::new(connection.name.clone());
                        ActionCell
                            label:(Derived::new(move || format!("Issue repository {} for {}", connection_name.get(), issue_title.get())))
                            text:(Some(UiText::from(Derived::new(move || connection_name.get()))))
                            selected:(Derived::new(move || repositories.get().get(&issue_id.get()) == Some(&connection_id.get())))
                            on-press:(press(move || repositories.update(|r| { r.insert(issue_id.get_untracked(), connection_id.get_untracked()); })))
                    }
                }
            }
            row height:{px(44.0)}px stroke:(width:{px(1.0)} color:ground.hairline edges:bottom) {
                PrimaryButton text:"Publish Board"
                    label:(Some(UiText::from("Confirm board publication"))) on-press:(publish)
                    disabled:(publish_blocked) fill:true
            }
            Operations model:(model)
        }
    }
}

#[component]
pub fn WorkspaceChoices(model: Model, #[prop(default = false)] flush: bool) -> Element {
    let open = State::new(false);
    let _ = flush;
    let ready = move || {
        model
            .snapshot
            .get()
            .connections
            .into_iter()
            .filter(|c| {
                c.project_id == model.project.get()
                    && c.enabled
                    && c.state == ConnectionState::Ready
                    && !matches!(c.kind, ConnectionKind::Board { .. })
            })
            .collect::<Vec<_>>()
    };
    view! {
        col height:min-content gap:0px {
            ActionCell label:"Choose session resources"
                text:(Some(UiText::from("Workspace resources")))
                on-press:(press(move || open.set(!open.get_untracked()))) selected:(open)
            if open.get() {
                Caption
                    text:"Automatic selects all ready repositories and directories on the server. Selection is fixed for subsequent turns."
                ActionCell label:"Automatic resources"
                    selected:(Derived::new(move || model.workspace_selection.get().is_none()))
                    on-press:(press(move || model.workspace_selection.set(None)))
                for (_, connection) in {ready().into_iter().map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                    let id = State::new(connection.id.clone());
                    let name = State::new(connection.name.clone());
                    ActionCell
                        label:(Derived::new(move || format!("Workspace resource {}", name.get())))
                        text:(Some(UiText::from(Derived::new(move || name.get()))))
                        selected:(Derived::new(move || model.workspace_selection.get().is_none_or(|ids| ids.contains(&id.get()))))
                        on-press:(press(move || {
                            let all = ready_ids(model);
                            model.workspace_selection.update(|selected| {
                                let ids = selected.get_or_insert(all);
                                if ids.contains(&id.get_untracked()) {
                                    ids.retain(|c| c != &id.get_untracked());
                                } else {
                                    ids.push(id.get_untracked());
                                }
                            });
                        }))
                }
            }
        }
    }
}

/// The ready repository and directory connections of the open project.
fn ready_ids(model: Model) -> Vec<String> {
    model
        .snapshot
        .get_untracked()
        .connections
        .iter()
        .filter(|c| {
            c.project_id == model.project.get_untracked()
                && c.enabled
                && c.state == ConnectionState::Ready
                && !matches!(c.kind, ConnectionKind::Board { .. })
        })
        .map(|c| c.id.clone())
        .collect()
}

fn task_markers(body: &str) -> Vec<&str> {
    let mut markers = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("<!-- relay-operation:") {
        rest = &rest[start..];
        let Some(end) = rest.find("-->") else {
            break;
        };
        let marker = &rest[..end + 3];
        let payload = marker
            .strip_prefix("<!-- relay-operation:")
            .unwrap()
            .strip_suffix("-->")
            .unwrap()
            .trim();
        if let Some((operation, task)) = payload.split_once(":task:")
            && !operation.is_empty()
            && !task.is_empty()
            && !payload.contains(['\n', '\r', '<', '>'])
        {
            markers.push(marker);
        }
        rest = &rest[end + 3..];
    }
    markers
}
pub(crate) fn task_body(body: &str) -> String {
    let markers = task_markers(body);
    if markers.is_empty() {
        return body.into();
    }
    let mut visible = body.to_owned();
    for marker in markers {
        visible = visible.replace(marker, "");
    }
    visible.trim_end().into()
}
pub(crate) fn preserve_task_markers(edited: &str, original: &str) -> String {
    let mut body = edited.to_owned();
    for marker in task_markers(original) {
        if !body.contains(marker) {
            body.push_str("\n\n");
            body.push_str(marker);
        }
    }
    body
}
