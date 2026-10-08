//! Server-only board providers. CLI credentials never cross the protocol boundary.
//!
//! Journal keys (values are JSON unless noted): `board` = resolved BoardSource;
//! `issue/<task-id>` = IssueRef; `membership/<task-id>` = remote item ID (plain);
//! `status/<task-id>` and `edit/<task-id>` = `confirmed` (plain).
//! `column/<local-column-id>` = resolved remote column ID (plain).
//! `pending` = key of an in-flight write (plain). It is persisted *before* a write;
//! an interrupted/ambiguous write cannot replay until that key has a known result.
//! Reconciliation must supply the exact result type above, never an arbitrary OK.
mod github;
mod gitlab;
#[cfg(test)]
mod tests;

use crate::{Error, RuntimeConfig, Workspace};
use relay_core::*;
use serde_json::{Value, json};
use std::{collections::HashSet, process::Command, time::Duration};

const LIMIT: usize = 16 * 1024 * 1024;
const MAX_PAGES: usize = 10_000;
const NO_STATUS: &str = "github-no-status";

fn text(v: &Value, key: &str) -> Result<String, Error> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::invalid(format!("Provider response missing {key}")))
}
fn number(v: &Value, key: &str) -> Result<u64, Error> {
    v[key]
        .as_u64()
        .ok_or_else(|| Error::invalid(format!("Provider response missing {key}")))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    v[key]
        .as_array()
        .ok_or_else(|| Error::invalid(format!("Provider response missing {key}")))
}
fn request(mut command: Command) -> Result<Value, Error> {
    let output = crate::process::capture(
        &mut command,
        LIMIT,
        Duration::from_secs(30),
        "Board provider request",
    )?;
    if output.truncated {
        return Err(Error::invalid("Provider response exceeds 16 MiB limit"));
    }
    if !output.status.success() {
        return Err(Error::invalid(
            "Provider request failed; check server CLI authentication, permissions, and network",
        ));
    }
    let value: Value = serde_json::from_slice(&output.bytes)
        .map_err(|_| Error::invalid("Invalid provider JSON response"))?;
    if value.get("errors").is_some() {
        return Err(Error::invalid(
            "Provider rejected request; check permissions and board configuration",
        ));
    }
    Ok(value)
}
fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn safe_host(host: &str) -> bool {
    host.as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && host
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
}
fn validate_source(source: &BoardSource) -> Result<(), Error> {
    let valid = match source {
        BoardSource::Local => false,
        BoardSource::Github { owner, url, .. } => {
            safe_segment(owner) && (url.is_empty() || url.starts_with("https://github.com/"))
        }
        BoardSource::Gitlab {
            host, path, url, ..
        } => {
            safe_host(host)
                && path.split('/').all(safe_segment)
                && (url.is_empty() || url.starts_with(&format!("https://{host}/")))
        }
    };
    if valid {
        Ok(())
    } else {
        Err(Error::invalid("Invalid or unsupported remote board source"))
    }
}

#[derive(Clone, Debug)]
struct RemoteBoard {
    board: Board,
    remote_id: String,
    status_field: Option<String>,
    writable: bool,
    label_lists: Vec<(String, String)>,
}
#[derive(Debug)]
struct RemoteTask {
    reference: IssueRef,
    title: String,
    body: String,
    labels: Vec<String>,
    columns: Vec<String>,
    item_id: String,
}
/// Metadata only: no task writes. IDs/project IDs are empty; caller attaches them.
/// For number=0, return supported default mappings after checking scope access.
/// Discovery never creates a board; only an explicit Publish operation does.
pub fn discover(config: &RuntimeConfig, source: &BoardSource) -> Result<Board, Error> {
    if source_number(source) == 0 {
        validate_source(source)?;
        let columns = match source {
            BoardSource::Github { owner, .. } => {
                github::check_creation(config, owner)?;
                ["Todo", "In Progress", "Done", "No status"]
                    .into_iter()
                    .map(|title| BoardColumn {
                        id: format!("name:{title}"),
                        title: title.into(),
                    })
                    .collect()
            }
            BoardSource::Gitlab { .. } => {
                if !gitlab::can_write(config, source)? {
                    return Err(Error::invalid(
                        "Destination board scope write permission required",
                    ));
                }
                [("gitlab-open", "Open"), ("gitlab-closed", "Closed")]
                    .into_iter()
                    .map(|(id, title)| BoardColumn {
                        id: id.into(),
                        title: title.into(),
                    })
                    .collect()
            }
            BoardSource::Local => return Err(Error::invalid("Select a remote board destination")),
        };
        return Ok(Board {
            id: String::new(),
            project_id: String::new(),
            name: String::new(),
            source: source.clone(),
            columns,
            last_synced_at: None,
            error: None,
        });
    }
    Ok(metadata(config, source)?.board)
}
fn metadata(config: &RuntimeConfig, source: &BoardSource) -> Result<RemoteBoard, Error> {
    validate_source(source)?;
    let remote = match source {
        BoardSource::Github { .. } => github::metadata(config, source),
        BoardSource::Gitlab { .. } => gitlab::metadata(config, source),
        BoardSource::Local => Err(Error::invalid("Local boards do not require discovery")),
    }?;
    validate_source(&remote.board.source)?;
    Ok(remote)
}
fn tasks(config: &RuntimeConfig, board: &RemoteBoard) -> Result<Vec<RemoteTask>, Error> {
    let tasks = match &board.board.source {
        BoardSource::Github { .. } => github::tasks(config, board),
        BoardSource::Gitlab { .. } => gitlab::tasks(config, board),
        BoardSource::Local => Err(Error::invalid("Cannot fetch local board")),
    }?;
    for task in &tasks {
        validate_reference(&task.reference)?;
    }
    Ok(tasks)
}
fn merge(
    snapshot: &mut Snapshot,
    id: &str,
    remote: &RemoteBoard,
    incoming: Vec<RemoteTask>,
) -> Result<(), Error> {
    let resolved_id = snapshot.canonical_board_id(id).to_owned();
    let id = resolved_id.as_str();
    let old = snapshot.board(id).map_err(Error::invalid)?.clone();
    let mut memberships = Vec::new();
    for task in incoming {
        let existing = snapshot
            .issues
            .iter()
            .find(|i| {
                i.project_id == old.project_id && i.reference.as_ref() == Some(&task.reference)
            })
            .map(|i| snapshot.canonical_issue_id(&i.id).to_owned());
        let issue_id = if let Some(issue_id) = existing {
            let issue = snapshot
                .issues
                .iter_mut()
                .find(|i| i.id == issue_id)
                .ok_or_else(|| Error::invalid("Task redirect is missing"))?;
            issue.title = task.title;
            issue.body = task.body;
            issue.labels = task.labels;
            issue.id.clone()
        } else {
            let issue_id = uuid::Uuid::new_v4().to_string();
            snapshot.issues.push(Issue {
                id: issue_id.clone(),
                project_id: old.project_id.clone(),
                reference: Some(task.reference),
                title: task.title,
                body: task.body,
                column_id: task.columns.first().cloned().unwrap_or_default(),
                labels: task.labels,
                result: None,
                repository_connection_id: None,
            });
            issue_id
        };
        memberships.push(BoardMembership {
            board_id: id.into(),
            issue_id,
            column_ids: task.columns,
            remote_item_id: Some(task.item_id),
        });
    }
    snapshot.memberships.retain(|m| m.board_id != id);
    snapshot.memberships.extend(memberships);
    let board = snapshot.boards.iter_mut().find(|b| b.id == id).unwrap();
    board.name = remote.board.name.clone();
    board.source = remote.board.source.clone();
    board.columns = remote.board.columns.clone();
    board.last_synced_at = Some(crate::now());
    board.error = None;
    for connection in &mut snapshot.connections {
        if matches!(&connection.kind, ConnectionKind::Board { board_id } if board_id == id) {
            connection.state = ConnectionState::Ready;
            connection.error = None;
        }
    }
    Ok(())
}
fn operation(snapshot: &Snapshot, id: &str) -> Result<ProjectOperation, Error> {
    snapshot
        .operations
        .iter()
        .find(|o| o.id == id)
        .cloned()
        .ok_or_else(|| Error::invalid("Operation not found"))
}

