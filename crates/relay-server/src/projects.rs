//! Server-owned project setup and durable resource operations.
use crate::{Action, Error, RuntimeConfig, Workspace, runtime};
use relay_core::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

pub(super) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::CreateProject { .. }
            | Command::RenameProject { .. }
            | Command::AddConnection { .. }
            | Command::RetryConnection { .. }
            | Command::RemoveConnection { .. }
            | Command::CreateTask { .. }
            | Command::UpdateTask { .. }
            | Command::MoveTask { .. }
            | Command::UpdateBoardColumns { .. }
            | Command::SyncBoard { .. }
            | Command::PublishBoard { .. }
            | Command::RetryOperation { .. }
            | Command::ReconcileOperation { .. }
            | Command::StartDirector { .. }
            | Command::StartSession { .. }
            | Command::SetSessionConnections { .. }
    )
}

pub(super) fn interrupt_operations(snapshot: &mut Snapshot, reason: &str) -> bool {
    let mut changed = false;
    for op in &mut snapshot.operations {
        if !matches!(op.state, OperationState::Pending | OperationState::Running) {
            continue;
        }
        let unknown = op
            .results
            .get("pending")
            .is_some_and(|key| !op.results.contains_key(key));
        op.state = if unknown {
            OperationState::NeedsReconciliation
        } else {
            OperationState::Interrupted
        };
        op.error = Some(reason.into());
        changed = true;
        match &op.kind {
            OperationKind::Clone { connection_id } => {
                if let Some(c) = snapshot
                    .connections
                    .iter_mut()
                    .find(|c| &c.id == connection_id)
                {
                    c.state = ConnectionState::Interrupted;
                    c.error = op.error.clone();
                }
            }
            OperationKind::Sync { board_id } => {
                if let Some(board) = snapshot.boards.iter_mut().find(|b| &b.id == board_id) {
                    board.error = op.error.clone();
                }
                for connection in &mut snapshot.connections {
                    if matches!(&connection.kind,ConnectionKind::Board{board_id:id} if id==board_id)
                    {
                        connection.state = ConnectionState::Interrupted;
                        connection.error = op.error.clone();
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn text(value: &str, maximum: usize, label: &str) -> Result<String, Error> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(Error::invalid(format!("Enter a valid {label}")));
    }
    Ok(value.to_owned())
}

pub(super) fn repository(remote: &str) -> Result<(String, String, String), Error> {
    let remote = text(remote, 4096, "repository URL")?;
    let (host, path, normalized) = if let Some(rest) = remote.strip_prefix("https://") {
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| Error::invalid("Repository URL requires a host and path"))?;
        (host.to_owned(), path.to_owned(), remote.clone())
    } else if let Some(rest) = remote.strip_prefix("ssh://git@") {
        let (host, path) = rest
            .split_once('/')
            .ok_or_else(|| Error::invalid("Repository URL requires a host and path"))?;
        (host.to_owned(), path.to_owned(), remote.clone())
    } else if let Some(rest) = remote.strip_prefix("git@") {
        let (host, path) = rest
            .split_once(':')
            .ok_or_else(|| Error::invalid("SSH repository requires host:path"))?;
        (host.to_owned(), path.to_owned(), remote.clone())
    } else {
        (
            "github.com".into(),
            remote.clone(),
            format!("git@github.com:{remote}.git"),
        )
    };
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_owned();
    if host.is_empty()
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
        || path.split('/').count() < 2
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || !path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/_.-".contains(&c))
    {
        return Err(Error::invalid(
            "Use an SSH or HTTPS Git URL without embedded credentials",
        ));
    }
    Ok((host.to_ascii_lowercase(), path, normalized))
}

fn absolute(path: &str, create: bool) -> Result<PathBuf, Error> {
    let path = Path::new(path);
    if !path.is_absolute() || path.as_os_str().len() > 4096 || path.to_string_lossy().contains('\0')
    {
        return Err(Error::invalid(
            "Enter an absolute directory path on the server",
        ));
    }
    if create {
        std::fs::create_dir_all(path)
            .map_err(|_| Error::invalid("Cannot create project root on the server"))?;
    }
    let path = path
        .canonicalize()
        .map_err(|_| Error::invalid("Directory is unavailable on the server"))?;
    if !path.is_dir() {
        return Err(Error::invalid("Path must be a directory on the server"));
    }
    Ok(path)
}

pub(super) fn validate_source(source: &BoardSource) -> Result<(), Error> {
    let valid_path = |path: &str| {
        !path.is_empty()
            && path.len() <= 1024
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != "..")
            && path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/_.-".contains(&c))
    };
    match source {
        BoardSource::Local => Ok(()),
        BoardSource::Github { owner, number, .. }
            if valid_path(owner) && !owner.contains('/') && *number > 0 =>
        {
            Ok(())
        }
        BoardSource::Gitlab {
            host, path, number, ..
        } if valid_path(path)
            && *number > 0
            && !host.is_empty()
            && host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)) =>
        {
            Ok(())
        }
        _ => Err(Error::invalid(
            "Enter a valid board owner/path, host and positive board number",
        )),
    }
}

fn operation(snapshot: &mut Snapshot, project_id: &str, id: &str, kind: OperationKind) -> String {
    snapshot.operations.push(ProjectOperation {
        id: id.into(),
        project_id: project_id.into(),
        kind,
        state: OperationState::Pending,
        error: None,
        results: BTreeMap::new(),
    });
    id.into()
}

