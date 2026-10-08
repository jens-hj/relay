use crate::styles::*;
use crate::{
    controls::{ButtonStyle, button},
    model::{Model, Page, Saved},
    theme::*,
};
use mosaic::core::theme::color;
use mosaic::prelude::*;
use relay_core::*;

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

#[component]
pub fn ProjectPage(model: Model) -> Element {
    view! {
        scroll {
            col height:min-content pad:{px(24.0)}px gap:{px(16.0)}px {
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
                } else {
                    for (_, director) in {model.snapshot.get().directors.into_iter().filter(|d|d.id==model.worker_director.get()).map(|d|(d.id.clone(),d)).collect::<Vec<_>>()} {
                        DirectorStart model:(model) director-id:(director.id.clone())
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
    view! {
        col height:min-content max-width:{px(640.0)}px gap:{px(12.0)}px {
            text font-family:sans-serif font-size:{px(13.0)}px "Name"
            input #relay.field label:"Project name" placeholder:"Project name" name
            text font-family:sans-serif font-size:{px(13.0)}px "Absolute root on server"
            input #relay.field label:"Absolute project root on server"
                placeholder:"/home/you/projects/project" root
            text font-color:ink.muted font-size:{px(12.0)}px
                "The root is on the connected server. A local board is created automatically."
            button #relay.action @click:{connections_open.set(!connections_open.get_untracked());}
                width:{px(200.0)}px max-width:100%
                label:{if connections_open.get(){"Hide connection form"}else{"Add connection"}}
                {if connections_open.get(){"Hide connection form"}else{"Add connection"}}
            if connections_open.get() {
                ConnectionForm model:(model) initial:true
            }
            for (index, _connection) in {model.project_draft.get().connections.into_iter().enumerate()} {
                let index = *index;
                let connection = Derived::new(move || model.project_draft.get().connections.get(index).cloned());
                col height:min-content gap:{px(6.0)}px {
                    text {connection.get().as_ref().map(connection_label).unwrap_or_default()}
                    button #relay.action
                        @click:{model.project_draft.update(|d| {d.connections.remove(index);});}
                        "Remove initial connection"
                }
            }
            button #relay.action
                @click:{
                    let draft = model.project_draft.get_untracked();
                    if draft.name.trim().is_empty() || !std::path::Path::new(draft.root.trim()).is_absolute() {
                        model.notice.set("Enter a name and an absolute root on the server.".into());
                    } else {
                        model.submit(Command::CreateProject {name:draft.name.trim().into(),root:draft.root.trim().into(),connections:draft.connections.clone()},model.snapshot.get_untracked().revision,Saved::CreatedProject(draft));
                    }
                }
                label:"Create project" disabled:{model.busy.get() || !model.connected.get()}
                "Create project"
        }
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
    view! {
        col height:min-content gap:{px(8.0)}px max-width:{px(640.0)}px {
            col height:min-content gap:{px(6.0)}px {
                for (index, label) in [(0,"Repository"),(1,"Directory"),(2,"GitHub board"),(3,"GitLab board")] {
                    button #relay.action @click:{kind.set(index);}
                        fill:if kind.get() == index {ink.inverse} else {surface.panel}
                        font-color:{color(if kind.get() == index {ink.on_inverse} else {ink.fg})}
                        font-weight:{if kind.get() == index {700} else {400}} (label)
                }
            }
            input #relay.field label:"Connection address"
                placeholder:{match kind.get(){0=>"Repository URL or SSH remote",1=>"Absolute directory on server",2=>"GitHub user or organization",_=>"GitLab project or group path"}}
                address
            if kind.get() == 3 {
                input #relay.field label:"GitLab host" host
                button #relay.action @click:{group.set(!group.get_untracked());}
                    {if group.get(){"Group board"}else{"Project board"}}
            }
            if kind.get() >= 2 {
                input #relay.field label:"Board number" placeholder:"Existing board number" number
            }
            text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                {if kind.get()==0 {"Repositories are cloned by the server."} else if kind.get()==1 {"Directory access follows the session execution mode."} else {"Connect an existing remote board."}}
            button #relay.action
                @click:{
                    let address = address.get_untracked().trim().to_owned();
                    let connection = match kind.get_untracked() {
                        0 if !address.is_empty() => Some(ConnectionInput::Repository {remote:address}),
                        1 if std::path::Path::new(&address).is_absolute() => Some(ConnectionInput::Directory {path:address}),
                        2 | 3 if !address.is_empty() => number.get_untracked().parse::<u64>().ok().filter(|n| *n>0).map(|number| ConnectionInput::Board {source:if kind.get_untracked()==2 {BoardSource::Github{owner:address,number,url:String::new()}} else {BoardSource::Gitlab{host:host.get_untracked(),path:address,group:group.get_untracked(),number,url:String::new()}}}),
                        _ => None,
                    };
                    if let Some(connection)=connection {if initial {model.project_draft.update(|d|d.connections.push(connection));} else {model.action(Command::AddConnection{project_id:model.project.get_untracked(),connection});}} else {model.notice.set("Enter a valid connection address and existing board number.".into());}
                }
                label:"Add connection"
                disabled:{!initial && (model.busy.get() || !model.connected.get())} "Add connection"
        }
    }
}

#[component]
fn Connections(model: Model) -> Element {
    view! {
        col height:min-content gap:{px(16.0)}px {
            for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && match &c.kind { ConnectionKind::Board{board_id} => model.snapshot.get().canonical_board_id(board_id) == board_id, _ => true }).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                let id = State::new(connection.id.clone());
                let fallback=connection.clone();
                let connection = Derived::new(move || model.snapshot.get().connections.into_iter().find(|c|c.id==id.get()).unwrap_or_else(||fallback.clone()));
                col height:min-content gap:{px(8.0)}px {
                    text font-family:{FontFamily::SansSerif} {connection.get().name}
                    text
                        {format!("{:?}{}",connection.get().state,if connection.get().enabled {""}else{" · disabled"})}
                    text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                        {match &connection.get().kind {ConnectionKind::Repository{remote,checkout,..}=>format!("{remote}\n{}",checkout.as_deref().unwrap_or("Clone pending")),ConnectionKind::Directory{path}=>path.clone(),ConnectionKind::Board{board_id}=>model.snapshot.get().boards.iter().find(|b| &b.id==board_id).map(|b|source_label(&b.source)).unwrap_or_else(||"Board unavailable".into())}}
                    text font-color:{color(status.danger)}
                        {connection.get().error.unwrap_or_default()}
                    if !connection.get().enabled {
                        button #relay.action
                            @click:{model.action(Command::RetryConnection{connection_id:id.get_untracked()});}
                            label:{format!("Restore connection {}",connection.get().name)}
                            disabled:{model.busy.get() || !model.connected.get()}
                            "Restore connection"
                    } else if matches!(connection.get().state,ConnectionState::Failed|ConnectionState::Interrupted) {
                        button #relay.action
                            @click:{model.action(Command::RetryConnection{connection_id:id.get_untracked()});}
                            disabled:{model.busy.get() || !model.connected.get()} "Retry connection"
                    }
                    if connection.get().enabled {
                        button #relay.action
                            @click:{model.action(Command::RemoveConnection{connection_id:id.get_untracked()});}
                            disabled:{model.busy.get() || !model.connected.get()}
                            "Remove connection"
                    }
                }
            }
            text font-family:sans-serif "Add connection"
            ConnectionForm model:(model) initial:false
            Operations model:(model)
        }
    }
}

#[component]
pub fn BoardActions(model: Model) -> Element {
    let title = State::new(String::new());
    let body = State::new(String::new());
    let creating = State::new(false);
    let repository = State::new(None::<String>);
    let managing = State::new(false);
    view! {
        col height:min-content gap:{px(8.0)}px {
            if model.snapshot.get().boards.iter().filter(|b| b.project_id == model.project.get() && model.snapshot.get().board_active(&b.id)).count() > 1 {
                row height:min-content gap:{px(6.0)}px label:"Boards" {
                    for (_, board) in {model.snapshot.get().boards.into_iter().filter(|b|b.project_id==model.project.get() && model.snapshot.get().board_active(&b.id)).map(|b|(b.id.clone(),b)).collect::<Vec<_>>()} {
                        let id = State::new(board.id.clone());
                        let board_name = State::new(board.name.clone());
                        button #relay.action
                            @click:{model.preferences.update(|p| {p.selected_boards.insert(model.project.get_untracked(),id.get_untracked());});model.issue.set(None);}
                            label:{format!("Select board {}",board_name.get())}
                            fill:if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.inverse} else {surface.panel}
                            font-color:{color(if model.selected_board().is_some_and(|b|b.id==id.get()) {ink.on_inverse} else {ink.fg})}
                            font-weight:{if model.selected_board().is_some_and(|b|b.id==id.get()) {700} else {400}}
                            {board_name.get()}
                    }
                }
            }
            if model.selected_board().is_some() {
                row height:min-content gap:{px(8.0)}px label:"Board actions" {
                    button #relay.action @click:{creating.set(!creating.get_untracked());}
                        "New task"
                    if model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                        button #relay.action @click:{managing.set(!managing.get_untracked());}
                            "Manage columns"
                        button #relay.action @click:{model.page.set(Page::Publish);} "Publish board"
                    }
                }
                if creating.get() {
                    input #relay.field label:"New task title" title
                    input #relay.area multiline label:"New task body" height:{px(100.0)}px body
                    text font-size:{px(12.0)}px "Issue repository (optional for local tasks)"
                    if model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                        button #relay.action @click:{repository.set(None);} "Local task"
                    }
                    for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && matches!(c.kind,ConnectionKind::Repository{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                        let connection_id=State::new(connection.id.clone());
                        let connection_name=State::new(connection.name.clone());
                        button #relay.action
                            @click:{repository.set(Some(connection_id.get_untracked()));}
                            label:{format!("New task repository {}",connection_name.get())}
                            fill:if repository.get().as_ref()==Some(&connection_id.get()) {ink.inverse} else {surface.panel}
                            font-color:{color(if repository.get().as_ref()==Some(&connection_id.get()) {ink.on_inverse} else {ink.fg})}
                            font-weight:{if repository.get().as_ref()==Some(&connection_id.get()) {700} else {400}}
                            {connection_name.get()}
                    }
                    button #relay.action
                        @click:{if let Some(board)=model.selected_board(){model.action(Command::CreateTask{board_id:board.id,title:title.get_untracked(),body:body.get_untracked(),repository_connection_id:repository.get_untracked()});}}
                        disabled:{model.busy.get() || !model.connected.get() || title.get().trim().is_empty() || (model.selected_board().is_some_and(|b|b.source!=BoardSource::Local) && repository.get().is_none())}
                        "Create task"
                }
                if managing.get() {
                    Columns model:(model) open:(managing)
                }
            }
            if model.snapshot.get().operations.iter().any(|o| o.project_id == model.project.get() && o.state != OperationState::Completed) {
                Operations model:(model)
            }
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
    view! {
        col height:min-content gap:{px(8.0)}px {
            button #relay.action @click:{editing.set(!editing.get_untracked());} "Edit task"
            if editing.get() {
                input #relay.field label:"Task title" title
                input #relay.area multiline label:"Task body" height:{px(120.0)}px body
                button #relay.action
                    @click:{if let Some(issue_id)=model.issue.get_untracked(){model.action(Command::UpdateTask{issue_id,title:title.get_untracked(),body:body.get_untracked()});}}
                    disabled:{model.busy.get() || !model.connected.get() || title.get().trim().is_empty()}
                    "Save task"
            }
            if editing.get() {
                for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                    let id = State::new(column.id.clone());
                    let column_title = State::new(column.title.clone());
                    button #relay.action
                        @click:{if let (Some(board),Some(issue_id))=(model.selected_board(),model.issue.get_untracked()){model.action(Command::MoveTask{board_id:board.id,issue_id,column_id:id.get_untracked()});}}
                        label:{format!("Move task to {}",column_title.get())}
                        disabled:{model.busy.get() || !model.connected.get()}
                        {format!("Move to {}",column_title.get())}
                }
            }
        }
    }
}

#[component]
fn Columns(model: Model, open: State<bool>) -> Element {
    let name = State::new(String::new());
    view! {
        col height:min-content gap:{px(8.0)}px {
            if open.get() && model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                input #relay.field label:"New column name" name
                button #relay.action
                    @click:{if let Some(board)=model.selected_board(){let mut columns=board.columns;columns.push(BoardColumn{id:uuid::Uuid::new_v4().to_string(),title:name.get_untracked()});model.action(Command::UpdateBoardColumns{board_id:board.id,columns});}}
                    disabled:{name.get().trim().is_empty() || model.busy.get() || !model.connected.get()}
                    "Add column"
                for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                    ColumnEditor model:(model) column:(column.clone())
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
    view! {
        col height:min-content gap:{px(6.0)}px {
            input #relay.field label:"Column title" title
            button #relay.action
                @click:{if let Some(mut board)=model.selected_board(){if let Some(c)=board.columns.iter_mut().find(|c|c.id==id.get_untracked()){c.title=title.get_untracked();}model.action(Command::UpdateBoardColumns{board_id:board.id,columns:board.columns});}}
                disabled:{title.get().trim().is_empty() || model.busy.get() || !model.connected.get()}
                "Rename column"
            button #relay.action
                @click:{if let Some(mut board)=model.selected_board(){board.columns.retain(|c|c.id!=id.get_untracked());model.action(Command::UpdateBoardColumns{board_id:board.id,columns:board.columns});}}
                disabled:{occupied.get() || model.board_columns().len()<2 || model.busy.get() || !model.connected.get()}
                "Delete empty column"
            if occupied.get() {
                text font-size:{px(12.0)}px font-color:ink.muted
                    "Move the tasks to another column before deleting this column."
            }
        }
    }
}

#[component]
fn DirectorStart(model: Model, director_id: String) -> Element {
    let id = State::new(director_id);
    let prompt = State::new(
        model
            .director_prompts
            .get_untracked()
            .get(&id.get_untracked())
            .cloned()
            .unwrap_or_default(),
    );
    Effect::new(move || {
        let value = prompt.get();
        model.director_prompts.update(|drafts| {
            drafts.insert(id.get_untracked(), value);
        });
    });
    view! {
        col height:min-content max-width:{px(640.0)}px gap:{px(12.0)}px {
            text font-family:sans-serif "Start the director conversation"
            input #relay.area multiline label:"First director prompt" height:{px(160.0)}px prompt
            button #relay.action
                @click:{let prompt=prompt.get_untracked();model.submit(Command::StartDirector{director_id:id.get_untracked(),prompt:prompt.clone(),approve_implementation:false},model.snapshot.get_untracked().revision,Saved::DirectorStart{director_id:id.get_untracked(),prompt});}
                disabled:{model.busy.get() || !model.connected.get() || prompt.get().trim().is_empty()}
                "Send first prompt"
        }
    }
}

#[component]
pub fn Operations(model: Model) -> Element {
    view! {
        col height:min-content gap:{px(8.0)}px {
            for (_, operation) in {model.snapshot.get().operations.into_iter().filter(|o|o.project_id==model.project.get() && o.state!=OperationState::Completed).map(|o|(o.id.clone(),o)).collect::<Vec<_>>()} {
                OperationCard model:(model) operation:(operation.clone())
            }
        }
    }
}
#[component]
fn OperationCard(model: Model, operation: ProjectOperation) -> Element {
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
    view! {
        col height:min-content gap:{px(6.0)}px {
            text
                {format!("{} · {:?}",match operation.get().kind{OperationKind::Clone{..}=>"Clone",OperationKind::Sync{..}=>"Sync",OperationKind::Publish{..}=>"Publish",_=>"Task update"},operation.get().state)}
            text font-color:{color(status.danger)} {operation.get().error.unwrap_or_default()}
            if matches!(operation.get().state,OperationState::Failed|OperationState::Interrupted) {
                button #relay.action
                    @click:{model.action(Command::RetryOperation{operation_id:id.get_untracked()});}
                    disabled:{model.busy.get() || !model.connected.get()} "Retry operation"
            }
            if operation.get().state==OperationState::NeedsReconciliation {
                text font-size:{px(12.0)}px
                    "Confirm the provider result before continuing. Inspect the remote board or issue; do not repeat an unknown write."
                text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                    {reconciliation_instruction(&key.get())}
                text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                    {key.get().split_once('/').and_then(|(_,id)|model.snapshot.get().issues.into_iter().find(|i|i.id==id)).map(|i|format!("{}\n{}",i.title,i.reference.map(|r|r.url).or_else(||operation.get().results.get(&format!("issue/{}",i.id)).and_then(|json|serde_json::from_str::<IssueRef>(json).ok()).map(|r|r.url)).unwrap_or_default())).unwrap_or_default()}
                if key.get()=="board" || key.get().starts_with("issue/") {
                    input #relay.field label:"Created board or issue URL" url
                    button #relay.action
                        @click:{let request=crate::project_network::RecoveryRequest{input:ReconciliationInput{operation_id:id.get_untracked(),url:url.get_untracked().trim().into()},pending:key.get_untracked(),id:uuid::Uuid::new_v4().to_string()};requested.set(Some(request.clone()));model.recovery.set(crate::project_network::RecoveryUpdate{request:Some(request.clone()),result:None});if model.recovery_requests.get_untracked().is_none_or(|sender|sender.send(request.clone()).is_err()){model.recovery.set(crate::project_network::RecoveryUpdate{request:Some(request),result:Some(Err("Cannot check the provider result. Reconnect and try again.".into()))});}}
                        disabled:{model.busy.get() || !model.connected.get() || url.get().trim().is_empty()}
                        "Check result"
                    if lookup.get().is_some() {
                        text font-size:{px(12.0)}px
                            {match lookup.get().unwrap(){Ok(result)=>result.description,Err(error)=>error}}
                    }
                    if lookup.get().is_some_and(|r|r.is_ok()) {
                        button #relay.action
                            @click:{if let Some(Ok(result))=lookup.get_untracked(){model.action(Command::ReconcileOperation{operation_id:id.get_untracked(),key:result.key,result:result.result});requested.set(None);}}
                            disabled:{model.busy.get() || !model.connected.get()}
                            "Use this result and continue"
                    }
                } else {
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "This recovery step does not support URL lookup. Ask the server administrator to inspect the provider result before continuing."
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
    view! {
        col height:min-content max-width:{px(640.0)}px gap:{px(12.0)}px {
            grid cols:{GridTracks::auto_fit(GridTrack::minmax(px(120.0).into(),GridTrack::fr(1.0)))}
                height:min-content gap:{px(8.0)}px {
                button #relay.action @click:{github.set(true);} width:fill
                    fill:if github.get() {ink.inverse} else {surface.panel}
                    font-color:{color(if github.get() {ink.on_inverse} else {ink.fg})}
                    font-weight:{if github.get() {700} else {400}} "GitHub project"
                button #relay.action @click:{github.set(false);} width:fill
                    fill:if github.get() {surface.panel} else {ink.inverse}
                    font-color:{color(if github.get() {ink.fg} else {ink.on_inverse})}
                    font-weight:{if github.get() {400} else {700}} "GitLab board"
            }
            text font-size:{px(12.0)}px font-color:ink.muted
                "Use 0 to create a new destination, or enter an existing board number."
            if !github.get() {
                input #relay.field label:"Publish GitLab host" host
                button #relay.action @click:{group.set(!group.get_untracked());}
                    {if group.get(){"Group"}else{"Project"}}
            }
            input #relay.field label:"Destination owner or path" path
            input #relay.field label:"Destination number (0 creates new)" number
            input #relay.field label:"Destination title" title
            col height:min-content gap:{px(8.0)}px {
                button #relay.action
                    @click:{if let Some(source)=source.get_untracked(){model.discover_destination(source);}}
                    label:"Read destination statuses"
                    disabled:{!model.connected.get() || path.get().trim().is_empty() || source.get().is_none()}
                    "Read destination statuses"
                if discovered.get().is_some() {
                    text font-size:{px(12.0)}px font-color:{color(ink.muted)}
                        {match discovered.get().unwrap(){Ok(metadata)=>format!("Destination: {}",metadata.name),Err(error)=>error.clone()}}
                }
                if model.discovery.get().source==source.get() && model.discovery.get().result.is_none() {
                    text font-size:{px(12.0)}px font-color:ink.muted
                        "Reading destination statuses…"
                }
            }
            text font-size:{px(12.0)}px font-color:ink.muted
                "Map each local column to a destination status or list."
            for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                let id = State::new(column.id.clone());
                let column_title = State::new(column.title.clone());
                text font-family:{FontFamily::SansSerif} {column_title.get()}
                for (_, choice) in {choices.get().into_iter().map(|c|(c.id.clone(),c))} {
                    let choice_id=State::new(choice.id.clone());
                    let choice_title=Derived::new(move || choices.get().iter().find(|c|c.id==choice_id.get()).map(|c|c.title.clone()).unwrap_or_default());
                    button #relay.action
                        @click:{mappings.update(|m|{m.insert(id.get_untracked(),choice_id.get_untracked());});}
                        label:{format!("Map {} to {}",column_title.get(),choice_title.get())}
                        fill:if mappings.get().get(&id.get())==Some(&choice_id.get()) {ink.inverse} else {surface.panel}
                        font-color:{color(if mappings.get().get(&id.get())==Some(&choice_id.get()) {ink.on_inverse} else {ink.fg})}
                        font-weight:{if mappings.get().get(&id.get())==Some(&choice_id.get()) {700} else {400}}
                        {choice_title.get()}
                }
            }
            for (_, issue) in {model.snapshot.get().issues.into_iter().filter(|i|model.board_columns().iter().any(|c|model.task_in_column(i,&c.id))).map(|i|(i.id.clone(),i)).collect::<Vec<_>>()} {
                let issue_id = State::new(issue.id.clone());
                let issue_title = State::new(issue.title.clone());
                col height:min-content gap:{px(6.0)}px {
                    text {issue_title.get()}
                    for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && matches!(c.kind,ConnectionKind::Repository{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                        let connection_id = State::new(connection.id.clone());
                        let connection_name = State::new(connection.name.clone());
                        button #relay.action
                            @click:{repositories.update(|r|{r.insert(issue_id.get_untracked(),connection_id.get_untracked());});}
                            label:{format!("Issue repository {} for {}",connection_name.get(),issue_title.get())}
                            fill:if repositories.get().get(&issue_id.get())==Some(&connection_id.get()) {ink.inverse} else {surface.panel}
                            font-color:{color(if repositories.get().get(&issue_id.get())==Some(&connection_id.get()) {ink.on_inverse} else {ink.fg})}
                            font-weight:{if repositories.get().get(&issue_id.get())==Some(&connection_id.get()) {700} else {400}}
                            {connection_name.get()}
                    }
                }
            }
            button #relay.action
                @click:{
                    if let (Some(board),Ok(number))=(model.selected_board(),number.get_untracked().parse::<u64>()) {
                        let columns:Vec<_>=board.columns.iter().map(|c|ColumnMapping{local_id:c.id.clone(),remote_id:mappings.get_untracked().get(&c.id).cloned().unwrap_or_default()}).collect();
                        let tasks:Vec<_>=model.snapshot.get_untracked().issues.iter().filter(|i|board.columns.iter().any(|c|model.task_in_column(i,&c.id))).map(|i|TaskPublication{issue_id:i.id.clone(),repository_connection_id:repositories.get_untracked().get(&i.id).cloned().unwrap_or_default()}).collect();
                        if path.get_untracked().trim().is_empty() || title.get_untracked().trim().is_empty() || columns.iter().any(|c|!choices.get_untracked().iter().any(|choice|choice.id==c.remote_id)) || tasks.iter().any(|t|t.repository_connection_id.is_empty()) {model.notice.set("Choose a destination, column mappings and a repository for every task.".into());} else {
                            let source=if github.get_untracked(){BoardSource::Github{owner:path.get_untracked(),number,url:String::new()}}else{BoardSource::Gitlab{host:host.get_untracked(),group:group.get_untracked(),path:path.get_untracked(),number,url:String::new()}};
                            model.action(Command::PublishBoard{board_id:board.id,target:PublishTarget{source,name:title.get_untracked()},columns,tasks});
                        }
                    } else {model.notice.set("Enter a destination board number, or 0 for a new board.".into());}
                }
                label:"Confirm board publication"
                disabled:{model.busy.get() || !model.connected.get() || model.selected_board().is_none_or(|b|b.source!=BoardSource::Local) || model.snapshot.get().operations.iter().any(|o|matches!(&o.kind,OperationKind::Publish{board_id:id,..} if id==&board_id.get()) && matches!(o.state,OperationState::Pending|OperationState::Running|OperationState::NeedsReconciliation))}
                "Publish board"
            Operations model:(model)
        }
    }
}

#[component]
pub fn WorkspaceChoices(model: Model) -> Element {
    let open = State::new(false);
    view! {
        col height:min-content gap:{px(6.0)}px {
            button #relay.action @click:{open.set(!open.get_untracked());}
                label:"Choose session resources" "Workspace resources"
            if open.get() {
                text font-size:{px(12.0)}px font-color:ink.muted
                    "Automatic selects all ready repositories and directories on the server. Selection is fixed for subsequent turns."
                button #relay.action @click:{model.workspace_selection.set(None);}
                    fill:if model.workspace_selection.get().is_none() {ink.inverse} else {surface.panel}
                    font-color:{color(if model.workspace_selection.get().is_none() {ink.on_inverse} else {ink.fg})}
                    font-weight:{if model.workspace_selection.get().is_none() {700} else {400}}
                    "Automatic resources"
                for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && !matches!(c.kind,ConnectionKind::Board{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                    let id=State::new(connection.id.clone());
                    let name=State::new(connection.name.clone());
                    button #relay.action
                        @click:{let all=model.snapshot.get_untracked().connections.iter().filter(|c|c.project_id==model.project.get_untracked() && c.enabled && c.state==ConnectionState::Ready && !matches!(c.kind,ConnectionKind::Board{..})).map(|c|c.id.clone()).collect();model.workspace_selection.update(|selected|{let ids=selected.get_or_insert(all);if ids.contains(&id.get_untracked()){ids.retain(|c|c!=&id.get_untracked());}else{ids.push(id.get_untracked());}});}
                        label:{format!("Workspace resource {}",name.get())}
                        fill:if model.workspace_selection.get().is_none_or(|ids|ids.contains(&id.get())) {ink.inverse} else {surface.panel}
                        font-color:{color(if model.workspace_selection.get().is_none_or(|ids|ids.contains(&id.get())) {ink.on_inverse} else {ink.fg})}
                        font-weight:{if model.workspace_selection.get().is_none_or(|ids|ids.contains(&id.get())) {700} else {400}}
                        {name.get()}
                }
            }
        }
    }
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