fn preserve_issue_identity(
    snapshot: &mut Snapshot,
    id: &str,
    reference: &IssueRef,
) -> Result<(), Error> {
    let id = snapshot.canonical_issue_id(id).to_owned();
    let project = snapshot
        .issue(&id)
        .map_err(Error::invalid)?
        .project_id
        .clone();
    let duplicates: Vec<_> = snapshot
        .issues
        .iter()
        .filter(|i| {
            i.project_id == project && i.id != id && i.reference.as_ref() == Some(reference)
        })
        .map(|i| i.id.clone())
        .collect();
    for duplicate in duplicates {
        snapshot.issue_aliases.insert(duplicate, id.clone());
    }
    // Keep original task/session/scope records, while every current board shows
    // one task. This also handles a sync that imported the accepted issue before
    // publication finished storing its source reference.
    let mut memberships: Vec<BoardMembership> = Vec::new();
    for mut member in std::mem::take(&mut snapshot.memberships) {
        member.issue_id = snapshot.canonical_issue_id(&member.issue_id).to_owned();
        if let Some(existing) = memberships
            .iter_mut()
            .find(|m| m.board_id == member.board_id && m.issue_id == member.issue_id)
        {
            for column in member.column_ids {
                if !existing.column_ids.contains(&column) {
                    existing.column_ids.push(column);
                }
            }
            if existing.remote_item_id.is_none() {
                existing.remote_item_id = member.remote_item_id;
            }
        } else {
            memberships.push(member);
        }
    }
    snapshot.memberships = memberships;
    Ok(())
}

fn preserve_board_identity(snapshot: &mut Snapshot, id: &str) -> Result<(), Error> {
    let board = snapshot.board(id).map_err(Error::invalid)?.clone();
    let duplicates: Vec<_> = snapshot
        .boards
        .iter()
        .filter(|b| {
            b.project_id == board.project_id
                && b.id != board.id
                && b.source.same_board(&board.source)
        })
        .map(|b| b.id.clone())
        .collect();
    let matches = |connection: &ProjectConnection| {
        connection.project_id == board.project_id
            && matches!(&connection.kind, ConnectionKind::Board {board_id} if board_id == id || duplicates.contains(board_id))
    };
    let preferred = snapshot
        .connections
        .iter()
        .position(|c| {
            matches(c) && matches!(&c.kind,ConnectionKind::Board{board_id} if board_id == id)
        })
        .or_else(|| {
            snapshot
                .connections
                .iter()
                .position(|c| matches(c) && c.enabled)
        })
        .or_else(|| snapshot.connections.iter().position(matches));
    for (index, connection) in snapshot.connections.iter_mut().enumerate() {
        if matches(connection) {
            connection.enabled = preferred == Some(index);
            if connection.enabled {
                connection.kind = ConnectionKind::Board {
                    board_id: board.id.clone(),
                };
                connection.name = board.name.clone();
                connection.state = ConnectionState::Ready;
                connection.error = None;
            }
        }
    }
    if preferred.is_none() {
        snapshot.connections.push(ProjectConnection {
            id: format!("connection-published-{}", board.id),
            project_id: board.project_id.clone(),
            name: board.name.clone(),
            enabled: true,
            state: ConnectionState::Ready,
            error: None,
            kind: ConnectionKind::Board {
                board_id: board.id.clone(),
            },
        });
    }
    for duplicate in duplicates {
        snapshot.board_aliases.insert(duplicate, board.id.clone());
    }
    Ok(())
}
fn journal(workspace: &Workspace, id: &str, key: &str, value: String) -> Result<(), Error> {
    workspace.update_project(|s| {
        let op = s
            .operations
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| Error::invalid("Operation not found"))?;
        op.results.insert(key.into(), value);
        Ok(())
    })
}
fn step(
    workspace: &Workspace,
    id: &str,
    key: &str,
    write: impl FnOnce() -> Result<String, Error>,
) -> Result<String, Error> {
    let op = operation(&workspace.snapshots.borrow(), id)?;
    if let Some(result) = op.results.get(key) {
        return Ok(result.clone());
    }
    if op
        .results
        .get("pending")
        .is_some_and(|p| !op.results.contains_key(p))
    {
        return Err(Error::new(
            axum::http::StatusCode::CONFLICT,
            "needs_reconciliation",
            "Remote write outcome is unknown; supply the known result for the pending step before resuming",
        ));
    }
    journal(workspace, id, "pending", key.into())?;
    let result = write().map_err(|_| {
        Error::new(
            axum::http::StatusCode::CONFLICT,
            "needs_reconciliation",
            "Remote write outcome is unknown; verify it remotely and reconcile the pending step",
        )
    })?;
    // Failure saving an accepted result leaves pending in storage, preventing replay.
    journal(workspace, id, key, result.clone())?;
    Ok(result)
}
fn confirmed(value: String) -> Result<(), Error> {
    if value == "confirmed" {
        Ok(())
    } else {
        Err(Error::invalid("Reconciliation result must be confirmed"))
    }
}
fn encode<T: serde::Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value).map_err(Error::internal)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, Error> {
    serde_json::from_str(value).map_err(|_| Error::invalid("Invalid reconciled provider result"))
}