fn local_board(snapshot: &mut Snapshot, project_id: &str) -> String {
    if let Some(board) = snapshot
        .boards
        .iter()
        .find(|b| b.project_id == project_id && b.source == BoardSource::Local)
    {
        return board.id.clone();
    }
    let id = format!("local-board-{project_id}");
    snapshot.boards.push(Board {
        id: id.clone(),
        project_id: project_id.into(),
        name: "Local board".into(),
        source: BoardSource::Local,
        columns: local_columns(),
        last_synced_at: None,
        error: None,
    });
    id
}

fn add_connection(
    snapshot: &mut Snapshot,
    project_id: &str,
    input: ConnectionInput,
    id: &str,
) -> Result<Option<String>, Error> {
    let project = snapshot.project(project_id).map_err(Error::invalid)?;
    if project.fixture {
        return Err(Error::invalid("Fixture connections cannot be edited"));
    }
    let (name, kind, state, job) = match input {
        ConnectionInput::Repository { remote } => {
            let (_, name, remote) = repository(&remote)?;
            if project.root.is_none() {
                return Err(Error::invalid(
                    "This historical project has no root directory",
                ));
            }
            if snapshot.connections.iter().any(|c| c.project_id == project_id && c.enabled && matches!(&c.kind, ConnectionKind::Repository{remote:r,..} if repository(r).ok().map(|r|(r.0,r.1)) == repository(&remote).ok().map(|r|(r.0,r.1)))) { return Err(Error::invalid("Repository is already connected")); }
            (
                name,
                ConnectionKind::Repository {
                    remote,
                    checkout: None,
                    owned: true,
                },
                ConnectionState::Pending,
                Some(OperationKind::Clone {
                    connection_id: id.into(),
                }),
            )
        }
        ConnectionInput::Directory { path } => {
            let path = absolute(&path, false)?;
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let path = path.display().to_string();
            if snapshot.connections.iter().any(|c| {
                c.project_id == project_id
                    && c.enabled
                    && matches!(&c.kind, ConnectionKind::Directory{path:p} if p == &path)
            }) {
                return Err(Error::invalid("Directory is already connected"));
            }
            (
                name,
                ConnectionKind::Directory { path },
                ConnectionState::Ready,
                None,
            )
        }
        ConnectionInput::Board { source } => {
            validate_source(&source)?;
            if snapshot
                .boards
                .iter()
                .any(|b| b.project_id == project_id && b.source.same_board(&source))
            {
                return Err(Error::invalid("Board is already connected"));
            }
            let board_id = format!("board-{id}");
            let name = match &source {
                BoardSource::Local => "Local board".into(),
                BoardSource::Github { owner, number, .. } => format!("{owner} · board {number}"),
                BoardSource::Gitlab { path, number, .. } => format!("{path} · board {number}"),
            };
            let local = source == BoardSource::Local;
            snapshot.boards.push(Board {
                id: board_id.clone(),
                project_id: project_id.into(),
                name: name.clone(),
                source,
                columns: if local { local_columns() } else { vec![] },
                last_synced_at: None,
                error: None,
            });
            (
                name,
                ConnectionKind::Board {
                    board_id: board_id.clone(),
                },
                if local {
                    ConnectionState::Ready
                } else {
                    ConnectionState::Pending
                },
                if local {
                    None
                } else {
                    Some(OperationKind::Sync { board_id })
                },
            )
        }
    };
    snapshot.connections.push(ProjectConnection {
        id: id.into(),
        project_id: project_id.into(),
        name,
        enabled: true,
        state,
        error: None,
        kind,
    });
    Ok(job.map(|kind| operation(snapshot, project_id, &format!("operation-{id}"), kind)))
}

