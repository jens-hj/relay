use crate::{
    model::{Model, Page, Saved},
    theme::*,
};
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
    Effect::new(move || {
        model.project_draft.update(|d| {
            d.name = name.get();
            d.root = root.get();
        })
    });
    view! {
        col height:min-content max-width:{px(640.0)}px gap:{px(12.0)}px {
            input #input-field label:"Project name" placeholder:"Project name" name
            input #input-field label:"Absolute project root on server"
                placeholder:"/home/you/projects/project" root
            text font-color:muted font-size:{px(12.0)}px
                "The root is on the connected server. A local board is created automatically."
            text font-family:sans-serif "Initial connections (optional)"
            ConnectionForm model:(model) initial:true
            for (index, _connection) in {model.project_draft.get().connections.into_iter().enumerate()} {
                let index = *index;
                let connection = Derived::new(move || model.project_draft.get().connections.get(index).cloned());
                col height:min-content gap:{px(6.0)}px {
                    text {connection.get().as_ref().map(connection_label).unwrap_or_default()}
                    button #action
                        @click:{model.project_draft.update(|d| {d.connections.remove(index);});}
                        "Remove initial connection"
                }
            }
            button #action
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
                    button #action @click:{kind.set(index);}
                        fill:if kind.get() == index {accent-soft} else {raised} (label)
                }
            }
            input #input-field label:"Connection address"
                placeholder:{match kind.get(){0=>"Repository URL or SSH remote",1=>"Absolute directory on server",2=>"GitHub user or organization",_=>"GitLab project or group path"}}
                address
            if kind.get() == 3 {
                input #input-field label:"GitLab host" host
                button #action @click:{group.set(!group.get_untracked());}
                    {if group.get(){"Group board"}else{"Project board"}}
            }
            if kind.get() >= 2 {
                input #input-field label:"Board number" placeholder:"Existing board number" number
            }
            text font-size:{px(12.0)}px font-color:muted
                {if kind.get()==0 {"Repositories are cloned by the server."} else if kind.get()==1 {"Directory access follows the session execution mode."} else {"Connect an existing remote board."}}
            button #action
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
            for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get()).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                let id = State::new(connection.id.clone());
                let fallback=connection.clone();
                let connection = Derived::new(move || model.snapshot.get().connections.into_iter().find(|c|c.id==id.get()).unwrap_or_else(||fallback.clone()));
                col height:min-content gap:{px(8.0)}px {
                    text font-family:sans-serif {connection.get().name}
                    text
                        {format!("{:?}{}",connection.get().state,if connection.get().enabled {""}else{" · disabled"})}
                    text font-size:{px(12.0)}px font-color:muted
                        {match &connection.get().kind {ConnectionKind::Repository{remote,checkout,..}=>format!("{remote}\n{}",checkout.as_deref().unwrap_or("Clone pending")),ConnectionKind::Directory{path}=>path.clone(),ConnectionKind::Board{board_id}=>model.snapshot.get().boards.iter().find(|b| &b.id==board_id).map(|b|source_label(&b.source)).unwrap_or_else(||"Board unavailable".into())}}
                    text font-color:danger {connection.get().error.unwrap_or_default()}
                    if matches!(connection.get().state,ConnectionState::Failed|ConnectionState::Interrupted) {
                        button #action
                            @click:{model.action(Command::RetryConnection{connection_id:id.get_untracked()});}
                            disabled:{model.busy.get() || !model.connected.get()} "Retry connection"
                    }
                    button #action
                        @click:{model.action(Command::RemoveConnection{connection_id:id.get_untracked()});}
                        disabled:{model.busy.get() || !model.connected.get()} "Remove connection"
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
            for (_, board) in {model.snapshot.get().boards.into_iter().filter(|b|b.project_id==model.project.get()).map(|b|(b.id.clone(),b)).collect::<Vec<_>>()} {
                let id = State::new(board.id.clone());
                let board_name = State::new(board.name.clone());
                button #action
                    @click:{model.preferences.update(|p| {p.selected_boards.insert(model.project.get_untracked(),id.get_untracked());});model.issue.set(None);}
                    label:{format!("Select board {}",board_name.get())}
                    fill:if model.selected_board().is_some_and(|b|b.id==id.get()){accent-soft}else{raised}
                    {board_name.get()}
            }
            if model.selected_board().is_some() {
                grid
                    cols:{GridTracks::auto_fit(GridTrack::minmax(px(120.0).into(),GridTrack::fr(1.0)))}
                    height:min-content gap:{px(8.0)}px {
                    button #action @click:{creating.set(!creating.get_untracked());} width:fill
                        "New task"
                    if model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                        button #action @click:{managing.set(!managing.get_untracked());} width:fill
                            "Manage columns"
                        button #action @click:{model.page.set(Page::Publish);} width:fill
                            "Publish board"
                    }
                }
                if creating.get() {
                    input #input-field label:"New task title" title
                    input #area multiline label:"New task body" height:{px(100.0)}px body
                    text font-size:{px(12.0)}px "Issue repository (optional for local tasks)"
                    if model.selected_board().is_some_and(|b|b.source==BoardSource::Local) {
                        button #action @click:{repository.set(None);} "Local task"
                    }
                    for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && matches!(c.kind,ConnectionKind::Repository{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                        let connection_id=State::new(connection.id.clone());
                        let connection_name=State::new(connection.name.clone());
                        button #action @click:{repository.set(Some(connection_id.get_untracked()));}
                            label:{format!("New task repository {}",connection_name.get())}
                            fill:if repository.get().as_ref()==Some(&connection_id.get()){accent-soft}else{raised}
                            {connection_name.get()}
                    }
                    button #action
                        @click:{if let Some(board)=model.selected_board(){model.action(Command::CreateTask{board_id:board.id,title:title.get_untracked(),body:body.get_untracked(),repository_connection_id:repository.get_untracked()});}}
                        disabled:{model.busy.get() || !model.connected.get() || title.get().trim().is_empty() || (model.selected_board().is_some_and(|b|b.source!=BoardSource::Local) && repository.get().is_none())}
                        "Create task"
                }
                Columns model:(model) open:(managing)
            }
            Operations model:(model)
        }
    }
}