pub(super) fn execute(workspace: &Workspace, operation_id: &str) -> Result<(), Error> {
    let result = execute_inner(workspace, operation_id);
    if let Err(error) = &result {
        workspace.update_project(|s| {
            let op = s
                .operations
                .iter_mut()
                .find(|o| o.id == operation_id)
                .ok_or_else(|| Error::invalid("Operation not found"))?;
            op.state = if error.code == "needs_reconciliation"
                || op
                    .results
                    .get("pending")
                    .is_some_and(|p| !op.results.contains_key(p))
            {
                OperationState::NeedsReconciliation
            } else {
                OperationState::Failed
            };
            op.error = Some(error.to_string());
            if let OperationKind::Sync { board_id } = &op.kind
                && let Some(board) = s.boards.iter_mut().find(|b| b.id == *board_id)
            {
                board.error = Some(error.to_string());
            }
            Ok(())
        })?;
    }
    result
}
fn execute_inner(workspace: &Workspace, id: &str) -> Result<(), Error> {
    let snapshot = workspace.snapshots.borrow().clone();
    let op = operation(&snapshot, id)?;
    if op.state == OperationState::Completed {
        return Ok(());
    }
    match &op.kind {
        OperationKind::Sync { board_id } => {
            let source = snapshot
                .board(board_id)
                .map_err(Error::invalid)?
                .source
                .clone();
            if source != BoardSource::Local {
                let remote = metadata(&workspace.config, &source)?;
                let incoming = tasks(&workspace.config, &remote)?;
                workspace.update_project(|s| merge(s, board_id, &remote, incoming))?;
            }
        }
        OperationKind::Publish {
            board_id,
            target,
            columns,
            tasks: publications,
        } => publish(workspace, &op, board_id, target, columns, publications)?,
        OperationKind::EditTask {
            issue_id,
            title,
            body,
        } => {
            let task = snapshot
                .issues
                .iter()
                .find(|i| i.id == *issue_id)
                .ok_or_else(|| Error::invalid("Task not found"))?;
            let reference = task
                .reference
                .as_ref()
                .ok_or_else(|| Error::invalid("Local task edits belong to the dispatcher"))?;
            validate_reference(reference)?;
            check_issue_permissions(&workspace.config, reference)?;
            confirmed(step(workspace, id, &format!("edit/{issue_id}"), || {
                edit(&workspace.config, reference, title, body)?;
                Ok("confirmed".into())
            })?)?;
            let accepted = read_issue(&workspace.config, reference)?;
            workspace.update_project(|s| {
                let resolved_issue_id = s.canonical_issue_id(issue_id).to_owned();
                let issue = s
                    .issues
                    .iter_mut()
                    .find(|i| i.id == resolved_issue_id)
                    .ok_or_else(|| Error::invalid("Task not found"))?;
                issue.title = text(&accepted, "title")?;
                issue.body = if reference.provider == Provider::Github {
                    text(&accepted, "body")?
                } else if accepted["description"].is_null() {
                    String::new()
                } else {
                    text(&accepted, "description")?
                };
                Ok(())
            })?;
        }
        OperationKind::MoveTask {
            board_id,
            issue_id,
            column_id,
        } => {
            let board = metadata(
                &workspace.config,
                &snapshot.board(board_id).map_err(Error::invalid)?.source,
            )?;
            writable(&board)?;
            validate_column(&board, column_id)?;
            let issue = snapshot
                .issues
                .iter()
                .find(|i| i.id == *issue_id)
                .ok_or_else(|| Error::invalid("Task not found"))?;
            let reference = issue
                .reference
                .as_ref()
                .ok_or_else(|| Error::invalid("Task has no remote reference"))?;
            check_destination(&workspace.config, &board.board.source, reference)?;
            let member = snapshot
                .memberships
                .iter()
                .find(|m| {
                    m.board_id == snapshot.canonical_board_id(board_id)
                        && m.issue_id == snapshot.canonical_issue_id(issue_id)
                })
                .ok_or_else(|| Error::invalid("Task is not on board"))?;
            let item = member
                .remote_item_id
                .as_ref()
                .ok_or_else(|| Error::invalid("Remote membership missing"))?;
            confirmed(step(workspace, id, &format!("status/{issue_id}"), || {
                move_issue(&workspace.config, &board, reference, item, column_id)?;
                Ok("confirmed".into())
            })?)?;
            let incoming = tasks(&workspace.config, &board)?;
            workspace.update_project(|s| merge(s, board_id, &board, incoming))?;
        }
        OperationKind::CreateTask { board_id, issue_id } => {
            create_task(workspace, &op, board_id, issue_id)?
        }
        OperationKind::Clone { .. } => {
            return Err(Error::invalid("Clone operations belong to the dispatcher"));
        }
    }
    workspace.update_project(|s| {
        let op = s
            .operations
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| Error::invalid("Operation not found"))?;
        op.state = OperationState::Completed;
        op.error = None;
        Ok(())
    })
}