pub(super) fn apply(
    snapshot: &mut Snapshot,
    command: Command,
    request: &str,
    defaults: DirectorProfile,
    config: &RuntimeConfig,
) -> Result<Option<Action>, Error> {
    let mut jobs = Vec::new();
    match command {
        Command::CreateProject {
            name,
            root,
            connections,
        } => {
            let name = text(&name, 256, "project name")?;
            if connections.len() > 128 {
                return Err(Error::invalid("Too many initial connections"));
            }
            let root = absolute(&root, true)?.display().to_string();
            if snapshot
                .projects
                .iter()
                .any(|p| p.root.as_deref() == Some(root.as_str()) && !p.fixture)
            {
                return Err(Error::invalid("That project root is already in use"));
            }
            // Validate all inputs before persisting any resource.
            for connection in &connections {
                match connection {
                    ConnectionInput::Repository { remote } => {
                        repository(remote)?;
                    }
                    ConnectionInput::Directory { path } => {
                        absolute(path, false)?;
                    }
                    ConnectionInput::Board { source } => validate_source(source)?,
                }
            }
            let id = format!("project-{request}");
            snapshot.projects.push(Project {
                id: id.clone(),
                name,
                root: Some(root),
                repository: String::new(),
                fixture: false,
                columns: local_columns(),
                defaults,
                github: None,
            });
            snapshot.directors.push(Director {
                id: format!("director-{id}"),
                project_id: id.clone(),
                name: "Project director".into(),
                overrides: ProfileOverrides::default(),
            });
            local_board(snapshot, &id);
            let directory = Path::new(
                snapshot
                    .project(&id)
                    .map_err(Error::invalid)?
                    .root
                    .as_ref()
                    .unwrap(),
            )
            .join("workspace");
            let directory = absolute(&directory.display().to_string(), true)?;
            snapshot.connections.push(ProjectConnection {
                id: format!("workspace-{id}"),
                project_id: id.clone(),
                name: "Workspace".into(),
                enabled: true,
                state: ConnectionState::Ready,
                error: None,
                kind: ConnectionKind::Directory {
                    path: directory.display().to_string(),
                },
            });
            for (index, input) in connections.into_iter().enumerate() {
                if let Some(job) = add_connection(
                    snapshot,
                    &id,
                    input,
                    &format!("connection-{request}-{index}"),
                )? {
                    jobs.push(job);
                }
            }
        }
        Command::RenameProject { project_id, name } => {
            let name = text(&name, 256, "project name")?;
            let project = snapshot
                .projects
                .iter_mut()
                .find(|p| p.id == project_id && !p.fixture)
                .ok_or_else(|| Error::invalid("Project not found"))?;
            project.name = name;
        }
        Command::AddConnection {
            project_id,
            connection,
        } => {
            if snapshot
                .connections
                .iter()
                .filter(|c| c.project_id == project_id)
                .count()
                >= 128
            {
                return Err(Error::invalid("Project connection limit reached"));
            }
            if let Some(job) = add_connection(
                snapshot,
                &project_id,
                connection,
                &format!("connection-{request}"),
            )? {
                jobs.push(job);
            }
        }
        Command::RemoveConnection { connection_id } => {
            let connection = snapshot
                .connections
                .iter_mut()
                .find(|c| c.id == connection_id)
                .ok_or_else(|| Error::invalid("Connection not found"))?;
            if snapshot.operations.iter().any(|o| {
                o.project_id == connection.project_id
                    && matches!(o.state, OperationState::Pending | OperationState::Running)
            }) {
                return Err(Error::invalid(
                    "Wait for connection operations to finish before removing a resource",
                ));
            }
            connection.enabled = false;
        }
        Command::RetryConnection { connection_id } => {
            let connection = snapshot
                .connections
                .iter_mut()
                .find(|c| c.id == connection_id && c.enabled)
                .ok_or_else(|| Error::invalid("Connection not found"))?;
            if !matches!(
                connection.state,
                ConnectionState::Failed | ConnectionState::Interrupted
            ) {
                return Err(Error::invalid("Connection is not awaiting retry"));
            }
            let kind = match &connection.kind {
                ConnectionKind::Repository { .. } => OperationKind::Clone { connection_id },
                ConnectionKind::Board { board_id } => OperationKind::Sync {
                    board_id: board_id.clone(),
                },
                ConnectionKind::Directory { path } => {
                    absolute(path, false)?;
                    connection.state = ConnectionState::Ready;
                    connection.error = None;
                    snapshot.revision += 1;
                    return Ok(None);
                }
            };
            connection.state = ConnectionState::Pending;
            connection.error = None;
            let project = connection.project_id.clone();
            jobs.push(operation(snapshot, &project, request, kind));
        }
        Command::CreateTask {
            board_id,
            title,
            body,
            repository_connection_id,
        } => {
            let board = snapshot.board(&board_id).map_err(Error::invalid)?.clone();
            editable_board(snapshot, &board_id)?;
            let title = text(&title, 1024, "task title")?;
            if body.len() > 64 * 1024 || body.contains('\0') {
                return Err(Error::invalid("Task body is too large"));
            }
            if let Some(id) = &repository_connection_id
                && !snapshot.connections.iter().any(|c| {
                    &c.id == id
                        && c.project_id == board.project_id
                        && c.enabled
                        && matches!(c.kind, ConnectionKind::Repository { .. })
                })
            {
                return Err(Error::invalid(
                    "Task repository does not belong to this project",
                ));
            }
            let issue_id = format!("task-{request}");
            snapshot.issues.push(Issue {
                id: issue_id.clone(),
                project_id: board.project_id.clone(),
                reference: None,
                repository_connection_id,
                title,
                body,
                column_id: board
                    .columns
                    .first()
                    .map(|c| c.id.clone())
                    .unwrap_or_default(),
                labels: vec![],
                result: None,
            });
            if board.source == BoardSource::Local {
                snapshot.memberships.push(BoardMembership {
                    board_id,
                    issue_id,
                    column_ids: vec![board.columns[0].id.clone()],
                    remote_item_id: None,
                });
            } else {
                jobs.push(operation(
                    snapshot,
                    &board.project_id,
                    request,
                    OperationKind::CreateTask { board_id, issue_id },
                ));
            }
        }
        Command::UpdateTask {
            issue_id,
            title,
            body,
        } => {
            let title = text(&title, 1024, "task title")?;
            if body.len() > 64 * 1024 || body.contains('\0') {
                return Err(Error::invalid("Task body is too large"));
            }
            let issue = snapshot
                .issues
                .iter()
                .find(|i| i.id == issue_id)
                .ok_or_else(|| Error::invalid("Task not found"))?
                .clone();
            editable_task(snapshot, &issue_id)?;
            if issue.reference.is_some() {
                jobs.push(operation(
                    snapshot,
                    &issue.project_id,
                    request,
                    OperationKind::EditTask {
                        issue_id,
                        title,
                        body,
                    },
                ));
            } else {
                let issue = snapshot
                    .issues
                    .iter_mut()
                    .find(|i| i.id == issue_id)
                    .unwrap();
                issue.title = title;
                issue.body = body;
            }
        }
        Command::MoveTask {
            board_id,
            issue_id,
            column_id,
        } => {
            editable_board(snapshot, &board_id)?;
            let board = snapshot.board(&board_id).map_err(Error::invalid)?.clone();
            if !board.columns.iter().any(|c| c.id == column_id)
                || !snapshot
                    .memberships
                    .iter()
                    .any(|m| m.board_id == board_id && m.issue_id == issue_id)
            {
                return Err(Error::invalid(
                    "Task or destination column not on this board",
                ));
            }
            if board.source == BoardSource::Local {
                let m = snapshot
                    .memberships
                    .iter_mut()
                    .find(|m| m.board_id == board_id && m.issue_id == issue_id)
                    .unwrap();
                m.column_ids = vec![column_id.clone()];
                snapshot
                    .issues
                    .iter_mut()
                    .find(|i| i.id == issue_id)
                    .unwrap()
                    .column_id = column_id;
            } else {
                jobs.push(operation(
                    snapshot,
                    &board.project_id,
                    request,
                    OperationKind::MoveTask {
                        board_id,
                        issue_id,
                        column_id,
                    },
                ));
            }
        }
        Command::UpdateBoardColumns { board_id, columns } => {
            editable_board(snapshot, &board_id)?;
            if snapshot.board(&board_id).map_err(Error::invalid)?.source != BoardSource::Local {
                return Err(Error::invalid("Manage remote columns on the provider"));
            }
            if columns.is_empty() || columns.len() > 64 {
                return Err(Error::invalid("Boards need 1–64 columns"));
            }
            let mut ids = std::collections::BTreeSet::new();
            for c in &columns {
                text(&c.id, 256, "column ID")?;
                text(&c.title, 256, "column title")?;
                if !ids.insert(&c.id) {
                    return Err(Error::invalid("Column IDs must be unique"));
                }
            }
            if snapshot
                .memberships
                .iter()
                .any(|m| m.board_id == board_id && m.column_ids.iter().any(|id| !ids.contains(id)))
            {
                return Err(Error::invalid("Move tasks before removing their column"));
            }
            snapshot
                .boards
                .iter_mut()
                .find(|b| b.id == board_id)
                .unwrap()
                .columns = columns;
        }
        Command::SyncBoard { board_id } => {
            let board = snapshot.board(&board_id).map_err(Error::invalid)?.clone();
            editable_board(snapshot, &board_id)?;
            if board.source == BoardSource::Local {
                return Err(Error::invalid(
                    "Local boards do not need remote synchronization",
                ));
            }
            jobs.push(operation(
                snapshot,
                &board.project_id,
                request,
                OperationKind::Sync { board_id },
            ));
        }
        Command::PublishBoard {
            board_id,
            target,
            columns,
            tasks,
        } => {
            let board = snapshot.board(&board_id).map_err(Error::invalid)?.clone();
            editable_board(snapshot, &board_id)?;
            if board.source != BoardSource::Local || target.source == BoardSource::Local {
                return Err(Error::invalid(
                    "Choose a remote destination for a local board",
                ));
            }
            let mut destination = target.source.clone();
            match &mut destination {
                BoardSource::Github { number, .. } | BoardSource::Gitlab { number, .. }
                    if *number == 0 =>
                {
                    *number = 1
                }
                _ => {}
            };
            validate_source(&destination)?;
            text(&target.name, 256, "board name")?;
            let board_tasks: std::collections::BTreeSet<_> = snapshot
                .memberships
                .iter()
                .filter(|m| m.board_id == board_id)
                .map(|m| m.issue_id.clone())
                .collect();
            let mapped_tasks: std::collections::BTreeSet<_> =
                tasks.iter().map(|t| t.issue_id.clone()).collect();
            if board_tasks != mapped_tasks || mapped_tasks.len() != tasks.len() {
                return Err(Error::invalid(
                    "Choose a destination repository for every task",
                ));
            }
            if board.columns.iter().any(|c| {
                !columns
                    .iter()
                    .any(|m| m.local_id == c.id && !m.remote_id.is_empty())
            }) {
                return Err(Error::invalid("Map every local column to a remote column"));
            }
            for task in &tasks {
                if !snapshot.connections.iter().any(|c| {
                    c.id == task.repository_connection_id
                        && c.project_id == board.project_id
                        && c.enabled
                        && c.state == ConnectionState::Ready
                        && matches!(c.kind, ConnectionKind::Repository { .. })
                }) {
                    return Err(Error::invalid("A publication repository is not ready"));
                }
            }
            jobs.push(operation(
                snapshot,
                &board.project_id,
                request,
                OperationKind::Publish {
                    board_id,
                    target,
                    columns,
                    tasks,
                },
            ));
        }
        Command::RetryOperation { operation_id } => {
            let operation = snapshot
                .operations
                .iter_mut()
                .find(|o| o.id == operation_id)
                .ok_or_else(|| Error::invalid("Operation not found"))?;
            if !matches!(
                operation.state,
                OperationState::Failed | OperationState::Interrupted
            ) {
                return Err(Error::invalid(
                    "Reconcile uncertain provider results before retrying",
                ));
            }
            operation.state = OperationState::Pending;
            operation.error = None;
            jobs.push(operation_id);
        }
        Command::ReconcileOperation {
            operation_id,
            key,
            result,
        } => {
            let operation = snapshot
                .operations
                .iter_mut()
                .find(|o| o.id == operation_id)
                .ok_or_else(|| Error::invalid("Operation not found"))?;
            if operation.state != OperationState::NeedsReconciliation {
                return Err(Error::invalid("Operation does not need reconciliation"));
            }
            let key = text(&key, 256, "operation step")?;
            let result = text(&result, 16 * 1024, "provider result")?;
            if operation.results.get("pending") != Some(&key)
                || operation.results.contains_key(&key)
            {
                return Err(Error::invalid(
                    "Use the exact unresolved provider step; known results cannot be replaced",
                ));
            }
            operation.results.insert(key, result);
            operation.state = OperationState::Pending;
            operation.error = None;
            jobs.push(operation_id);
        }
        Command::StartDirector {
            director_id,
            prompt,
            approve_implementation,
        } => {
            return start(
                snapshot,
                request,
                &director_id,
                None,
                SessionRole::Director,
                prompt,
                approve_implementation,
                None,
                config,
            );
        }
        Command::StartSession {
            director_id,
            issue_id,
            role,
            prompt,
            approve_implementation,
            connection_ids,
        } => {
            return start(
                snapshot,
                request,
                &director_id,
                issue_id,
                role,
                prompt,
                approve_implementation,
                connection_ids,
                config,
            );
        }
        Command::SetSessionConnections {
            session_id,
            connection_ids,
        } => {
            let session = snapshot
                .sessions
                .iter()
                .find(|s| s.id == session_id && !s.fixture)
                .ok_or_else(|| Error::invalid("Session not found"))?;
            if session
                .worker
                .as_ref()
                .is_none_or(|w| w.thread_id.is_some() || runtime::active(&w.status))
                || !session.workspaces.is_empty()
            {
                return Err(Error::invalid(
                    "Started sessions retain their original workspaces; start another session to change them",
                ));
            }
            validate_connections(snapshot, &session.project_id, &connection_ids)?;
            snapshot
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                .unwrap()
                .connection_ids = connection_ids;
        }
        _ => return Err(Error::invalid("Unsupported project command")),
    }
    if jobs.is_empty() {
        snapshot.revision += 1;
        Ok(None)
    } else {
        Ok(Some(Action::Operations(jobs)))
    }
}