#[component]
pub fn TaskEditor(model: Model) -> Element {
    let issue = model
        .snapshot
        .get_untracked()
        .issues
        .into_iter()
        .find(|i| Some(&i.id) == model.issue.get_untracked().as_ref());
    let title = State::new(issue.as_ref().map(|i| i.title.clone()).unwrap_or_default());
    let body = State::new(issue.map(|i| i.body).unwrap_or_default());
    let editing = State::new(false);
    view! {
        col height:min-content gap:{px(8.0)}px {
            button #action @click:{editing.set(!editing.get_untracked());} "Edit task"
            if editing.get() {
                input #input-field label:"Task title" title
                input #area multiline label:"Task body" height:{px(120.0)}px body
                button #action
                    @click:{if let Some(issue_id)=model.issue.get_untracked(){model.action(Command::UpdateTask{issue_id,title:title.get_untracked(),body:body.get_untracked()});}}
                    disabled:{model.busy.get() || !model.connected.get() || title.get().trim().is_empty()}
                    "Save task"
            }
            if editing.get() {
                for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                    let id = State::new(column.id.clone());
                    let column_title = State::new(column.title.clone());
                    button #action
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
                input #input-field label:"New column name" name
                button #action
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
            input #input-field label:"Column title" title
            button #action
                @click:{if let Some(mut board)=model.selected_board(){if let Some(c)=board.columns.iter_mut().find(|c|c.id==id.get_untracked()){c.title=title.get_untracked();}model.action(Command::UpdateBoardColumns{board_id:board.id,columns:board.columns});}}
                disabled:{title.get().trim().is_empty() || model.busy.get() || !model.connected.get()}
                "Rename column"
            button #action
                @click:{if let Some(mut board)=model.selected_board(){board.columns.retain(|c|c.id!=id.get_untracked());model.action(Command::UpdateBoardColumns{board_id:board.id,columns:board.columns});}}
                disabled:{occupied.get() || model.board_columns().len()<2 || model.busy.get() || !model.connected.get()}
                "Delete empty column"
            if occupied.get() {
                text font-size:{px(12.0)}px font-color:muted
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
            input #area multiline label:"First director prompt" height:{px(160.0)}px prompt
            button #action
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
    let key = State::new(
        operation
            .get_untracked()
            .results
            .get("pending")
            .cloned()
            .unwrap_or_default(),
    );
    let result = State::new(String::new());
    view! {
        col height:min-content gap:{px(6.0)}px {
            text
                {format!("{} · {:?}",match operation.get().kind{OperationKind::Clone{..}=>"Clone",OperationKind::Sync{..}=>"Sync",OperationKind::Publish{..}=>"Publish",_=>"Task update"},operation.get().state)}
            text font-color:danger {operation.get().error.unwrap_or_default()}
            if matches!(operation.get().state,OperationState::Failed|OperationState::Interrupted) {
                button #action
                    @click:{model.action(Command::RetryOperation{operation_id:id.get_untracked()});}
                    disabled:{model.busy.get() || !model.connected.get()} "Retry operation"
            }
            if operation.get().state==OperationState::NeedsReconciliation {
                text font-size:{px(12.0)}px
                    "Confirm the provider result before continuing. Inspect the remote board or issue; do not repeat an unknown write."
                text
                    {operation.get().results.iter().map(|(key,value)|format!("{key}: {value}")).collect::<Vec<_>>().join("\n")}
                input #input-field label:"Provider operation key" key
                input #input-field label:"Confirmed provider result" result
                button #action
                    @click:{model.action(Command::ReconcileOperation{operation_id:id.get_untracked(),key:key.get_untracked(),result:result.get_untracked()});}
                    disabled:{model.busy.get() || !model.connected.get() || key.get().trim().is_empty() || result.get().trim().is_empty()}
                    "Confirm provider result"
            }
        }
    }
}

#[component]
fn Publish(model: Model) -> Element {
    let github = State::new(true);
    let group = State::new(false);
    let host = State::new("gitlab.com".to_owned());
    let path = State::new(String::new());
    let number = State::new("0".to_owned());
    let title = State::new(model.selected_board().map(|b| b.name).unwrap_or_default());
    let mappings = State::new(std::collections::BTreeMap::<String, String>::new());
    let repositories = State::new(std::collections::BTreeMap::<String, String>::new());
    let manual = State::new(false);
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
        if number.get().trim() == "0" {
            if github.get() {
                [
                    ("name:Todo", "Todo"),
                    ("name:In Progress", "In Progress"),
                    ("name:Done", "Done"),
                    ("name:No status", "No status"),
                ]
                .into_iter()
                .map(|(id, title)| BoardColumn {
                    id: id.into(),
                    title: title.into(),
                })
                .collect()
            } else {
                [("gitlab-open", "Open"), ("gitlab-closed", "Closed")]
                    .into_iter()
                    .map(|(id, title)| BoardColumn {
                        id: id.into(),
                        title: title.into(),
                    })
                    .collect()
            }
        } else {
            discovered
                .get()
                .and_then(Result::ok)
                .map(|d| d.columns)
                .unwrap_or_default()
        }
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
                button #action @click:{github.set(true);} width:fill
                    fill:if github.get(){accent-soft}else{raised} "GitHub project"
                button #action @click:{github.set(false);} width:fill
                    fill:if github.get(){raised}else{accent-soft} "GitLab board"
            }
            text font-size:{px(12.0)}px font-color:muted
                "Use 0 to create a new destination, or enter an existing board number."
            if !github.get() {
                input #input-field label:"Publish GitLab host" host
                button #action @click:{group.set(!group.get_untracked());}
                    {if group.get(){"Group"}else{"Project"}}
            }
            input #input-field label:"Destination owner or path" path
            input #input-field label:"Destination number (0 creates new)" number
            input #input-field label:"Destination title" title
            if number.get().trim()!="0" {
                button #action
                    @click:{if let (Some(sender),Some(source))=(model.discovery_requests.get_untracked(),source.get_untracked()){model.discovery.set(crate::project_network::DiscoveryUpdate{source:Some(source.clone()),result:None});let _=sender.send(source);}else{model.notice.set("Destination discovery is unavailable.".into());}}
                    label:"Read destination statuses"
                    disabled:{!model.connected.get() || path.get().trim().is_empty() || source.get().is_none()}
                    "Read destination statuses"
                if discovered.get().is_some() {
                    text font-size:{px(12.0)}px font-color:muted
                        {match discovered.get().unwrap(){Ok(metadata)=>format!("Destination: {}",metadata.name),Err(error)=>error.clone()}}
                }
                if model.discovery.get().source==source.get() && model.discovery.get().result.is_none() {
                    text font-size:{px(12.0)}px font-color:muted "Reading destination statuses…"
                }
            }
            text font-size:{px(12.0)}px font-color:muted
                "Map each local column to a destination status or list."
            button #action @click:{manual.set(!manual.get_untracked());} "Enter status IDs"
            for (_, column) in {model.board_columns().into_iter().map(|c|(c.id.clone(),c))} {
                let id = State::new(column.id.clone());
                let column_title = State::new(column.title.clone());
                let destination = State::new(String::new());
                {Effect::new(move || mappings.update(|m|{m.insert(id.get(),destination.get());}));}
                text font-family:sans-serif {column_title.get()}
                for (_, choice) in {choices.get().into_iter().map(|c|(c.id.clone(),c))} {
                    let choice_id=State::new(choice.id.clone());
                    let choice_title=State::new(choice.title.clone());
                    button #action @click:{destination.set(choice_id.get_untracked());}
                        label:{format!("Map {} to {}",column_title.get(),choice_title.get())}
                        fill:if mappings.get().get(&id.get())==Some(&choice_id.get()){accent-soft}else{raised}
                        {choice_title.get()}
                }
                if manual.get() {
                    input #input-field
                        label:{format!("Destination status for {}",column_title.get())} destination
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
                        button #action
                            @click:{repositories.update(|r|{r.insert(issue_id.get_untracked(),connection_id.get_untracked());});}
                            label:{format!("Issue repository {} for {}",connection_name.get(),issue_title.get())}
                            fill:if repositories.get().get(&issue_id.get())==Some(&connection_id.get()){accent-soft}else{raised}
                            {connection_name.get()}
                    }
                }
            }
            button #action
                @click:{
                    if let (Some(board),Ok(number))=(model.selected_board(),number.get_untracked().parse::<u64>()) {
                        let columns:Vec<_>=board.columns.iter().map(|c|ColumnMapping{local_id:c.id.clone(),remote_id:mappings.get_untracked().get(&c.id).cloned().unwrap_or_default()}).collect();
                        let tasks:Vec<_>=model.snapshot.get_untracked().issues.iter().filter(|i|board.columns.iter().any(|c|model.task_in_column(i,&c.id))).map(|i|TaskPublication{issue_id:i.id.clone(),repository_connection_id:repositories.get_untracked().get(&i.id).cloned().unwrap_or_default()}).collect();
                        if path.get_untracked().trim().is_empty() || title.get_untracked().trim().is_empty() || columns.iter().any(|c|c.remote_id.trim().is_empty()) || tasks.iter().any(|t|t.repository_connection_id.is_empty()) {model.notice.set("Choose a destination, column mappings and a repository for every task.".into());} else {
                            let source=if github.get_untracked(){BoardSource::Github{owner:path.get_untracked(),number,url:String::new()}}else{BoardSource::Gitlab{host:host.get_untracked(),group:group.get_untracked(),path:path.get_untracked(),number,url:String::new()}};
                            model.action(Command::PublishBoard{board_id:board.id,target:PublishTarget{source,name:title.get_untracked()},columns,tasks});
                        }
                    } else {model.notice.set("Enter a destination board number, or 0 for a new board.".into());}
                }
                label:"Confirm board publication"
                disabled:{model.busy.get() || !model.connected.get()} "Publish board"
            Operations model:(model)
        }
    }
}