fn reference_host(reference: &IssueRef) -> Result<String, Error> {
    reference
        .url
        .strip_prefix("https://")
        .and_then(|s| s.split_once('/'))
        .map(|(host, _)| host.to_string())
        .filter(|s| safe_host(s))
        .ok_or_else(|| Error::invalid("Remote issue requires a valid HTTPS URL"))
}
fn validate_reference(reference: &IssueRef) -> Result<(), Error> {
    let host = reference_host(reference)?;
    if reference.number == 0
        || !reference.repository.split('/').all(safe_segment)
        || !reference.repository.contains('/')
        || reference.provider == Provider::Github && host != "github.com"
    {
        return Err(Error::invalid("Invalid remote issue reference"));
    }
    let expected = format!(
        "https://{host}/{}{}/{}",
        reference.repository,
        if reference.provider == Provider::Github {
            "/issues"
        } else {
            "/-/issues"
        },
        reference.number
    );
    if reference.url != expected {
        return Err(Error::invalid(
            "Remote issue reference URL does not match repository and number",
        ));
    }
    Ok(())
}
fn repository(
    snapshot: &Snapshot,
    project: &str,
    id: &str,
    source: &BoardSource,
) -> Result<String, Error> {
    let connection = snapshot
        .connections
        .iter()
        .find(|c| {
            c.id == id && c.project_id == project && c.enabled && c.state == ConnectionState::Ready
        })
        .ok_or_else(|| Error::invalid("Select a ready repository connection in this project"))?;
    let ConnectionKind::Repository { remote, .. } = &connection.kind else {
        return Err(Error::invalid(
            "Task destination is not a repository connection",
        ));
    };
    let host = match source {
        BoardSource::Github { .. } => "github.com",
        BoardSource::Gitlab { host, .. } => host,
        BoardSource::Local => return Err(Error::invalid("Select remote destination")),
    };
    let path = remote
        .strip_prefix(&format!("git@{host}:"))
        .or_else(|| remote.strip_prefix(&format!("https://{host}/")))
        .or_else(|| remote.strip_prefix(&format!("ssh://git@{host}/")))
        .ok_or_else(|| {
            Error::invalid("Task repository provider/host does not match destination")
        })?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    if !path.contains('/')
        || !path.split('/').all(safe_segment)
        || matches!(source, BoardSource::Github { .. }) && path.split('/').count() != 2
    {
        return Err(Error::invalid("Invalid destination repository"));
    }
    Ok(path.into())
}
#[cfg(test)]
fn reference_for(source: &BoardSource, repo: &str, number: u64) -> IssueRef {
    let (provider, host, part) = match source {
        BoardSource::Github { .. } => (Provider::Github, "github.com", "issues"),
        BoardSource::Gitlab { host, .. } => (Provider::Gitlab, host.as_str(), "-/issues"),
        BoardSource::Local => unreachable!(),
    };
    IssueRef {
        provider,
        repository: repo.into(),
        number,
        url: format!("https://{host}/{repo}/{part}/{number}"),
    }
}
fn check_repository(config: &RuntimeConfig, source: &BoardSource, repo: &str) -> Result<(), Error> {
    check_repository_access(config, source, repo, true)
}
fn check_repository_access(
    config: &RuntimeConfig,
    source: &BoardSource,
    repo: &str,
    issue_write: bool,
) -> Result<(), Error> {
    match source {
        BoardSource::Github { .. } => {
            let v = github::rest(config, &format!("repos/{repo}"), "GET", json!({}))?;
            if issue_write && v["permissions"]["push"] != true {
                return Err(Error::invalid("Repository write permission is required"));
            }
            if issue_write && (v["has_issues"] == false || v["archived"] == true) {
                return Err(Error::invalid(
                    "Destination repository issues are disabled or archived",
                ));
            }
        }
        BoardSource::Gitlab {
            host, group, path, ..
        } => {
            if !*group && path != repo || *group && !repo.starts_with(&format!("{path}/")) {
                return Err(Error::invalid(
                    "Repository is outside destination board scope",
                ));
            }
            let project_source = BoardSource::Gitlab {
                host: host.clone(),
                group: false,
                path: repo.into(),
                number: 1,
                url: String::new(),
            };
            if !gitlab::can_write(config, &project_source)? {
                return Err(Error::invalid(
                    "GitLab project write permission is required",
                ));
            }
            let v = gitlab::rest(
                config,
                host,
                &format!("projects/{}", gitlab::escaped(repo)),
                "GET",
                json!({}),
            )?;
            if v["issues_enabled"] == false || v["archived"] == true {
                return Err(Error::invalid(
                    "Destination project issues are disabled or archived",
                ));
            }
        }
        BoardSource::Local => return Err(Error::invalid("Remote destination required")),
    }
    Ok(())
}
fn check_destination(
    config: &RuntimeConfig,
    source: &BoardSource,
    reference: &IssueRef,
) -> Result<(), Error> {
    validate_reference(reference)?;
    let (provider, host) = match source {
        BoardSource::Github { .. } => (Provider::Github, "github.com"),
        BoardSource::Gitlab { host, .. } => (Provider::Gitlab, host.as_str()),
        BoardSource::Local => return Err(Error::invalid("Remote board required")),
    };
    if provider != reference.provider || reference_host(reference)? != host {
        return Err(Error::invalid(
            "Task provider/host does not match destination",
        ));
    }
    // Projects v2 membership/status writes require project permission and issue
    // read access, not write permission in every external issue repository.
    check_repository_access(config, source, &reference.repository, false)
}
fn same_repository(provider: &Provider, left: &str, right: &str) -> bool {
    if *provider == Provider::Github {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}
fn check_issue_permissions(config: &RuntimeConfig, reference: &IssueRef) -> Result<(), Error> {
    let source = match reference.provider {
        Provider::Github => BoardSource::Github {
            owner: reference.repository.split('/').next().unwrap().into(),
            number: 1,
            url: String::new(),
        },
        Provider::Gitlab => BoardSource::Gitlab {
            host: reference_host(reference)?,
            group: false,
            path: reference.repository.clone(),
            number: 1,
            url: String::new(),
        },
    };
    check_repository(config, &source, &reference.repository)
}
fn writable(board: &RemoteBoard) -> Result<(), Error> {
    if board
        .label_lists
        .iter()
        .any(|(_, label)| label.contains(','))
    {
        return Err(Error::invalid(
            "GitLab boards with comma-containing label names are read-only",
        ));
    }
    if board.writable {
        Ok(())
    } else {
        Err(Error::invalid("Board write permission is required"))
    }
}
fn validate_column(board: &RemoteBoard, id: &str) -> Result<(), Error> {
    if !board.board.columns.iter().any(|c| c.id == id) {
        return Err(Error::invalid(
            "Column mapping references an unknown destination column",
        ));
    }
    if board
        .label_lists
        .iter()
        .any(|(list, label)| list == id && label.contains(','))
    {
        return Err(Error::invalid(
            "GitLab label names containing commas cannot be moved",
        ));
    }
    Ok(())
}
fn read_issue(config: &RuntimeConfig, reference: &IssueRef) -> Result<Value, Error> {
    validate_reference(reference)?;
    match reference.provider {
        Provider::Github => github::rest(
            config,
            &format!("repos/{}/issues/{}", reference.repository, reference.number),
            "GET",
            json!({}),
        ),
        Provider::Gitlab => gitlab::rest(
            config,
            &reference_host(reference)?,
            &format!(
                "projects/{}/issues/{}",
                gitlab::escaped(&reference.repository),
                reference.number
            ),
            "GET",
            json!({}),
        ),
    }
}
fn edit(
    config: &RuntimeConfig,
    reference: &IssueRef,
    title: &str,
    body: &str,
) -> Result<(), Error> {
    match reference.provider {
        Provider::Github => {
            github::rest(
                config,
                &format!("repos/{}/issues/{}", reference.repository, reference.number),
                "PATCH",
                json!({"title":title,"body":body}),
            )?;
        }
        Provider::Gitlab => {
            gitlab::rest(
                config,
                &reference_host(reference)?,
                &format!(
                    "projects/{}/issues/{}",
                    gitlab::escaped(&reference.repository),
                    reference.number
                ),
                "PUT",
                json!({"title":title,"description":body}),
            )?;
        }
    }
    Ok(())
}
fn create_issue(
    config: &RuntimeConfig,
    source: &BoardSource,
    repo: &str,
    task: &Issue,
    operation_id: &str,
) -> Result<IssueRef, Error> {
    // Only explicit task body, never messages, results, comments or transcripts.
    let body = format!(
        "{}\n\n<!-- relay-operation:{operation_id}:task:{} -->",
        task.body, task.id
    );
    let reference = match source {
        BoardSource::Github { .. } => {
            let v = github::rest(
                config,
                &format!("repos/{repo}/issues"),
                "POST",
                json!({"title":task.title,"body":body}),
            )?;
            let reference = issue_reference_from_url(source, &text(&v, "html_url")?)?;
            if reference.number != number(&v, "number")?
                || !same_repository(&reference.provider, &reference.repository, repo)
            {
                return Err(Error::invalid(
                    "Provider returned a different issue repository or number",
                ));
            }
            reference
        }
        BoardSource::Gitlab { host, .. } => {
            let v = gitlab::rest(
                config,
                host,
                &format!("projects/{}/issues", gitlab::escaped(repo)),
                "POST",
                json!({"title":task.title,"description":body}),
            )?;
            IssueRef {
                provider: Provider::Gitlab,
                repository: repo.into(),
                number: number(&v, "iid")?,
                url: text(&v, "web_url")?,
            }
        }
        BoardSource::Local => unreachable!(),
    };
    validate_reference(&reference)?;
    Ok(reference)
}
fn add_issue(
    config: &RuntimeConfig,
    board: &RemoteBoard,
    reference: &IssueRef,
) -> Result<String, Error> {
    match &board.board.source {
        BoardSource::Github { .. } => github::add(config, board, reference),
        BoardSource::Gitlab { .. } => {
            let issue = read_issue(config, reference)?;
            Ok(number(&issue, "id")?.to_string())
        }
        BoardSource::Local => unreachable!(),
    }
}
fn move_issue(
    config: &RuntimeConfig,
    board: &RemoteBoard,
    reference: &IssueRef,
    item: &str,
    column: &str,
) -> Result<(), Error> {
    match &board.board.source {
        BoardSource::Github { .. } => github::move_task(config, board, item, column),
        BoardSource::Gitlab { .. } => gitlab::move_task(config, board, reference, column),
        BoardSource::Local => unreachable!(),
    }
}
fn attach_task(
    workspace: &Workspace,
    op: &ProjectOperation,
    board: &RemoteBoard,
    task: &Issue,
    repo: &str,
    column: &str,
) -> Result<IssueRef, Error> {
    let reference = if let Some(reference) = &task.reference {
        reference.clone()
    } else {
        decode(&step(
            workspace,
            &op.id,
            &format!("issue/{}", task.id),
            || {
                encode(&create_issue(
                    &workspace.config,
                    &board.board.source,
                    repo,
                    task,
                    &op.id,
                )?)
            },
        )?)?
    };
    check_destination(&workspace.config, &board.board.source, &reference)?;
    if !same_repository(&reference.provider, &reference.repository, repo) {
        return Err(Error::invalid(
            "Accepted issue repository does not match selected task repository",
        ));
    }
    // Durable source assignment precedes membership. Local membership survives partial publish.
    workspace.update_project(|s| {
        preserve_issue_identity(s, &task.id, &reference)?;
        let issue = s
            .issues
            .iter_mut()
            .find(|i| i.id == task.id)
            .ok_or_else(|| Error::invalid("Task not found"))?;
        issue.reference = Some(reference.clone());
        Ok(())
    })?;
    let item = step(
        workspace,
        &op.id,
        &format!("membership/{}", task.id),
        || add_issue(&workspace.config, board, &reference),
    )?;
    if item.is_empty() {
        return Err(Error::invalid("Reconciled membership ID must not be empty"));
    }
    if let OperationKind::CreateTask { board_id, .. } = &op.kind {
        // Board addition is confirmed; status is still unconfirmed. Publication
        // intentionally retains the local membership until its final swap.
        workspace.update_project(|s| {
            if let Some(member) = s
                .memberships
                .iter_mut()
                .find(|m| m.board_id == *board_id && m.issue_id == task.id)
            {
                member.remote_item_id = Some(item.clone());
            } else {
                s.memberships.push(BoardMembership {
                    board_id: board_id.clone(),
                    issue_id: task.id.clone(),
                    column_ids: Vec::new(),
                    remote_item_id: Some(item.clone()),
                });
            }
            Ok(())
        })?;
    }
    confirmed(step(
        workspace,
        &op.id,
        &format!("status/{}", task.id),
        || {
            move_issue(&workspace.config, board, &reference, &item, column)?;
            Ok("confirmed".into())
        },
    )?)?;
    Ok(reference)
}
fn create_task(
    workspace: &Workspace,
    op: &ProjectOperation,
    board_id: &str,
    issue_id: &str,
) -> Result<(), Error> {
    let snapshot = workspace.snapshots.borrow().clone();
    let board = metadata(
        &workspace.config,
        &snapshot.board(board_id).map_err(Error::invalid)?.source,
    )?;
    writable(&board)?;
    let task = snapshot
        .issues
        .iter()
        .find(|i| i.id == issue_id && i.project_id == op.project_id)
        .ok_or_else(|| Error::invalid("Pending task not found"))?;
    let column = snapshot
        .memberships
        .iter()
        .find(|m| m.board_id == board_id && m.issue_id == issue_id)
        .and_then(|m| m.column_ids.first())
        .unwrap_or(&task.column_id);
    validate_column(&board, column)?;
    let connection = task
        .repository_connection_id
        .as_ref()
        .ok_or_else(|| Error::invalid("Select a repository connection for remote task creation"))?;
    let repo = repository(&snapshot, &op.project_id, connection, &board.board.source)?;
    check_repository_access(
        &workspace.config,
        &board.board.source,
        &repo,
        task.reference.is_none() && !op.results.contains_key(&format!("issue/{}", task.id)),
    )?;
    if let Some(reference) = &task.reference {
        check_destination(&workspace.config, &board.board.source, reference)?;
    }
    attach_task(workspace, op, &board, task, &repo, column)?;
    let incoming = tasks(&workspace.config, &board)?;
    workspace.update_project(|s| merge(s, board_id, &board, incoming))
}

fn source_number(source: &BoardSource) -> u64 {
    match source {
        BoardSource::Github { number, .. } | BoardSource::Gitlab { number, .. } => *number,
        BoardSource::Local => 0,
    }
}
/// New GitHub destinations map by default option name using `name:Todo`,
/// `name:In Progress`, `name:Done`, `name:No status`. New GitLab destinations
/// support `gitlab-open`/`gitlab-closed`; configure label lists on an existing board.
fn publish(
    workspace: &Workspace,
    op: &ProjectOperation,
    board_id: &str,
    target: &PublishTarget,
    columns: &[ColumnMapping],
    publications: &[TaskPublication],
) -> Result<(), Error> {
    let snapshot = workspace.snapshots.borrow().clone();
    let local = snapshot.board(board_id).map_err(Error::invalid)?;
    if local.source != BoardSource::Local {
        return Err(Error::invalid("Only local boards can be published"));
    }
    validate_source(&target.source)?;
    let new = source_number(&target.source) == 0;
    let mut remote = if new {
        None
    } else {
        Some(metadata(&workspace.config, &target.source)?)
    };
    if let Some(remote) = &remote {
        writable(remote)?;
    } else {
        match &target.source {
            BoardSource::Github { owner, .. } => {
                github::check_creation(&workspace.config, owner)?;
            }
            BoardSource::Gitlab { .. } => {
                if !gitlab::can_write(&workspace.config, &target.source)? {
                    return Err(Error::invalid(
                        "Destination board scope write permission required",
                    ));
                }
            }
            BoardSource::Local => unreachable!(),
        }
    }
    let mut mapped = HashSet::new();
    for mapping in columns {
        if !local.columns.iter().any(|c| c.id == mapping.local_id)
            || !mapped.insert(mapping.local_id.clone())
        {
            return Err(Error::invalid("Invalid or duplicate local column mapping"));
        }
        if let Some(remote) = &remote {
            validate_column(remote, &mapping.remote_id)?;
        } else {
            let valid = match target.source {
                BoardSource::Github { .. } => [
                    "name:Todo",
                    "name:In Progress",
                    "name:Done",
                    "name:No status",
                ]
                .contains(&mapping.remote_id.as_str()),
                BoardSource::Gitlab { .. } => {
                    ["gitlab-open", "gitlab-closed"].contains(&mapping.remote_id.as_str())
                }
                BoardSource::Local => false,
            };
            if !valid {
                return Err(Error::invalid(
                    "New board column mapping is unsupported; use named default GitHub columns or GitLab Open/Closed",
                ));
            }
        }
    }
    if mapped.len() != local.columns.len() {
        return Err(Error::invalid("Map every local column before publishing"));
    }
    let members: Vec<_> = snapshot
        .memberships
        .iter()
        .filter(|m| m.board_id == board_id)
        .collect();
    let mut selected = HashSet::new();
    let mut prepared = Vec::new();
    for publication in publications {
        if !selected.insert(publication.issue_id.clone()) {
            return Err(Error::invalid("Duplicate task publication"));
        }
        let member = members
            .iter()
            .find(|m| m.issue_id == publication.issue_id)
            .ok_or_else(|| Error::invalid("Publication task is not on local board"))?;
        // The contract represents one explicit destination for publication; GitLab
        // already-mirrored tasks can subsequently occupy multiple label lists.
        if member.column_ids.len() != 1 {
            return Err(Error::invalid(
                "Local publication requires one column per task",
            ));
        }
        let mapping = columns
            .iter()
            .find(|c| c.local_id == member.column_ids[0])
            .ok_or_else(|| Error::invalid("Task column mapping missing"))?;
        let task = snapshot
            .issues
            .iter()
            .find(|i| i.id == publication.issue_id && i.project_id == op.project_id)
            .ok_or_else(|| Error::invalid("Task not found in project"))?;
        let repo = repository(
            &snapshot,
            &op.project_id,
            &publication.repository_connection_id,
            &target.source,
        )?;
        check_repository_access(
            &workspace.config,
            &target.source,
            &repo,
            task.reference.is_none() && !op.results.contains_key(&format!("issue/{}", task.id)),
        )?;
        if let Some(reference) = &task.reference {
            check_destination(&workspace.config, &target.source, reference)?;
            if !same_repository(&reference.provider, &reference.repository, &repo) {
                return Err(Error::invalid(
                    "Existing issue repository does not match task repository selection",
                ));
            }
        }
        prepared.push((
            task.clone(),
            repo,
            mapping.remote_id.clone(),
            publication.repository_connection_id.clone(),
        ));
    }
    if selected.len() != members.len() {
        return Err(Error::invalid(
            "Select a repository for every local board task",
        ));
    }
    // All deterministic scope/mapping/repository checks precede the first write.
    if new {
        let resolved: BoardSource = decode(&step(workspace, &op.id, "board", || {
            let source = match &target.source {
                BoardSource::Github { .. } => {
                    github::create_board(&workspace.config, &target.source, &target.name)?
                }
                BoardSource::Gitlab { .. } => {
                    gitlab::create_board(&workspace.config, &target.source, &target.name)?
                }
                BoardSource::Local => unreachable!(),
            };
            encode(&source)
        })?)?;
        validate_source(&resolved)?;
        match (&target.source, &resolved) {
            (
                BoardSource::Github { owner: a, .. },
                BoardSource::Github {
                    owner: b, number, ..
                },
            ) if a == b && *number > 0 => {}
            (
                BoardSource::Gitlab {
                    host: a,
                    group: ag,
                    path: ap,
                    ..
                },
                BoardSource::Gitlab {
                    host: b,
                    group: bg,
                    path: bp,
                    number,
                    ..
                },
            ) if a == b && ag == bg && ap == bp && *number > 0 => {}
            _ => {
                return Err(Error::invalid(
                    "Reconciled destination is outside requested board scope",
                ));
            }
        }
        remote = Some(metadata(&workspace.config, &resolved)?);
    }
    let remote = remote.unwrap();
    writable(&remote)?;
    // Resolve generated GitHub option IDs once; journal each before creating issues.
    for (_, _, column, _) in &mut prepared {
        if new && matches!(target.source, BoardSource::Github { .. }) {
            let name = column.strip_prefix("name:").unwrap();
            *column = remote
                .board
                .columns
                .iter()
                .find(|c| c.title == name)
                .map(|c| c.id.clone())
                .ok_or_else(|| {
                    Error::invalid(
                        "New board defaults differ; select an existing board with known column IDs",
                    )
                })?;
        }
        validate_column(&remote, column)?;
    }
    for mapping in columns {
        let id = if new && matches!(target.source, BoardSource::Github { .. }) {
            remote
                .board
                .columns
                .iter()
                .find(|c| Some(c.title.as_str()) == mapping.remote_id.strip_prefix("name:"))
                .ok_or_else(||Error::invalid("New board default column is missing; select a destination with known column IDs"))?
                .id
                .clone()
        } else {
            mapping.remote_id.clone()
        };
        journal(
            workspace,
            &op.id,
            &format!("column/{}", mapping.local_id),
            id,
        )?;
    }
    for (task, repo, column, connection) in prepared {
        attach_task(workspace, op, &remote, &task, &repo, &column)?;
        workspace.update_project(|s| {
            s.issues
                .iter_mut()
                .find(|i| i.id == task.id)
                .ok_or_else(|| Error::invalid("Task not found"))?
                .repository_connection_id = Some(connection);
            Ok(())
        })?;
    }
    // Fetch the combined authoritative board, including pre-existing remote tasks,
    // before replacing local source/columns. Any failure leaves local membership.
    let incoming = tasks(&workspace.config, &remote)?;
    workspace.update_project(|s| {
        merge(s, board_id, &remote, incoming)?;
        preserve_board_identity(s, board_id)?;
        let operation = s
            .operations
            .iter_mut()
            .find(|o| o.id == op.id)
            .ok_or_else(|| Error::invalid("Operation not found"))?;
        operation.state = OperationState::Completed;
        operation.error = None;
        Ok(())
    })
}

/// Bound whole paginated reads in addition to the per-process byte/time bounds.
struct ReadBudget {
    started: std::time::Instant,
    bytes: usize,
}
impl ReadBudget {
    fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
            bytes: 0,
        }
    }
    fn include(&mut self, value: &Value) -> Result<(), Error> {
        self.bytes = self.bytes.saturating_add(value.to_string().len());
        if self.bytes > LIMIT {
            return Err(Error::invalid("Provider collection exceeds 16 MiB limit"));
        }
        self.check()
    }
    fn check(&self) -> Result<(), Error> {
        if self.started.elapsed() >= Duration::from_secs(120) {
            Err(Error::invalid(
                "Provider pagination exceeded its two minute deadline",
            ))
        } else {
            Ok(())
        }
    }
}