fn editable_board(snapshot: &Snapshot, board_id: &str) -> Result<(), Error> {
    let board = snapshot.board(board_id).map_err(Error::invalid)?;
    if snapshot
        .project(&board.project_id)
        .map_err(Error::invalid)?
        .fixture
    {
        return Err(Error::invalid("Fixture boards are read-only"));
    }
    if snapshot.operations.iter().any(|o|matches!(o.state,OperationState::Pending|OperationState::Running|OperationState::NeedsReconciliation|OperationState::Interrupted) && matches!(&o.kind,OperationKind::Publish{board_id:id,..}|OperationKind::Sync{board_id:id}|OperationKind::MoveTask{board_id:id,..}|OperationKind::CreateTask{board_id:id,..} if id==board_id)) {return Err(Error::invalid("Resolve the pending board operation first"));}
    Ok(())
}
fn editable_task(snapshot: &Snapshot, issue_id: &str) -> Result<(), Error> {
    for membership in snapshot
        .memberships
        .iter()
        .filter(|m| m.issue_id == issue_id)
    {
        editable_board(snapshot, &membership.board_id)?;
    }
    if snapshot.operations.iter().any(|o| {
        matches!(
            o.state,
            OperationState::Pending
                | OperationState::Running
                | OperationState::NeedsReconciliation
                | OperationState::Interrupted
        ) && matches!(&o.kind,OperationKind::EditTask{issue_id:id,..} if id==issue_id)
    }) {
        return Err(Error::invalid("Resolve the pending task edit first"));
    }
    Ok(())
}
pub(super) fn validate_connections(
    snapshot: &Snapshot,
    project_id: &str,
    ids: &[String],
) -> Result<(), Error> {
    if ids.len() > 64 {
        return Err(Error::invalid("Select at most 64 session workspaces"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id)
            || !snapshot.connections.iter().any(|c| {
                &c.id == id
                    && c.project_id == project_id
                    && c.enabled
                    && c.state == ConnectionState::Ready
                    && !matches!(c.kind, ConnectionKind::Board { .. })
            })
        {
            return Err(Error::invalid(
                "Session workspace must be a ready resource in its project",
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn start(
    snapshot: &mut Snapshot,
    request: &str,
    director_id: &str,
    issue_id: Option<String>,
    role: SessionRole,
    prompt: String,
    approved: bool,
    ids: Option<Vec<String>>,
    config: &RuntimeConfig,
) -> Result<Option<Action>, Error> {
    crate::validate_prompt(&prompt)?;
    let director = snapshot
        .directors
        .iter()
        .find(|d| d.id == director_id)
        .ok_or_else(|| Error::invalid("Director not found"))?
        .clone();
    let project = snapshot
        .project(&director.project_id)
        .map_err(Error::invalid)?
        .clone();
    if project.fixture {
        return Err(Error::invalid("Fixture agents cannot be started"));
    }
    let issue_id = if let Some(id) = issue_id {
        id
    } else if role == SessionRole::Director {
        let board_id = local_board(snapshot, &project.id);
        let id = format!("task-{request}");
        let title = prompt
            .lines()
            .next()
            .unwrap_or("Planning")
            .chars()
            .take(160)
            .collect();
        snapshot.issues.push(Issue {
            id: id.clone(),
            project_id: project.id.clone(),
            reference: None,
            repository_connection_id: None,
            title,
            body: prompt.clone(),
            column_id: "backlog".into(),
            labels: vec![],
            result: None,
        });
        snapshot.memberships.push(BoardMembership {
            board_id,
            issue_id: id.clone(),
            column_ids: vec!["backlog".into()],
            remote_item_id: None,
        });
        id
    } else {
        return Err(Error::invalid("Choose a task for the worker"));
    };
    runtime::authorize_session(snapshot, &issue_id, director_id, approved, &role, config)?;
    let ids = ids.unwrap_or_else(|| {
        snapshot
            .connections
            .iter()
            .filter(|c| {
                c.project_id == project.id
                    && c.enabled
                    && c.state == ConnectionState::Ready
                    && !matches!(c.kind, ConnectionKind::Board { .. })
            })
            .map(|c| c.id.clone())
            .collect()
    });
    validate_connections(snapshot, &project.id, &ids)?;
    let profile = snapshot
        .effective_profile(&director)
        .map_err(Error::invalid)?;
    let session_id = format!("session-{request}");
    let title = snapshot
        .issues
        .iter()
        .find(|i| i.id == issue_id)
        .unwrap()
        .title
        .clone();
    snapshot.sessions.push(Session {
        id: session_id.clone(),
        project_id: project.id,
        issue_id: Some(issue_id),
        director_id: director_id.into(),
        title,
        role,
        fixture: false,
        connection_ids: ids,
        workspaces: vec![],
        worker: Some(WorkerRun {
            harness: profile.harness,
            execution: None,
            status: WorkerStatus::Queued,
            thread_id: None,
            worktree: None,
            branch: None,
            base_commit: None,
            error: None,
            usage: None,
            changes: None,
        }),
    });
    snapshot
        .messages
        .push(crate::prompt_message(&session_id, request, &prompt));
    Ok(Some(Action::Run { session_id, prompt }))
}

pub(super) fn clone_repository(workspace: &Workspace, id: &str) -> Result<(), Error> {
    let snapshot = workspace.snapshots.borrow().clone();
    let op = snapshot
        .operations
        .iter()
        .find(|o| o.id == id)
        .ok_or_else(|| Error::invalid("Operation not found"))?;
    let OperationKind::Clone { connection_id } = &op.kind else {
        return Err(Error::invalid("Not a clone operation"));
    };
    let connection = snapshot
        .connections
        .iter()
        .find(|c| &c.id == connection_id && c.enabled)
        .ok_or_else(|| Error::invalid("Connection removed"))?;
    let ConnectionKind::Repository { remote, .. } = &connection.kind else {
        return Err(Error::invalid("Not a repository"));
    };
    let (host, path, remote) = repository(remote)?;
    let project = snapshot
        .project(&connection.project_id)
        .map_err(Error::invalid)?;
    let root = absolute(
        project
            .root
            .as_deref()
            .ok_or_else(|| Error::invalid("Project root unavailable"))?,
        false,
    )?;
    let destination = root.join("repos").join(host).join(path);
    let parent = destination.parent().unwrap();
    std::fs::create_dir_all(parent).map_err(|_| Error::invalid("Cannot create clone directory"))?;
    if !parent
        .canonicalize()
        .map_err(Error::internal)?
        .starts_with(&root)
    {
        return Err(Error::invalid("Clone directory escapes the project root"));
    }
    let mut owned = true;
    if destination.exists() {
        let origin = crate::process::capture(
            ProcessCommand::new("git")
                .arg("-C")
                .arg(&destination)
                .args(["remote", "get-url", "origin"]),
            16 * 1024,
            Duration::from_secs(10),
            "Git",
        )?;
        let existing = repository(String::from_utf8_lossy(&origin.bytes).trim())?;
        let wanted = repository(&remote)?;
        if !origin.status.success() || existing.0 != wanted.0 || existing.1 != wanted.1 {
            return Err(Error::invalid(
                "Clone destination already contains different files",
            ));
        }
        owned = false;
    } else {
        let staging = tempfile::Builder::new()
            .prefix(".relay-clone-")
            .tempdir_in(parent)
            .map_err(Error::internal)?;
        let result = crate::process::capture(
            ProcessCommand::new("git")
                .args(["clone", "--"])
                .arg(&remote)
                .arg(staging.path())
                .env("GIT_TERMINAL_PROMPT", "0")
                .env_remove("RELAY_TOKEN"),
            1024 * 1024,
            Duration::from_secs(300),
            "Git clone",
        )?;
        if !result.status.success() || result.truncated {
            return Err(Error::invalid(
                "Repository clone failed; check server Git credentials and network, then retry",
            ));
        }
        std::fs::rename(staging.path(), &destination)
            .map_err(|_| Error::invalid("Cannot install repository clone; destination changed"))?;
    }
    let path = destination
        .canonicalize()
        .map_err(Error::internal)?
        .display()
        .to_string();
    workspace.update_project(|s| {
        let connection = s
            .connections
            .iter_mut()
            .find(|c| &c.id == connection_id && c.enabled)
            .ok_or_else(|| Error::invalid("Connection removed"))?;
        if let ConnectionKind::Repository {
            checkout, owned: o, ..
        } = &mut connection.kind
        {
            *checkout = Some(path);
            *o = owned;
        }
        connection.state = ConnectionState::Ready;
        connection.error = None;
        Ok(())
    })
}

pub(super) fn run(workspace: Workspace, id: String) {
    crate::process::with_cancellation(workspace.transport_shutdown.subscribe(), || {
        run_inner(workspace.clone(), id)
    });
}
fn run_inner(workspace: Workspace, id: String) {
    let ready = workspace.update_project(|s| {
        let op = s
            .operations
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| Error::invalid("Operation not found"))?;
        if op.state != OperationState::Pending {
            return Err(Error::invalid("Operation is not queued"));
        }
        op.state = OperationState::Running;
        op.error = None;
        match &op.kind {
            OperationKind::Clone { connection_id } => {
                if let Some(c) = s.connections.iter_mut().find(|c| &c.id == connection_id) {
                    c.state = ConnectionState::Pending;
                    c.error = None;
                }
            }
            OperationKind::Sync { board_id } => {
                for c in &mut s.connections {
                    if matches!(&c.kind,ConnectionKind::Board{board_id:id} if id==board_id) {
                        c.state = ConnectionState::Pending;
                        c.error = None;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    });
    if ready.is_err() {
        return;
    }
    let clone = workspace
        .snapshots
        .borrow()
        .operations
        .iter()
        .any(|o| o.id == id && matches!(o.kind, OperationKind::Clone { .. }));
    let result = if clone {
        clone_repository(&workspace, &id)
    } else {
        crate::providers::execute(&workspace, &id)
    };
    let _ = workspace.update_project(|s| {
        let op = s
            .operations
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| Error::invalid("Operation not found"))?;
        if let Err(error) = result {
            if op.state != OperationState::NeedsReconciliation {
                op.state = OperationState::Failed;
            }
            op.error = Some(error.to_string());
            if let OperationKind::Clone { connection_id } = &op.kind
                && let Some(c) = s.connections.iter_mut().find(|c| &c.id == connection_id)
            {
                c.state = ConnectionState::Failed;
                c.error = op.error.clone();
            }
            if let OperationKind::Sync { board_id } = &op.kind {
                if let Some(b) = s.boards.iter_mut().find(|b| &b.id == board_id) {
                    b.error = op.error.clone();
                }
                for c in &mut s.connections {
                    if matches!(&c.kind,ConnectionKind::Board{board_id:id} if id==board_id) {
                        c.state = ConnectionState::Failed;
                        c.error = op.error.clone();
                    }
                }
            }
        } else if op.state == OperationState::Running {
            op.state = OperationState::Completed;
            op.error = None;
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{env, workspace};

    fn create(snapshot: &mut Snapshot, root: &Path, name: &str) -> String {
        let request = uuid::Uuid::new_v4().to_string();
        apply(
            snapshot,
            Command::CreateProject {
                name: name.into(),
                root: root.display().to_string(),
                connections: vec![],
            },
            &request,
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        format!("project-{request}")
    }

    #[test]
    fn clone_real_git_child() {
        let Some(root) = std::env::var_os("RELAY_CLONE_TEST_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let mut s = Snapshot::default();
        let project = create(&mut s, &root, "Clone");
        let action = apply(
            &mut s,
            Command::AddConnection {
                project_id: project,
                connection: ConnectionInput::Repository {
                    remote: "org/repo".into(),
                },
            },
            "clone",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        let Some(Action::Operations(ids)) = action else {
            panic!("Clone not scheduled");
        };
        let w = workspace(&root.join("test.sqlite3"), &s, RuntimeConfig::default());
        run(w.clone(), ids[0].clone());
        let completed = w.snapshots.borrow().clone();
        let connection = completed
            .connections
            .iter()
            .find(|c| c.id == "connection-clone")
            .unwrap();
        assert_eq!(
            connection.state,
            ConnectionState::Ready,
            "{:?}",
            connection.error
        );
        let ConnectionKind::Repository {
            checkout: Some(path),
            owned,
            ..
        } = &connection.kind
        else {
            panic!("No clone");
        };
        assert!(*owned);
        assert!(Path::new(path).join(".git").is_dir());
        assert_eq!(Path::new(path), root.join("repos/github.com/org/repo"));
        assert_eq!(completed.operations[0].state, OperationState::Completed);
    }

    #[test]
    fn automatic_clone_uses_real_git_and_an_isolated_credential_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let repo = crate::tests::review_repo(&source);
        let bare = source.join("repo.git");
        crate::tests::git(
            &repo,
            &[
                "clone",
                "--bare",
                repo.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let config = dir.path().join("gitconfig");
        std::fs::write(
            &config,
            format!(
                "[url \"file://{}/\"]\n\tinsteadOf = git@github.com:org/\n",
                source.display()
            ),
        )
        .unwrap();
        let output = ProcessCommand::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "projects::tests::clone_real_git_child",
                "--nocapture",
            ])
            .env("RELAY_CLONE_TEST_ROOT", dir.path().join("project"))
            .env("GIT_CONFIG_GLOBAL", config)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[tokio::test]
    async fn shutdown_cancels_provider_capture_and_records_interruption_before_returning() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("provider-pid");
        let bin = dir.path().join("gh");
        crate::tests::script(
            &bin,
            &format!(
                "printf '%s' $$ > '{}'\nwhile :; do sleep 1; done",
                pid_file.display()
            ),
        );
        let mut s = Snapshot::default();
        let project = create(&mut s, &dir.path().join("project"), "Project");
        let action = apply(
            &mut s,
            Command::AddConnection {
                project_id: project,
                connection: ConnectionInput::Board {
                    source: BoardSource::Github {
                        owner: "owner".into(),
                        number: 1,
                        url: String::new(),
                    },
                },
            },
            "board",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        let Some(Action::Operations(ids)) = action else {
            panic!("No sync");
        };
        let w = workspace(
            &dir.path().join("db"),
            &s,
            RuntimeConfig {
                gh: bin,
                ..Default::default()
            },
        );
        let job = crate::ProjectJob::new(&w);
        let child = w.clone();
        let task = tokio::task::spawn_blocking(move || {
            let _job = job;
            run(child, ids[0].clone())
        });
        let pid = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(pid) = std::fs::read_to_string(&pid_file)
                    .ok()
                    .and_then(|p| p.parse::<u32>().ok())
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let shutdown = crate::Shutdown {
            workspace: w.clone(),
        };
        tokio::time::timeout(Duration::from_secs(2), shutdown.shutdown())
            .await
            .unwrap()
            .unwrap();
        task.await.unwrap();
        assert_eq!(w.project_jobs.load(std::sync::atomic::Ordering::SeqCst), 0);
        let snapshot = w.snapshots.borrow().clone();
        assert_eq!(snapshot.operations[0].state, OperationState::Interrupted);
        assert!(crate::runtime::process_identity(pid).is_none());
    }
    #[test]
    fn projects_are_independent_and_local_tasks_have_no_fabricated_remote_reference() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Snapshot {
            protocol_version: PROTOCOL_VERSION,
            ..Default::default()
        };
        let first = create(&mut s, &dir.path().join("one"), "One");
        let second = create(&mut s, &dir.path().join("two"), "Two");
        assert_ne!(first, second);
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.directors.len(), 2);
        let board = s
            .boards
            .iter()
            .find(|b| b.project_id == first)
            .unwrap()
            .id
            .clone();
        apply(
            &mut s,
            Command::CreateTask {
                board_id: board.clone(),
                title: "Build locally".into(),
                body: "No provider required".into(),
                repository_connection_id: None,
            },
            "local-task",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        let task = &s.issues[0];
        assert!(task.reference.is_none());
        assert_eq!(task.project_id, first);
        assert!(s.visible_task(&task.id));
        assert!(s.boards.iter().any(|b| b.project_id == second));
        let task_id = task.id.clone();
        apply(
            &mut s,
            Command::MoveTask {
                board_id: board.clone(),
                issue_id: task_id.clone(),
                column_id: "done".into(),
            },
            "move",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        assert_eq!(s.memberships[0].column_ids, vec!["done"]);
        let remove = Command::UpdateBoardColumns {
            board_id: board,
            columns: vec![BoardColumn {
                id: "backlog".into(),
                title: "Backlog".into(),
            }],
        };
        assert!(
            apply(
                &mut s,
                remove,
                "remove",
                DirectorProfile::default(),
                &RuntimeConfig::default()
            )
            .is_err()
        );
        assert_eq!(s.issues[0].id, task_id);
    }

    #[test]
    fn project_and_directory_validation_does_not_replace_existing_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Snapshot::default();
        let first = create(&mut s, &dir.path().join("project"), "Project");
        let original = s.clone();
        assert!(
            apply(
                &mut s,
                Command::CreateProject {
                    name: "Duplicate".into(),
                    root: dir.path().join("project").display().to_string(),
                    connections: vec![]
                },
                "duplicate",
                DirectorProfile::default(),
                &RuntimeConfig::default()
            )
            .is_err()
        );
        assert_eq!(s, original);
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        apply(
            &mut s,
            Command::AddConnection {
                project_id: first.clone(),
                connection: ConnectionInput::Directory {
                    path: outside.display().to_string(),
                },
            },
            "directory",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        assert!(s.connections.iter().any(|c|c.project_id==first&&matches!(&c.kind,ConnectionKind::Directory{path} if path==&outside.display().to_string())));
        for remote in [
            "../../escape",
            "https://token:secret@github.com/org/repo",
            "git@github.com:org/../escape",
            "https://github.com/repo",
        ] {
            assert!(repository(remote).is_err(), "{remote}");
        }
        assert_eq!(
            repository("org/repo").unwrap(),
            (
                "github.com".into(),
                "org/repo".into(),
                "git@github.com:org/repo.git".into()
            )
        );
    }

    #[test]
    fn director_start_creates_traceable_local_planning_task_and_worker_selection_is_immutable() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Snapshot::default();
        let project = create(&mut s, &dir.path().join("project"), "Project");
        let director = s
            .directors
            .iter()
            .find(|d| d.project_id == project)
            .unwrap()
            .id
            .clone();
        let action = apply(
            &mut s,
            Command::StartDirector {
                director_id: director,
                prompt: "Plan this project".into(),
                approve_implementation: false,
            },
            "director-start",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        assert!(matches!(action, Some(Action::Run { .. })));
        let session = &s.sessions[0];
        assert_eq!(session.role, SessionRole::Director);
        assert!(session.issue_id.is_some());
        assert!(s.visible_task(session.issue_id.as_ref().unwrap()));
        assert_eq!(session.connection_ids.len(), 1);
        let id = session.id.clone();
        let ids = session.connection_ids.clone();
        assert!(
            apply(
                &mut s,
                Command::SetSessionConnections {
                    session_id: id,
                    connection_ids: vec![]
                },
                "changed",
                DirectorProfile::default(),
                &RuntimeConfig::default()
            )
            .is_err()
        );
        assert_eq!(s.sessions[0].connection_ids, ids);
    }

    #[test]
    fn accepted_project_creation_is_idempotent_across_database_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let mut store = crate::Store::open(&db, DirectorProfile::default()).unwrap();
        let before = store.snapshot().unwrap();
        let request = env(
            before.revision,
            Command::CreateProject {
                name: "Saved".into(),
                root: dir.path().join("saved").display().to_string(),
                connections: vec![],
            },
        );
        let (s, _) = store
            .apply(request.clone(), &RuntimeConfig::default())
            .unwrap();
        drop(store);
        let mut store = crate::Store::open(&db, DirectorProfile::default()).unwrap();
        let (retry, action) = store.apply(request, &RuntimeConfig::default()).unwrap();
        assert_eq!(s, retry);
        assert!(action.is_none());
        assert_eq!(
            retry.projects.iter().filter(|p| p.name == "Saved").count(),
            1
        );
    }

    #[test]
    fn restart_interrupts_clone_without_replaying_it_and_preserves_all_projects() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let mut s = Snapshot::default();
        let project = create(&mut s, &dir.path().join("saved"), "Saved");
        apply(
            &mut s,
            Command::AddConnection {
                project_id: project.clone(),
                connection: ConnectionInput::Repository {
                    remote: "org/repo".into(),
                },
            },
            "clone",
            DirectorProfile::default(),
            &RuntimeConfig::default(),
        )
        .unwrap();
        let w = workspace(&db, &s, RuntimeConfig::default());
        drop(w);
        let (_router, _shutdown) = crate::router_with_shutdown(
            &db,
            "test-secret-token".into(),
            DirectorProfile::default(),
            RuntimeConfig::default(),
        )
        .unwrap();
        let store = crate::Store::open(&db, DirectorProfile::default()).unwrap();
        let next = store.snapshot().unwrap();
        assert_eq!(next.projects, s.projects);
        assert_eq!(next.operations[0].state, OperationState::Interrupted);
        assert_eq!(
            next.connections
                .iter()
                .find(|c| c.id == "connection-clone")
                .unwrap()
                .state,
            ConnectionState::Interrupted
        );
    }
}