#[component]
pub fn WorkspaceChoices(model: Model) -> Element {
    let open = State::new(false);
    view! {
        col height:min-content gap:{px(6.0)}px {
            button #action @click:{open.set(!open.get_untracked());}
                label:"Choose session resources" "Workspace resources"
            if open.get() {
                text font-size:{px(12.0)}px font-color:muted
                    "Automatic selects all ready repositories and directories on the server. Selection is fixed for subsequent turns."
                button #action @click:{model.workspace_selection.set(None);}
                    fill:if model.workspace_selection.get().is_none(){accent-soft}else{raised}
                    "Automatic resources"
                for (_, connection) in {model.snapshot.get().connections.into_iter().filter(|c|c.project_id==model.project.get() && c.enabled && c.state==ConnectionState::Ready && !matches!(c.kind,ConnectionKind::Board{..})).map(|c|(c.id.clone(),c)).collect::<Vec<_>>()} {
                    let id=State::new(connection.id.clone());
                    let name=State::new(connection.name.clone());
                    button #action
                        @click:{let all=model.snapshot.get_untracked().connections.iter().filter(|c|c.project_id==model.project.get_untracked() && c.enabled && c.state==ConnectionState::Ready && !matches!(c.kind,ConnectionKind::Board{..})).map(|c|c.id.clone()).collect();model.workspace_selection.update(|selected|{let ids=selected.get_or_insert(all);if ids.contains(&id.get_untracked()){ids.retain(|c|c!=&id.get_untracked());}else{ids.push(id.get_untracked());}});}
                        label:{format!("Workspace resource {}",name.get())}
                        fill:if model.workspace_selection.get().is_none_or(|ids|ids.contains(&id.get())){accent-soft}else{raised}
                        {name.get()}
                }
            }
        }
    }
}