/// Resolve an explicit known remote URL into a typed reconciliation result.
/// This function is read-only: it neither journals nor retries any provider write.
/// The dispatcher must still require the exact pending key when accepting it.
pub fn lookup_reconciliation(
    config: &RuntimeConfig,
    snapshot: &Snapshot,
    operation_id: &str,
    url: &str,
) -> Result<(String, String, String), Error> {
    let op = operation(snapshot, operation_id)?;
    if op.state != OperationState::NeedsReconciliation {
        return Err(Error::invalid("Operation does not need reconciliation"));
    }
    let key = op
        .results
        .get("pending")
        .ok_or_else(|| Error::invalid("Operation has no pending provider step"))?
        .clone();
    if op.results.contains_key(&key) {
        return Err(Error::invalid(
            "Pending provider step already has a known result",
        ));
    }
    if key == "board" {
        let OperationKind::Publish { target, .. } = &op.kind else {
            return Err(Error::invalid("Operation has no board creation step"));
        };
        let source = board_source_from_url(&target.source, url)?;
        let board = metadata(config, &source)?;
        return Ok((
            key,
            encode(&board.board.source)?,
            format!("Board: {}", board.board.name),
        ));
    }
    let (namespace, task_id) = key
        .split_once('/')
        .ok_or_else(|| Error::invalid("Unknown provider recovery step"))?;
    let task = snapshot
        .issues
        .iter()
        .find(|i| i.id == task_id && i.project_id == op.project_id)
        .ok_or_else(|| Error::invalid("Recovery task not found"))?;
    let source = match &op.kind {
        OperationKind::Publish { target, tasks, .. }
            if tasks.iter().any(|t| t.issue_id == task_id) =>
        {
            if source_number(&target.source) == 0 {
                decode(
                    op.results
                        .get("board")
                        .ok_or_else(|| Error::invalid("Resolve board creation first"))?,
                )?
            } else {
                target.source.clone()
            }
        }
        OperationKind::CreateTask { board_id, issue_id }
        | OperationKind::MoveTask {
            board_id, issue_id, ..
        } if issue_id == task_id => snapshot
            .board(board_id)
            .map_err(Error::invalid)?
            .source
            .clone(),
        OperationKind::EditTask { issue_id, .. } if issue_id == task_id => {
            let reference = task
                .reference
                .as_ref()
                .ok_or_else(|| Error::invalid("Recovery task has no remote reference"))?;
            match reference.provider {
                Provider::Github => BoardSource::Github {
                    owner: reference
                        .repository
                        .split('/')
                        .next()
                        .unwrap_or_default()
                        .into(),
                    number: 1,
                    url: String::new(),
                },
                Provider::Gitlab => BoardSource::Gitlab {
                    host: reference_host(reference)?,
                    group: false,
                    path: reference.repository.clone(),
                    number: 1,
                    url: String::new(),
                },
            }
        }
        _ => {
            return Err(Error::invalid(
                "Pending step does not belong to this operation",
            ));
        }
    };
    validate_source(&source)?;
    let reference = issue_reference_from_url(&source, url)?;
    if namespace == "issue" {
        let connection = match &op.kind {
            OperationKind::Publish { tasks, .. } => tasks
                .iter()
                .find(|t| t.issue_id == task_id)
                .map(|t| t.repository_connection_id.as_str()),
            OperationKind::CreateTask { .. } => task.repository_connection_id.as_deref(),
            _ => None,
        }
        .ok_or_else(|| Error::invalid("Operation has no task creation step"))?;
        let repo = repository(snapshot, &op.project_id, connection, &source)?;
        if !same_repository(&reference.provider, &repo, &reference.repository) {
            return Err(Error::invalid(
                "Known issue does not match selected repository",
            ));
        }
        let issue = read_issue(config, &reference)?;
        let accepted_number = number(
            &issue,
            if reference.provider == Provider::Github {
                "number"
            } else {
                "iid"
            },
        )?;
        let accepted_url = text(
            &issue,
            if reference.provider == Provider::Github {
                "html_url"
            } else {
                "web_url"
            },
        )?;
        if accepted_number != reference.number || accepted_url != reference.url {
            return Err(Error::invalid("Provider returned a different issue"));
        }
        let body = issue[if reference.provider == Provider::Github {
            "body"
        } else {
            "description"
        }]
        .as_str()
        .unwrap_or_default();
        let marker = format!("<!-- relay-operation:{}:task:{} -->", op.id, task.id);
        if !body.contains(&marker) {
            return Err(Error::invalid(
                "Known issue does not contain this operation's creation marker; verify the issue before supplying an advanced known result",
            ));
        }
        return Ok((
            key,
            encode(&reference)?,
            format!("Issue: {} #{}", reference.repository, reference.number),
        ));
    }
    if task.reference.as_ref() != Some(&reference) {
        return Err(Error::invalid("Known issue URL does not match this task"));
    }
    if namespace == "edit" {
        let OperationKind::EditTask { title, body, .. } = &op.kind else {
            return Err(Error::invalid("Operation has no edit step"));
        };
        let accepted = read_issue(config, &reference)?;
        let remote_body = accepted[if reference.provider == Provider::Github {
            "body"
        } else {
            "description"
        }]
        .as_str()
        .unwrap_or_default();
        if text(&accepted, "title")? != *title || remote_body != body {
            return Err(Error::invalid(
                "Remote issue does not match the requested edit",
            ));
        }
        return Ok((key, "confirmed".into(), "Task edit confirmed".into()));
    }
    let board = metadata(config, &source)?;
    let incoming = tasks(config, &board)?;
    let member = incoming
        .iter()
        .find(|t| t.reference == reference)
        .ok_or_else(|| Error::invalid("Known issue is not a member of the destination board"))?;
    if namespace == "membership" {
        if !matches!(
            op.kind,
            OperationKind::Publish { .. } | OperationKind::CreateTask { .. }
        ) {
            return Err(Error::invalid("Operation has no membership creation step"));
        }
        return Ok((
            key,
            member.item_id.clone(),
            format!("Board membership: {}", board.board.name),
        ));
    }
    if namespace == "status" {
        let column = match &op.kind {
            OperationKind::MoveTask { column_id, .. } => column_id.clone(),
            OperationKind::CreateTask { .. } => task.column_id.clone(),
            OperationKind::Publish { board_id, .. } => {
                let local_column = snapshot
                    .memberships
                    .iter()
                    .find(|m| m.board_id == *board_id && m.issue_id == task.id)
                    .and_then(|m| m.column_ids.first())
                    .ok_or_else(|| Error::invalid("Local publication column is missing"))?;
                op.results
                    .get(&format!("column/{local_column}"))
                    .cloned()
                    .ok_or_else(|| Error::invalid("Resolved publication column is missing"))?
            }
            _ => return Err(Error::invalid("Operation has no status step")),
        };
        if !member.columns.contains(&column) {
            return Err(Error::invalid(
                "Remote board does not confirm the requested status",
            ));
        }
        return Ok((key, "confirmed".into(), "Task status confirmed".into()));
    }
    Err(Error::invalid("Unsupported provider recovery step"))
}
fn positive_number(text: &str) -> Result<u64, Error> {
    text.parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Error::invalid("Remote URL requires a positive number"))
}
fn board_source_from_url(scope: &BoardSource, url: &str) -> Result<BoardSource, Error> {
    let source = match scope {
        BoardSource::Github { owner, .. } => {
            let path = url
                .strip_prefix("https://github.com/")
                .and_then(|p| p.strip_prefix("users/").or_else(|| p.strip_prefix("orgs/")))
                .ok_or_else(|| Error::invalid("Enter a GitHub project URL"))?;
            let (actual_owner, suffix) = path
                .split_once("/projects/")
                .ok_or_else(|| Error::invalid("Enter a GitHub project URL"))?;
            if !owner.eq_ignore_ascii_case(actual_owner) {
                return Err(Error::invalid(
                    "Known board URL is outside destination owner",
                ));
            }
            BoardSource::Github {
                owner: owner.clone(),
                number: positive_number(suffix)?,
                url: url.into(),
            }
        }
        BoardSource::Gitlab {
            host, group, path, ..
        } => {
            let prefix = format!(
                "https://{host}/{}{path}/-/boards/",
                if *group { "groups/" } else { "" }
            );
            let suffix = url
                .strip_prefix(&prefix)
                .ok_or_else(|| Error::invalid("Known board URL is outside destination scope"))?;
            BoardSource::Gitlab {
                host: host.clone(),
                group: *group,
                path: path.clone(),
                number: positive_number(suffix)?,
                url: url.into(),
            }
        }
        BoardSource::Local => return Err(Error::invalid("Remote destination required")),
    };
    validate_source(&source)?;
    Ok(source)
}
fn issue_reference_from_url(source: &BoardSource, url: &str) -> Result<IssueRef, Error> {
    let (provider, host, separator) = match source {
        BoardSource::Github { .. } => (Provider::Github, "github.com", "/issues/"),
        BoardSource::Gitlab { host, .. } => (Provider::Gitlab, host.as_str(), "/-/issues/"),
        BoardSource::Local => return Err(Error::invalid("Remote board required")),
    };
    let path = url
        .strip_prefix(&format!("https://{host}/"))
        .ok_or_else(|| Error::invalid("Known issue URL does not match destination host"))?;
    let (repo, number) = path
        .rsplit_once(separator)
        .ok_or_else(|| Error::invalid("Enter a remote issue URL"))?;
    let reference = IssueRef {
        provider,
        repository: repo.into(),
        number: positive_number(number)?,
        url: url.into(),
    };
    validate_reference(&reference)?;
    if let BoardSource::Gitlab { group, path, .. } = source
        && (!*group && path != repo || *group && !repo.starts_with(&format!("{path}/")))
    {
        return Err(Error::invalid(
            "Known issue is outside destination board scope",
        ));
    }
    Ok(reference)
}
